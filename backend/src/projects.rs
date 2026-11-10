pub(crate) mod attachments;
pub(crate) mod checkpoints;
pub(crate) mod git;

use crate::config::read_config;
pub(crate) use attachments::{ProjectFile, list_local_files};
pub(crate) use checkpoints::{checkpoint, ensure_content_repo};
use git::repo_context;
pub(crate) use git::{GitRepo, GitWatcher, clone_repo_under, git_output, git_remote_repo};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Subfolder of the platform Documents directory holding per-project dirs.
const APP_PROJECTS_DIR: &str = "Inference Gateway Desktop";

/// The CLI's own default for INFER_TOOLS_SANDBOX_DIRECTORIES; when we set the
/// env var we must re-state it or the cwd/tmp grants would silently disappear.
const CLI_DEFAULT_SANDBOX_DIRS: &str = ".,/tmp";

/// Platform Documents folder (Tauri's document_dir semantics via the dirs
/// crate), falling back to ~/Documents when the platform lookup fails.
fn document_dir(home: &Path) -> PathBuf {
    dirs::document_dir().unwrap_or_else(|| home.join("Documents"))
}

/// `~`-expanded path; config values may start with `~`.
fn expand_home(raw: &str) -> String {
    raw.strip_prefix('~').map_or_else(
        || raw.to_string(),
        |rest| format!("{}{}", crate::env::home_dir().display(), rest),
    )
}

/// Default projects root: <Documents>/Inference Gateway Desktop.
pub(crate) fn default_projects_root(home: &Path) -> String {
    document_dir(home)
        .join(APP_PROJECTS_DIR)
        .to_string_lossy()
        .to_string()
}

/// Filesystem- and repo-safe project name: anything outside Unicode
/// alphanumerics, space, dash, underscore and dot becomes `-`; edge
/// separators/dots/spaces are trimmed (Windows forbids trailing dots) and an
/// empty result falls back to "project".
pub(crate) fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(['-', '.', ' ']);
    if trimmed.is_empty() {
        "project".into()
    } else {
        trimmed.to_string()
    }
}

/// A sidebar group as a relative directory under the projects root (what
/// `scan_git_repos_in` reports): `..`, absolute and empty groups are rejected.
fn group_rel(group: &str) -> Option<&Path> {
    let group = group.trim().trim_matches('/');
    let rel = Path::new(group);
    let ok = !group.is_empty()
        && rel
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    ok.then_some(rel)
}

/// The project's own folder name: the display name minus a leading
/// `<group>/`, so "core/cli" in group "core" maps to `core/cli`, not
/// `core/core-cli`.
fn leaf_name<'a>(name: &'a str, group: &str) -> &'a str {
    group_rel(group)
        .and_then(|g| name.strip_prefix(&format!("{}/", g.display())))
        .unwrap_or(name)
}

/// Deterministic per-project directory mapping: names are processed in sorted
/// order, sanitized under their group folder (`root/<group>/<leaf>`, or
/// `root/<name>` without a group), and a numeric suffix is appended on
/// collision ("a/b" and "a:b" both sanitize to "a-b"; the second sorted name
/// gets "a-b-2"). Pure function of (root, names, groups) so grants can be
/// re-derived from projects.yaml alone and repeated calls are idempotent.
fn assign_dirs(
    root: &Path,
    names: &[String],
    groups: &BTreeMap<String, String>,
) -> BTreeMap<String, PathBuf> {
    let mut taken: BTreeSet<PathBuf> = BTreeSet::new();
    let mut map = BTreeMap::new();
    for name in names.iter().collect::<BTreeSet<_>>() {
        let group = groups.get(name).map_or("", String::as_str);
        let base = sanitize_name(leaf_name(name, group));
        let parent = group_rel(group).map_or_else(|| root.to_path_buf(), |g| root.join(g));
        let mut dir = parent.join(&base);
        let mut n = 2;
        while !taken.insert(dir.clone()) {
            dir = parent.join(format!("{base}-{n}"));
            n += 1;
        }
        map.insert(name.to_string(), dir);
    }
    map
}

/// The sidebar's persisted state. YAML because people open it to work out why
/// a chat landed in the wrong project, and the CLI carves it out of the agent
/// sandbox so the agent can edit it on the user's behalf.
fn projects_path(home: &Path) -> PathBuf {
    crate::env::infer_dir_in(home).join("projects.yaml")
}

