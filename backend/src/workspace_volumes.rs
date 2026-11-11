use crate::workspace_settings::{filled, text};
use serde_norway::{Mapping, Value};

/// An existing PersistentVolumeClaim of the workspace's namespace that every
/// one of its computers mounts. The claim is created and filled elsewhere, and
/// a computer never creates or deletes it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct WorkspaceVolume {
    name: String,
    pub(crate) claim: String,
    mount_path: String,
    /// For content an agent copies before it changes it, so the original
    /// stays as it is.
    read_only: bool,
}

/// The volumes a workspace entry of `daemon.yaml` lists.
pub(crate) fn volumes_of(entry: &Value) -> Vec<WorkspaceVolume> {
    entry
        .get("volumes")
        .and_then(Value::as_sequence)
        .map(|volumes| volumes.iter().map(volume_of).collect())
        .unwrap_or_default()
}

fn volume_of(entry: &Value) -> WorkspaceVolume {
    WorkspaceVolume {
        name: text(entry, "name"),
        claim: text(entry, "claim"),
        mount_path: text(entry, "mount_path"),
        read_only: entry
            .get("read_only")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// `volumes` as `daemon.yaml` holds them, or none when the workspace has no
/// volume, so the key leaves the file with the last one.
pub(crate) fn listed_volumes(volumes: &[WorkspaceVolume]) -> Option<Value> {
    let entries: Vec<Value> = volumes.iter().map(volume_entry).collect();
    (!entries.is_empty()).then_some(Value::Sequence(entries))
}

fn volume_entry(volume: &WorkspaceVolume) -> Value {
    let mut entry = Mapping::new();
    for (key, value) in [
        ("name", &volume.name),
        ("claim", &volume.claim),
        ("mount_path", &volume.mount_path),
    ] {
        entry.insert(key.into(), filled(value).unwrap_or_else(|| "".into()));
    }
    if volume.read_only {
        entry.insert("read_only".into(), true.into());
    }
    Value::Mapping(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(name: &str, claim: &str, mount_path: &str, read_only: bool) -> WorkspaceVolume {
        WorkspaceVolume {
            name: name.into(),
            claim: claim.into(),
            mount_path: mount_path.into(),
            read_only,
        }
    }

    #[test]
    fn volumes_survive_a_round_trip_and_only_a_read_only_one_says_so() {
        let volumes = vec![
            volume("resolve", "davinci-resolve", "/opt/resolve", false),
            volume("footage", "raw-footage", "/mnt/footage", true),
        ];
        let mut entry = Mapping::new();
        entry.insert("volumes".into(), listed_volumes(&volumes).unwrap());
        let yaml = serde_norway::to_string(&entry).unwrap();
        assert_eq!(yaml.matches("read_only: true").count(), 1);
        assert!(!yaml.contains("read_only: false"));
        assert_eq!(volumes_of(&Value::Mapping(entry)), volumes);
    }

    #[test]
    fn a_workspace_without_volumes_has_no_volumes_key() {
        assert!(listed_volumes(&[]).is_none());
        assert!(volumes_of(&Value::Mapping(Mapping::new())).is_empty());
    }
}
