use crate::workspace_settings::{named_entries, text};
use serde_norway::{Mapping, Value};
use std::path::{Path, PathBuf};

const FOLDERS_FILE: &str = "cluster_folders.yaml";
const FOLDERS_KEY: &str = "folders";
const NAME_MAX: usize = 40;
/// Where the local cluster's node keeps its folders. It lies on the node's own
/// disk, which a pod in a user namespace can mount, unlike a folder shared
/// from this computer.
const NODE_ROOT: &str = "/var/lib/rancher/k3s/folders";
const READ_ONLY_SUFFIX: &str = "-ro";
const CLAIM_PREFIX: &str = "folder-";
const VOLUME_PREFIX: &str = "infer-folder-";

/// A folder of this computer the local cluster's workspaces can mount. A
/// read-only one cannot be changed from any workspace.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ClusterFolder {
    name: String,
    path: String,
    read_only: bool,
}

/// One way a workspace mounts a folder: writable or read-only.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Access {
    Writable,
    ReadOnly,
}

impl Access {
    const ALL: [Access; 2] = [Access::Writable, Access::ReadOnly];

    fn directory(self) -> &'static str {
        match self {
            Access::Writable => "rw",
            Access::ReadOnly => "ro",
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Access::Writable => "",
            Access::ReadOnly => READ_ONLY_SUFFIX,
        }
    }
}

impl ClusterFolder {
    /// The directory on the node a claim mounts. The folder sits one level
    /// below it under its own name, because only a directory of the node's own
    /// disk can be the top of a volume in a user namespace.
    fn node_parent(&self, access: Access) -> String {
        format!("{NODE_ROOT}/{}/{}", self.name, access.directory())
    }

    /// What Docker mounts for `access`. A read-only folder is read-only both
    /// ways, so no workspace can write to it.
    fn bind(&self, access: Access) -> String {
        let target = format!("{}/{}", self.node_parent(access), self.name);
        let read_only = self.read_only || access == Access::ReadOnly;
        let mode = if read_only { ":ro" } else { "" };
        format!("{}:{target}{mode}", self.path)
    }

    fn claim(&self, access: Access) -> String {
        format!("{CLAIM_PREFIX}{}{}", self.name, access.suffix())
    }

    fn volume(&self, access: Access) -> String {
        format!("{VOLUME_PREFIX}{}{}", self.name, access.suffix())
    }
}

fn folders_path() -> PathBuf {
    crate::env::infer_dir().join(FOLDERS_FILE)
}