/// Raw projects.yaml as a JSON value (the shape the sidebar and the export
/// both work in); an empty mapping when the file is missing or unparseable, so
/// a fresh install rebuilds its state rather than failing.
pub(crate) fn read_projects_in(home: &Path) -> serde_json::Value {
    std::fs::read_to_string(projects_path(home))
        .ok()
        .and_then(|text| serde_norway::from_str(&text).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

pub(crate) fn write_projects_in(home: &Path, val: &serde_json::Value) -> Result<(), String> {
    let path = projects_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_norway::to_string(val).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Read ~/.infer/projects.yaml as JSON text for the sidebar. The file is YAML
/// on disk; the frontend keeps its JSON view of it, so the conversion lives
/// here rather than shipping a YAML parser to the webview. An empty object
/// when the file does not exist yet.
#[tauri::command]
pub(crate) fn read_projects() -> Result<String, String> {
    serde_json::to_string(&read_projects_in(&crate::env::home_dir())).map_err(|e| e.to_string())
}

/// Write ~/.infer/projects.yaml (entire mapping, atomically replaced).
#[tauri::command]
pub(crate) fn write_projects(data: String) -> Result<(), String> {
    let val: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    write_projects_in(&crate::env::home_dir(), &val)
}

fn projects_state() -> serde_json::Value {
    read_projects_in(&crate::env::home_dir())
}

/// Project names known to the app: the explicit `names` list plus every value
/// in `assignments`, from ~/.infer/projects.yaml (same sources the sidebar uses).
fn project_names() -> Vec<String> {
    let val = projects_state();
    let mut names: BTreeSet<String> = BTreeSet::new();
    if let Some(list) = val.get("names").and_then(|v| v.as_array()) {
        names.extend(
            list.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
        );
    }
    if let Some(assignments) = val.get("assignments").and_then(|v| v.as_object()) {
        names.extend(
            assignments
                .values()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
        );
    }
    names.into_iter().collect()
}

/// Per-project directory overrides from the `paths` object in projects.yaml:
/// trimmed, `~`-expanded, and only absolute paths (a relative override would
/// grant a meaningless relative sandbox entry).
fn project_paths() -> BTreeMap<String, PathBuf> {
    let val = projects_state();
    let Some(paths) = val.get("paths").and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };
    paths
        .iter()
        .filter_map(|(name, v)| {
            let raw = v.as_str()?.trim();
            let p = PathBuf::from(expand_home(raw));
            p.is_absolute().then(|| (name.clone(), p))
        })
        .collect()
}

/// Per-project sidebar group from the `groups` object in projects.yaml.
fn project_groups() -> BTreeMap<String, String> {
    let val = projects_state();
    let Some(groups) = val.get("groups").and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };
    groups
        .iter()
        .filter_map(|(name, v)| Some((name.clone(), v.as_str()?.to_string())))
        .collect()
}

/// Default name->dir mapping with per-project overrides applied on top.
/// ponytail: two projects may point at the same dir; harmless (duplicate
/// grant, shared files) - validate only if users hit it.
fn resolved_dirs(
    root: &Path,
    names: &[String],
    groups: &BTreeMap<String, String>,
    overrides: &BTreeMap<String, PathBuf>,
) -> BTreeMap<String, PathBuf> {
    let mut dirs = assign_dirs(root, names, groups);
    for (name, dir) in overrides {
        if dirs.contains_key(name) {
            dirs.insert(name.clone(), dir.clone());
        }
    }
    dirs
}

/// Comma-separated value for INFER_TOOLS_SANDBOX_DIRECTORIES covering every
/// project directory, so agent runs can read and write project files without
/// approval prompts. None when no projects exist, leaving the CLI default
/// untouched; deleting a project drops its grant on the next spawn.
pub(crate) fn sandbox_allowed_dirs() -> Option<String> {
    let names = project_names();
    if names.is_empty() {
        return None;
    }
    let root = PathBuf::from(read_config().projects_root);
    let dirs = resolved_dirs(&root, &names, &project_groups(), &project_paths());
    let mut value = String::from(CLI_DEFAULT_SANDBOX_DIRS);
    for dir in dirs.values() {
        value.push(',');
        value.push_str(&dir.to_string_lossy());
    }
    Some(value)
}

/// Files directory for a project: the same deterministic mapping the sandbox
/// grant, dir creation, uploads and the agent cwd resolve through. None when
/// the name cannot be mapped.
/// Every directory the desktop spawns agents in: each project's files
/// directory. Conversations stored elsewhere belong to other CLI sessions.
pub(crate) fn project_dirs() -> Vec<PathBuf> {
    let root = PathBuf::from(read_config().projects_root);
    resolved_dirs(&root, &project_names(), &project_groups(), &project_paths())
        .into_values()
        .collect()
}

pub(crate) fn project_dir(name: &str) -> Option<PathBuf> {
    let mut names = project_names();
    if !names.iter().any(|n| n == name) {
        names.push(name.to_string());
    }
    let root = PathBuf::from(read_config().projects_root);
    resolved_dirs(&root, &names, &project_groups(), &project_paths())
        .get(name)
        .cloned()
}

/// Whether the project's resolved directory exists on disk; gates the Init
/// action so a stale import fails visibly instead of cryptically.
#[tauri::command]
pub(crate) fn project_dir_exists(name: String) -> bool {
    project_dir(&name).is_some_and(|dir| dir.is_dir())
}

/// Whether a project is a "content" project (video/timeline), per the `types`
/// map the sidebar persists in projects.yaml. Content projects get a local git
/// repo so agent and manual edits are revertable; code projects do not.
pub(crate) fn project_is_content(name: &str) -> bool {
    projects_state()["types"][name].as_str() == Some("content")
}

/// Platform command that opens a folder in VS Code: `open -a` on macOS, the
/// `code` CLI elsewhere. Both exit promptly after handing the folder over.
fn vscode_launch(dir: &Path) -> std::process::Command {
    #[cfg(target_os = "macos")]
    let mut cmd = crate::env::command("open");
    #[cfg(not(target_os = "macos"))]
    let mut cmd = crate::env::command("code");
    #[cfg(target_os = "macos")]
    cmd.arg("-a").arg("Visual Studio Code");
    cmd.arg(dir);
    cmd
}

/// Open `dir` in VS Code via the injected launcher, mirroring `ensure_clone`:
/// a directory that does not exist fails before the launcher runs and names
/// the path, so a stale import fails visibly instead of cryptically.
fn open_in_vs_code_with(
    launch: impl FnOnce(&Path) -> Result<(), String>,
    dir: &Path,
) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("Project directory not found: {}", dir.display()));
    }
    launch(dir)
}

