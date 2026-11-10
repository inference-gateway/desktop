use super::{project_dir, project_is_content, sanitize_name};
use crate::config::{DesktopConfig, read_config};
use base64::Engine as _;
use std::collections::BTreeSet;
use std::path::Path;
use tauri_plugin_dialog::DialogExt;

/// One file entry of a project's files summary.
#[derive(Clone, serde::Serialize)]
pub(crate) struct ProjectFile {
    pub(crate) name: String,
    pub(crate) size: u64,
}

/// Summary of a project's stored files for the Projects tab: the local files
/// directory, or the project folder in the configured GitHub repository.
#[tauri::command]
pub(crate) async fn list_project_files(project: String) -> Result<Vec<ProjectFile>, String> {
    let cfg = read_config();
    if cfg.projects_backend == "github" {
        tokio::task::spawn_blocking(move || list_github_files(&cfg, &project))
            .await
            .map_err(|e| format!("list task failed: {e}"))?
    } else {
        Ok(list_local_files(
            &project_dir(&project).ok_or("project directory not resolved")?,
        ))
    }
}

pub(crate) fn list_local_files(dir: &Path) -> Vec<ProjectFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<ProjectFile> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then_some(ProjectFile {
                name: e.file_name().to_string_lossy().into_owned(),
                size: meta.len(),
            })
        })
        .collect();
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

fn list_github_files(cfg: &DesktopConfig, project: &str) -> Result<Vec<ProjectFile>, String> {
    let full = github_full_repo(cfg)?;
    let out = crate::scheduler::gh_output(&[
        "api",
        &format!("repos/{full}/contents/{}", sanitize_name(project)),
    ])?;
    let val: serde_json::Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
    Ok(val
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            (f.get("type").and_then(|v| v.as_str()) == Some("file")).then_some(ProjectFile {
                name: f.get("name")?.as_str()?.to_string(),
                size: f.get("size")?.as_u64()?,
            })
        })
        .collect())
}

fn mime_for_ext(ext: &str) -> Option<&'static str> {
    match ext {
        "pdf" => Some("application/pdf"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "m4a" => Some("audio/mp4"),
        "aac" => Some("audio/aac"),
        "txt" => Some("text/plain"),
        "md" => Some("text/markdown"),
        "csv" => Some("text/csv"),
        _ => None,
    }
}

/// Which project subdirectory an uploaded file belongs in: a content project
/// keeps media-typed files (video, audio, image) in its media pool so a
/// timeline can reference them as `media/<file>` without a manual move;
/// documents and every code project stay at the project root, as do files with
/// an unknown extension. Pure so the routing is testable without HOME.
fn media_rel_dir(content: bool, fname: &str) -> Option<&'static str> {
    let ext = Path::new(fname).extension()?.to_str()?.to_ascii_lowercase();
    let mime = mime_for_ext(&ext)?;
    let media =
        mime.starts_with("image/") || mime.starts_with("video/") || mime.starts_with("audio/");
    (content && media).then_some("media")
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

/// Store an attachment with the projects backend (local filesystem or the
/// projects GitHub repository), after enforcing the configured size and
/// extension limits backend-side.
fn dispatch_attachment(
    cfg: &DesktopConfig,
    project: &str,
    fname: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    match cfg.projects_backend.as_str() {
        "github" => save_to_github(cfg, project, fname, bytes),
        _ => save_to_local(project, fname, bytes),
    }
}

/// Validate and store one attachment. With a project it goes through the
/// projects backend (media pool or root per `media_rel_dir`); without one it
/// is written to ~/.infer/tmp/uploads under its own name.
fn finish_attachment(
    project: Option<&str>,
    fname: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let cfg = read_config();
    validate_upload(
        bytes.len(),
        &fname,
        "",
        &cfg.projects_max_file_size_mb,
        &cfg.projects_allowed_mimes,
    )?;
    match project {
        Some(project) => dispatch_attachment(&cfg, project, fname, bytes),
        None => write_to_uploads(fname, &bytes),
    }
}

