use crate::config::{DesktopConfig, read_config};
use base64::Engine as _;
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

/// Deterministic per-project directory mapping: names are processed in sorted
/// order, sanitized, and a numeric suffix is appended on collision ("a/b" and
/// "a:b" both sanitize to "a-b"; the second sorted name gets "a-b-2"). Pure
/// function of (root, names) so grants can be re-derived from projects.json
/// alone and repeated calls are idempotent.
fn assign_dirs(root: &Path, names: &[String]) -> BTreeMap<String, PathBuf> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut map = BTreeMap::new();
    for name in names.iter().collect::<BTreeSet<_>>() {
        let base = sanitize_name(name);
        let mut dir = base.clone();
        let mut n = 2;
        while !taken.insert(dir.clone()) {
            dir = format!("{base}-{n}");
            n += 1;
        }
        map.insert(name.to_string(), root.join(dir));
    }
    map
}

/// Project names known to the app: the explicit `names` list plus every value
/// in `assignments`, from ~/.infer/projects.json (same sources the sidebar uses).
fn project_names() -> Vec<String> {
    let path = crate::env::home_dir().join(".infer").join("projects.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
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
    let dirs = assign_dirs(&root, &names);
    let mut value = String::from(CLI_DEFAULT_SANDBOX_DIRS);
    for dir in dirs.values() {
        value.push(',');
        value.push_str(&dir.to_string_lossy());
    }
    Some(value)
}

/// Create (if needed) and return the files directory for a project.
#[tauri::command]
pub(crate) fn create_project_dir(name: String) -> Result<String, String> {
    let mut names = project_names();
    if !names.iter().any(|n| n == &name) {
        names.push(name.clone());
    }
    let root = PathBuf::from(read_config().projects_root);
    let dir = assign_dirs(&root, &names)
        .get(&name)
        .ok_or("project directory not resolved")?
        .clone();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

fn mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "txt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "csv" => Some("text/csv"),
        _ => None,
    }
}

/// Check a decoded upload against the configured max size and extension/MIME
/// allowlist; errors name the violated limit. Split from `save_project_file`
/// so the guards are testable.
fn validate_upload(
    bytes_len: usize,
    filename: &str,
    mime: &str,
    max_mb: &str,
    allowed: &str,
) -> Result<(), String> {
    let max_bytes: usize = max_mb.trim().parse::<usize>().unwrap_or(10) * 1024 * 1024;
    if bytes_len > max_bytes {
        return Err(format!(
            "File too large: {:.1} MB exceeds the {} MB max size",
            bytes_len as f64 / (1024.0 * 1024.0),
            max_bytes / (1024 * 1024)
        ));
    }
    let allowed: BTreeSet<&str> = allowed
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let base_mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let ok = if base_mime.is_empty() {
        Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| allowed.contains(e.to_ascii_lowercase().as_str()))
    } else {
        allowed
            .iter()
            .any(|ext| mime_for_ext(ext) == Some(base_mime.as_str()))
    };
    if !ok {
        let list = allowed.iter().copied().collect::<Vec<_>>().join(", ");
        return Err(format!(
            "File type {} is not allowed (allowed types: {list})",
            if base_mime.is_empty() {
                filename.to_string()
            } else {
                base_mime
            }
        ));
    }
    Ok(())
}

/// Store an uploaded file in the project's files directory (local backend) or
/// the projects GitHub repository (github backend), after enforcing the
/// configured size and MIME limits backend-side.
#[tauri::command]
pub(crate) async fn save_project_file(
    project: String,
    filename: String,
    mime: String,
    data: String,
) -> Result<String, String> {
    let cfg = read_config();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid upload data: {e}"))?;
    validate_upload(
        bytes.len(),
        &filename,
        &mime,
        &cfg.projects_max_file_size_mb,
        &cfg.projects_allowed_mimes,
    )?;
    let fname = Path::new(&filename)
        .file_name()
        .and_then(|f| f.to_str())
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .ok_or("Invalid filename")?
        .to_string();

    tokio::task::spawn_blocking(move || match cfg.projects_backend.as_str() {
        "github" => save_to_github(&cfg, &project, fname, bytes),
        _ => save_to_local(&cfg, &project, fname, bytes),
    })
    .await
    .map_err(|e| format!("upload task failed: {e}"))?
}