/// Open the project's resolved directory in VS Code. Waiting for the
/// launcher's exit is what makes a missing VS Code a visible error
/// ("Unable to find application named ...") instead of a silent no-op.
#[tauri::command]
pub(crate) async fn open_in_vs_code(name: String) -> Result<(), String> {
    let dir = project_dir(&name).ok_or("Project directory not resolved")?;
    tokio::task::spawn_blocking(move || {
        open_in_vs_code_with(
            |dir| {
                let out = vscode_launch(dir)
                    .output()
                    .map_err(|e| format!("Failed to launch VS Code: {e}"))?;
                out.status.success().then_some(()).ok_or_else(|| {
                    format!(
                        "Failed to open in VS Code: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    )
                })
            },
            &dir,
        )
    })
    .await
    .map_err(|e| format!("open task failed: {e}"))?
}

/// Re-read the project's agent instructions (AGENTS.md, falling back to
/// CLAUDE.md) from its resolved directory, e.g. after an /init run, so the
/// new file becomes the project context without restarting the app.
#[tauri::command]
pub(crate) fn refresh_project_context(name: String) -> Result<Option<String>, String> {
    let dir = project_dir(&name).ok_or("project directory not resolved")?;
    Ok(repo_context(&dir))
}

/// Directory for a project placed inside a sidebar group: `root/<group>/<name>`.
/// `group` is a relative path under the root (what `scan_git_repos_in` reports),
/// so `..` and absolute components are rejected to keep it under the root.
fn grouped_dir(root: &Path, group: &str, name: &str) -> Result<PathBuf, String> {
    let rel = group_rel(group).ok_or_else(|| format!("invalid project group: {group:?}"))?;
    Ok(root.join(rel).join(sanitize_name(name)))
}

/// Create (if needed) and return the files directory for a project. With a
/// group, the directory is created under that group's folder instead of the
/// default mapping; the caller stores the returned path as the override.
#[tauri::command]
pub(crate) fn create_project_dir(name: String, group: Option<String>) -> Result<String, String> {
    let dir = match group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        Some(group) => grouped_dir(
            &PathBuf::from(read_config().projects_root),
            group,
            leaf_name(&name, group),
        )?,
        None => project_dir(&name).ok_or("project directory not resolved")?,
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

/// Move a project's directory under `root/<group>/` and return the new path.
/// A `<old group>/` prefix on the name is dropped so the folder keeps its
/// leaf name. A missing source just creates the destination.
#[tauri::command]
pub(crate) fn move_project(name: String, group: String) -> Result<String, String> {
    let from = project_dir(&name).ok_or("project directory not resolved")?;
    let old_group = project_groups().remove(&name).unwrap_or_default();
    let leaf = leaf_name(&name, &old_group);
    let to = grouped_dir(&PathBuf::from(read_config().projects_root), &group, leaf)?;
    if from == to {
        return Ok(to.to_string_lossy().to_string());
    }
    if to.exists() {
        return Err(format!("{} already exists", to.display()));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create group: {e}"))?;
    }
    if from.exists() {
        std::fs::rename(&from, &to).map_err(|e| format!("Failed to move project: {e}"))?;
    } else {
        std::fs::create_dir_all(&to)
            .map_err(|e| format!("Failed to create project directory: {e}"))?;
    }
    Ok(to.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn sanitize_strips_separators_and_reserved_chars() {
        assert_eq!(sanitize_name("My Project"), "My Project");
        assert_eq!(
            sanitize_name("a/b\\c:d*e?f\"g<h>i|j"),
            "a-b-c-d-e-f-g-h-i-j"
        );
        assert_eq!(sanitize_name("  ..-- x  "), "x");
        assert_eq!(sanitize_name(""), "project");
        assert_eq!(sanitize_name("///"), "project");
        assert_eq!(sanitize_name("École"), "École");
    }

    #[test]
    fn projects_state_round_trips_yaml() {
        let home = TempDir::new("projects-yaml");

        assert_eq!(read_projects_in(&home), serde_json::json!({}));

        let state = serde_json::json!({
            "names": ["desktop", "core/cli"],
            "assignments": {"abc-123": "desktop"},
            "groups": {"core/cli": "core"},
            "paths": {"desktop": "/Users/me/code/desktop"},
        });
        write_projects_in(&home, &state).unwrap();

        let text = std::fs::read_to_string(projects_path(&home)).unwrap();
        assert!(!text.starts_with('{'), "wrote JSON, not YAML: {text}");
        assert!(
            text.starts_with("assignments:\n  abc-123: desktop\n"),
            "{text}"
        );
        assert!(text.contains("names:\n- desktop\n- core/cli\n"), "{text}");
        assert_eq!(read_projects_in(&home), state);

        // A hand-edited file is read as written - this is why it is YAML.
        std::fs::write(projects_path(&home), "names:\n  - solo\ngroups: {}\n").unwrap();
        assert_eq!(
            read_projects_in(&home)["names"],
            serde_json::json!(["solo"])
        );
    }

    #[test]
    fn assign_dirs_places_grouped_projects_under_their_group_folder() {
        let root = Path::new("/tmp/projects-root");
        let names: Vec<String> = vec!["core/cli".into(), "cli".into(), "loose".into()];
        let groups = BTreeMap::from([
            ("core/cli".to_string(), "core".to_string()),
            ("cli".to_string(), "../evil".to_string()),
        ]);
        let dirs = assign_dirs(root, &names, &groups);
        assert_eq!(dirs["core/cli"], root.join("core").join("cli"));
        assert_eq!(
            dirs["cli"],
            root.join("cli"),
            "invalid group falls back to root"
        );
        assert_eq!(dirs["loose"], root.join("loose"));
    }

    #[test]
    fn grouped_dir_stays_under_root() {
        let root = Path::new("/tmp/projects-root");
        assert_eq!(
            grouped_dir(root, "videos", "my/clip").unwrap(),
            root.join("videos").join("my-clip")
        );
        assert_eq!(
            grouped_dir(root, "/adks/", "x").unwrap(),
            root.join("adks").join("x")
        );
        assert!(grouped_dir(root, "", "x").is_err());
        assert!(grouped_dir(root, "../etc", "x").is_err());
    }

    #[test]
    fn assign_dirs_suffixes_collisions_deterministically() {
        let root = Path::new("/tmp/projects-root");
        let names: Vec<String> = vec![
            "B".into(),
            "A".into(),
            "A:B".into(),
            "A?".into(),
            "A/B".into(),
        ];
        let dirs = assign_dirs(root, &names, &BTreeMap::new());
        assert_eq!(dirs["A"], root.join("A"));
        assert_eq!(dirs["B"], root.join("B"));
        assert_eq!(dirs["A?"], root.join("A-2"));
        assert_eq!(dirs["A/B"], root.join("A-B"));
        assert_eq!(dirs["A:B"], root.join("A-B-2"));
        let again = assign_dirs(root, &names, &BTreeMap::new());
        assert_eq!(dirs, again);
    }

    #[test]
    fn resolved_dirs_prefers_override_and_keeps_defaults() {
        let root = Path::new("/tmp/projects-root");
        let names: Vec<String> = vec!["A".into(), "B".into()];
        let overrides = BTreeMap::from([(
            "A".to_string(),
            PathBuf::from("/elsewhere/repo with spaces"),
        )]);
        let dirs = resolved_dirs(root, &names, &BTreeMap::new(), &overrides);
        assert_eq!(dirs["A"], PathBuf::from("/elsewhere/repo with spaces"));
        assert_eq!(dirs["B"], root.join("B"));
        let unknown = BTreeMap::from([("Ghost".to_string(), PathBuf::from("/x"))]);
        assert!(!resolved_dirs(root, &names, &BTreeMap::new(), &unknown).contains_key("Ghost"));
    }

    #[test]
    fn default_root_ends_with_app_dir() {
        assert!(default_projects_root(Path::new("/home/x")).ends_with(APP_PROJECTS_DIR));
    }

    #[test]
    fn project_dir_maps_the_sanitized_name_under_the_root() {
        let dir = project_dir("Weird/Name").expect("dir resolves");
        assert!(dir.ends_with("Weird-Name"), "{dir:?}");
    }

    /// The init wiring: the dir check gates the action, and the context
    /// refresh reads AGENTS.md (not CLAUDE.md) from the resolved directory.
    #[test]
    fn init_dir_check_and_context_refresh_read_the_project_dir() {
        let name = "Init Probe";
        let dir = project_dir(name).expect("dir resolves");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!project_dir_exists(name.into()), "missing dir is absent");

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "fresh agents rules").unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "stale claude rules").unwrap();
        assert!(project_dir_exists(name.into()), "present dir exists");
        let ctx = refresh_project_context(name.into()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            ctx.as_deref(),
            Some("fresh agents rules"),
            "AGENTS.md wins over CLAUDE.md"
        );
    }

    /// The open action checks the resolved directory before launching
    /// (injected launcher, never spawns) and surfaces launcher failures.
    #[test]
    fn open_in_vs_code_checks_the_dir_before_launching() {
        let name = "VSCode Probe";
        let dir = project_dir(name).expect("dir resolves");
        let _ = std::fs::remove_dir_all(&dir);

        let mut calls = 0;
        let err = open_in_vs_code_with(
            |_d| {
                calls += 1;
                Ok(())
            },
            &dir,
        )
        .unwrap_err();
        assert_eq!(calls, 0, "a missing directory must not be launched");
        assert!(err.contains("not found"), "{err}");

        std::fs::create_dir_all(&dir).unwrap();
        let mut launched_with = None;
        open_in_vs_code_with(
            |d| {
                launched_with = Some(d.to_path_buf());
                Ok(())
            },
            &dir,
        )
        .unwrap();
        assert_eq!(launched_with.as_deref(), Some(dir.as_path()));
        assert!(
            open_in_vs_code_with(
                |_| Err("Unable to find application named 'Visual Studio Code'".into()),
                &dir,
            )
            .unwrap_err()
            .contains("Unable to find application"),
            "launcher failures surface"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