fn folder_of(entry: &Value) -> ClusterFolder {
    ClusterFolder {
        name: text(entry, "name"),
        path: text(entry, "path"),
        read_only: entry
            .get("read_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn parse_folders(yaml: Option<&str>) -> Vec<ClusterFolder> {
    crate::config::config_mapping(yaml)
        .get(FOLDERS_KEY)
        .and_then(Value::as_sequence)
        .map(|entries| entries.iter().map(folder_of).collect())
        .unwrap_or_default()
}

/// The folders the local cluster shares, as this computer last saved them.
pub(crate) fn read_folders() -> Vec<ClusterFolder> {
    parse_folders(std::fs::read_to_string(folders_path()).ok().as_deref())
}

fn folder_entry(folder: &ClusterFolder) -> Value {
    let mut entry = Mapping::new();
    entry.insert("name".into(), folder.name.trim().into());
    entry.insert("path".into(), folder.path.trim().into());
    if folder.read_only {
        entry.insert("read_only".into(), true.into());
    }
    Value::Mapping(entry)
}

/// Whether `name` can name a claim and a directory: lowercase letters, digits
/// and dashes. One ending in `-ro` would read as another folder's read-only
/// claim.
fn valid_name(name: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    name.len() <= NAME_MAX
        && edge(name.chars().next())
        && edge(name.chars().last())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with(READ_ONLY_SUFFIX)
}

/// Why the folders cannot be shared as listed, checked before anything is
/// saved. `is_dir` says whether a path is a directory of this computer.
fn folders_problem(folders: &[ClusterFolder], is_dir: impl Fn(&Path) -> bool) -> Option<String> {
    for (index, folder) in folders.iter().enumerate() {
        let name = folder.name.trim();
        let path = Path::new(folder.path.trim());
        if !valid_name(name) {
            return Some(format!(
                "Folder {} needs a name of up to {NAME_MAX} lowercase letters, digits or dashes that does not end in {READ_ONLY_SUFFIX}",
                index + 1
            ));
        }
        if folders[..index]
            .iter()
            .any(|other| other.name.trim() == name)
        {
            return Some(format!("Two folders are named {name}"));
        }
        if !path.is_absolute() || folder.path.contains(':') {
            return Some(format!(
                "Folder {name} needs an absolute path without a colon"
            ));
        }
        if !is_dir(path) {
            return Some(format!(
                "{} is not a folder on this computer",
                path.display()
            ));
        }
    }
    None
}

fn merge_folders(yaml: Option<&str>, folders: &[ClusterFolder]) -> Result<String, String> {
    let mut document = crate::config::config_mapping(yaml);
    let root = document
        .as_mapping_mut()
        .ok_or("yaml root is not a mapping")?;
    *named_entries(root, FOLDERS_KEY, FOLDERS_FILE)? = folders.iter().map(folder_entry).collect();
    serde_norway::to_string(&document).map_err(|e| e.to_string())
}

/// What `docker run` mounts so the node holds every folder both ways, sorted
/// so two lists of the same folders compare equal.
pub(crate) fn binds(folders: &[ClusterFolder]) -> Vec<String> {
    let mut binds: Vec<String> = folders
        .iter()
        .flat_map(|folder| Access::ALL.map(|access| folder.bind(access)))
        .collect();
    binds.sort();
    binds
}

fn volume_manifest(folder: &ClusterFolder, access: Access, namespace: &str) -> String {
    let volume = folder.volume(access);
    let claim = folder.claim(access);
    let path = folder.node_parent(access);
    format!(
        "apiVersion: v1\nkind: PersistentVolume\nmetadata:\n  name: {volume}\nspec:\n  capacity:\n    storage: 1Gi\n  accessModes:\n    - ReadWriteOnce\n  storageClassName: \"\"\n  persistentVolumeReclaimPolicy: Retain\n  claimRef:\n    namespace: {namespace}\n    name: {claim}\n  hostPath:\n    path: {path}\n---\napiVersion: v1\nkind: PersistentVolumeClaim\nmetadata:\n  name: {claim}\n  namespace: {namespace}\nspec:\n  accessModes:\n    - ReadWriteOnce\n  storageClassName: \"\"\n  volumeName: {volume}\n  resources:\n    requests:\n      storage: 1Gi\n"
    )
}

/// The volumes and claims that give `namespace` every folder both ways, as
/// one manifest, or none when the cluster shares no folder.
pub(crate) fn claims_manifest(folders: &[ClusterFolder], namespace: &str) -> Option<String> {
    let documents: Vec<String> = folders
        .iter()
        .flat_map(|folder| Access::ALL.map(|access| volume_manifest(folder, access, namespace)))
        .collect();
    (!documents.is_empty()).then(|| documents.join("---\n"))
}

/// The folders the local cluster shares with its workspaces.
#[tauri::command]
pub(crate) fn cluster_folders() -> Vec<ClusterFolder> {
    read_folders()
}

/// Saves the folders the local cluster shares. The cluster's node mounts them
/// only when it is created, so they take effect once it is provisioned again.
#[tauri::command]
pub(crate) fn save_cluster_folders(folders: Vec<ClusterFolder>) -> Result<(), String> {
    if let Some(problem) = folders_problem(&folders, Path::is_dir) {
        return Err(problem);
    }
    crate::config::update_file(&folders_path(), |existing| {
        merge_folders(existing, &folders)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, path: &str, read_only: bool) -> ClusterFolder {
        ClusterFolder {
            name: name.into(),
            path: path.into(),
            read_only,
        }
    }

    #[test]
    fn folders_survive_a_round_trip_and_other_keys_stay() {
        let folders = vec![
            folder("footage", "/Users/me/Movies/raw", true),
            folder("apps", "/Users/me/apps", false),
        ];
        let saved = merge_folders(Some("note: kept\n"), &folders).unwrap();
        assert!(saved.contains("note: kept"));
        assert_eq!(saved.matches("read_only: true").count(), 1);
        assert_eq!(parse_folders(Some(&saved)), folders);
        assert!(parse_folders(Some(&merge_folders(Some(&saved), &[]).unwrap())).is_empty());
    }

    #[test]
    fn the_node_mounts_each_folder_below_a_directory_of_its_own_disk_both_ways() {
        let binds = binds(&[folder("footage", "/Users/me/Movies/raw", false)]);
        assert_eq!(
            binds,
            [
                "/Users/me/Movies/raw:/var/lib/rancher/k3s/folders/footage/ro/footage:ro",
                "/Users/me/Movies/raw:/var/lib/rancher/k3s/folders/footage/rw/footage",
            ]
        );
    }

    #[test]
    fn a_read_only_folder_is_never_mounted_writable() {
        let binds = binds(&[folder("footage", "/Users/me/Movies/raw", true)]);
        assert!(binds.iter().all(|bind| bind.ends_with(":ro")));
    }

    #[test]
    fn a_folder_gets_a_writable_and_a_read_only_claim_on_its_parent_directory() {
        let manifest = claims_manifest(&[folder("footage", "/Users/me/raw", false)], "agents");
        let manifest = manifest.unwrap();
        let documents: Vec<Value> = manifest
            .split("---\n")
            .map(|document| serde_norway::from_str(document).unwrap())
            .collect();
        assert_eq!(documents.len(), 4);
        assert_eq!(documents[0]["metadata"]["name"], "infer-folder-footage");
        assert_eq!(
            documents[0]["spec"]["hostPath"]["path"],
            "/var/lib/rancher/k3s/folders/footage/rw"
        );
        assert_eq!(documents[0]["spec"]["claimRef"]["name"], "folder-footage");
        assert_eq!(documents[1]["kind"], "PersistentVolumeClaim");
        assert_eq!(documents[1]["metadata"]["namespace"], "agents");
        assert_eq!(documents[3]["metadata"]["name"], "folder-footage-ro");
        assert_eq!(
            documents[2]["spec"]["hostPath"]["path"],
            "/var/lib/rancher/k3s/folders/footage/ro"
        );
        assert!(claims_manifest(&[], "agents").is_none());
    }

    #[test]
    fn folders_need_a_claim_safe_name_of_their_own_and_a_folder_that_exists() {
        let exists = |path: &Path| path == Path::new("/Users/me/raw");
        let problem = |folders: &[ClusterFolder]| folders_problem(folders, exists);
        let footage = folder("footage", "/Users/me/raw", false);
        assert_eq!(problem(std::slice::from_ref(&footage)), None);
        for name in ["", "Raw", "raw_footage", "-raw", "footage-ro"] {
            let named = folder(name, "/Users/me/raw", false);
            assert!(problem(&[named]).unwrap().contains("needs a name"));
        }
        let twice = problem(&[footage.clone(), footage.clone()]).unwrap();
        assert!(twice.contains("Two folders are named footage"));
        let relative = problem(&[folder("footage", "Movies/raw", false)]).unwrap();
        assert!(relative.contains("absolute path"));
        let colon = problem(&[folder("footage", "/Users/me/a:b", false)]).unwrap();
        assert!(colon.contains("absolute path"));
        let missing = problem(&[folder("footage", "/Users/me/gone", false)]).unwrap();
        assert!(missing.contains("is not a folder"));
    }
}