fn save_to_local(
    cfg: &DesktopConfig,
    project: &str,
    fname: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let mut names = project_names();
    if !names.iter().any(|n| n == project) {
        names.push(project.to_string());
    }
    let root = PathBuf::from(&cfg.projects_root);
    let dir = assign_dirs(&root, &names)
        .get(project)
        .ok_or("project directory not resolved")?
        .clone();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;
    let dest = dir.join(&fname);
    std::fs::write(&dest, bytes).map_err(|e| format!("Failed to save file: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Run `gh` with `input` piped over stdin (large bodies exceed argv limits).
fn gh_stdin(args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = std::process::Command::new(crate::download::gh_bin())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("gh failed to start: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("gh stdin unavailable")?
        .write_all(input.as_bytes())
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn ensure_github_repo(full: &str) -> Result<(), String> {
    if !crate::scheduler::valid_repo(full) {
        return Err(format!("invalid repository: {full}"));
    }
    if crate::scheduler::gh_output(&["repo", "view", full, "--json", "name"]).is_ok() {
        return Ok(());
    }
    crate::scheduler::gh_output(&["repo", "create", full, "--private", "--add-readme"]).map(|_| ())
}

fn save_to_github(
    cfg: &DesktopConfig,
    project: &str,
    fname: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let name = cfg.projects_github_repository.trim().trim_matches('/');
    if name.is_empty() {
        return Err("No GitHub repository configured for projects".into());
    }
    let full = if name.contains('/') {
        name.to_string()
    } else {
        let owner = crate::scheduler::gh_output(&["api", "user", "--jq", ".login"])
            .map_err(|e| format!("Cannot resolve GitHub owner: {e}"))?;
        format!("{}/{}", owner.trim(), name)
    };
    ensure_github_repo(&full)?;
    let path = format!("{}/{fname}", sanitize_name(project));
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let body = serde_json::json!({ "message": format!("Add {path}"), "content": b64 }).to_string();
    gh_stdin(
        &[
            "api",
            &format!("repos/{full}/contents/{path}"),
            "-X",
            "PUT",
            "--input",
            "-",
        ],
        &body,
    )?;
    Ok(format!("{full}/{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn assign_dirs_suffixes_collisions_deterministically() {
        let root = Path::new("/tmp/projects-root");
        let names: Vec<String> = vec![
            "B".into(),
            "A".into(),
            "A:B".into(),
            "A?".into(),
            "A/B".into(),
        ];
        let dirs = assign_dirs(root, &names);
        assert_eq!(dirs["A"], root.join("A"));
        assert_eq!(dirs["B"], root.join("B"));
        assert_eq!(dirs["A?"], root.join("A-2"));
        assert_eq!(dirs["A/B"], root.join("A-B"));
        assert_eq!(dirs["A:B"], root.join("A-B-2"));
        let again = assign_dirs(root, &names);
        assert_eq!(dirs, again);
    }

    #[test]
    fn default_root_ends_with_app_dir() {
        assert!(default_projects_root(Path::new("/home/x")).ends_with(APP_PROJECTS_DIR));
    }

    #[test]
    fn validate_rejects_oversize_naming_the_limit() {
        let err =
            validate_upload(11 * 1024 * 1024, "a.pdf", "application/pdf", "10", "pdf").unwrap_err();
        assert!(err.contains("exceeds the 10 MB max size"), "{err}");
        validate_upload(10 * 1024 * 1024, "a.pdf", "application/pdf", "10", "pdf").unwrap();
    }

    #[test]
    fn validate_rejects_disallowed_mime_naming_the_allowlist() {
        let err = validate_upload(1, "a.rtf", "application/rtf", "10", "pdf,txt").unwrap_err();
        assert!(err.contains("application/rtf is not allowed"), "{err}");
        assert!(err.contains("allowed types: pdf, txt"), "{err}");
        validate_upload(1, "a.pdf", "application/pdf", "10", "pdf,txt").unwrap();
        validate_upload(1, "a.png", "image/PNG", "10", "pdf,png").unwrap();
    }

    #[test]
    fn validate_falls_back_to_extension_when_mime_missing() {
        validate_upload(1, "notes.md", "", "10", "pdf,md").unwrap();
        let err = validate_upload(1, "run.sh", "", "10", "pdf,md").unwrap_err();
        assert!(err.contains("run.sh is not allowed"), "{err}");
    }

    #[test]
    fn every_default_allowlisted_extension_maps_to_a_mime() {
        for ext in "pdf,png,jpg,jpeg,gif,webp,mp4,mov,txt,md,csv".split(',') {
            assert!(mime_for_ext(ext).is_some(), "{ext}");
        }
    }
}