/// Copy an attached file into ~/.infer/tmp/uploads under its own name; a name
/// already present gets a -2 style suffix, so re-attaching never overwrites
/// the file an earlier message points at.
fn write_to_uploads(fname: String, bytes: &[u8]) -> Result<String, String> {
    let dir = crate::env::uploads_dir();
    let path = Path::new(&fname);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("upload");
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let mut dest = dir.join(&fname);
    let mut n = 2;
    while dest.exists() {
        dest = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    std::fs::write(&dest, bytes).map_err(|e| format!("Failed to save upload: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// The composer's attach button: pick a file with the native dialog (filtered
/// to the configured allowlist) and copy it to its destination. The picked
/// path is real, so no file bytes cross the webview - a multi-hundred-MB video
/// never becomes a base64 string. Returns `None` when the user cancels.
#[tauri::command]
pub(crate) async fn attach_pick(
    app: tauri::AppHandle,
    project: Option<String>,
) -> Result<Option<String>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let cfg = read_config();
        let exts: Vec<&str> = cfg
            .projects_allowed_mimes
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        app.dialog()
            .file()
            .add_filter("Allowed", &exts)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("file dialog failed: {e}"))?;
    let Some(src) = picked.and_then(|fp| fp.into_path().ok()) else {
        return Ok(None);
    };
    let saved = tokio::task::spawn_blocking(move || {
        let fname = src
            .file_name()
            .and_then(|f| f.to_str())
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .ok_or_else(|| format!("invalid file name: {}", src.display()))?
            .to_string();
        let bytes = std::fs::read(&src).map_err(|e| e.to_string())?;
        finish_attachment(project.as_deref(), fname, bytes)
    })
    .await
    .map_err(|e| format!("attach task failed: {e}"))??;
    Ok(Some(saved))
}

/// A file dropped or pasted onto the composer: raw bytes in the request body,
/// with the project and file name in hex-encoded headers (header values are
/// ASCII only). ponytail: the whole file streams through the webview because
/// with Tauri's native drag-drop off a drop yields a File object, not a path;
/// this is the non-base64 route, and the + button's native picker
/// (attach_pick) avoids the round trip entirely.
#[tauri::command]
pub(crate) fn attach_bytes(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let header = |key: &str| -> Result<String, String> {
        request
            .headers()
            .get(key)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| format!("missing {key} header"))
    };
    let project = match header("x-project")? {
        hex if hex.is_empty() => None,
        hex => Some(crate::timeline::hex_decode(&hex)?),
    };
    let fname = crate::timeline::hex_decode(&header("x-name")?)?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw file bytes".into());
    };
    finish_attachment(project.as_deref(), fname, bytes.to_vec())
}

fn save_to_local(project: &str, fname: String, bytes: Vec<u8>) -> Result<String, String> {
    let dir = project_dir(project).ok_or("project directory not resolved")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;
    let dest = match media_rel_dir(project_is_content(project), &fname) {
        Some(rel) => {
            std::fs::create_dir_all(dir.join(rel))
                .map_err(|e| format!("Failed to create {rel} directory: {e}"))?;
            dir.join(rel).join(&fname)
        }
        None => dir.join(&fname),
    };
    std::fs::write(&dest, bytes).map_err(|e| format!("Failed to save file: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Run `gh` with `input` piped over stdin (large bodies exceed argv limits).
fn gh_stdin(args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = crate::env::command(crate::download::gh_bin())
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

/// Resolved `owner/name` of the configured projects repository.
fn github_full_repo(cfg: &DesktopConfig) -> Result<String, String> {
    let name = cfg.projects_github_repository.trim().trim_matches('/');
    if name.is_empty() {
        return Err("No GitHub repository configured for projects".into());
    }
    if name.contains('/') {
        return Ok(name.to_string());
    }
    let owner = crate::scheduler::gh_output(&["api", "user", "--jq", ".login"])
        .map_err(|e| format!("Cannot resolve GitHub owner: {e}"))?;
    Ok(format!("{}/{}", owner.trim(), name))
}

fn save_to_github(
    cfg: &DesktopConfig,
    project: &str,
    fname: String,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let full = github_full_repo(cfg)?;
    ensure_github_repo(&full)?;
    let path = match media_rel_dir(project_is_content(project), &fname) {
        Some(rel) => format!("{}/{rel}/{fname}", sanitize_name(project)),
        None => format!("{}/{fname}", sanitize_name(project)),
    };
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
    use crate::test_support::TempDir;

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
    fn list_local_files_lists_regular_files_sorted_with_sizes() {
        let dir = TempDir::new("list");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.txt"), "hey").unwrap();
        std::fs::write(dir.join("a.pdf"), [0u8; 5]).unwrap();
        let files = list_local_files(&dir);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a.pdf", "b.txt"], "dirs skipped, sorted by name");
        assert_eq!(files[0].size, 5);
        assert_eq!(files[1].size, 3);
    }

    #[test]
    fn every_default_allowlisted_extension_maps_to_a_mime() {
        for ext in
            "pdf,png,jpg,jpeg,heic,heif,gif,webp,mp4,mov,webm,mp3,wav,m4a,aac,txt,md,csv".split(',')
        {
            assert!(mime_for_ext(ext).is_some(), "{ext}");
        }
    }
}
