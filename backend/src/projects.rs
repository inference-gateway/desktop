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

/// Per-project directory overrides from the `paths` object in projects.json:
/// trimmed, `~`-expanded, and only absolute paths (a relative override would
/// grant a meaningless relative sandbox entry).
fn project_paths() -> BTreeMap<String, PathBuf> {
    let path = crate::env::home_dir().join(".infer").join("projects.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
        return BTreeMap::new();
    };
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

/// Default name->dir mapping with per-project overrides applied on top.
/// ponytail: two projects may point at the same dir; harmless (duplicate
/// grant, shared files) - validate only if users hit it.
fn resolved_dirs(
    root: &Path,
    names: &[String],
    overrides: &BTreeMap<String, PathBuf>,
) -> BTreeMap<String, PathBuf> {
    let mut dirs = assign_dirs(root, names);
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
    let dirs = resolved_dirs(&root, &names, &project_paths());
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
pub(crate) fn project_dir(name: &str) -> Option<PathBuf> {
    let mut names = project_names();
    if !names.iter().any(|n| n == name) {
        names.push(name.to_string());
    }
    let root = PathBuf::from(read_config().projects_root);
    resolved_dirs(&root, &names, &project_paths())
        .get(name)
        .cloned()
}

/// A git repository found under the projects root, with its agent
/// instructions (AGENTS.md, falling back to CLAUDE.md) when present.
#[derive(Clone, PartialEq, Debug, serde::Serialize)]
pub(crate) struct GitRepo {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) group: String,
    pub(crate) context: Option<String>,
}

/// Agent instructions from the repo root: AGENTS.md, else CLAUDE.md.
/// ponytail: 64 KB cap so a runaway file cannot bloat projects.json.
fn repo_context(dir: &Path) -> Option<String> {
    ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
        .find(|text| !text.trim().is_empty() && text.len() <= 64 * 1024)
}

/// Recursively find git repositories under `root` (a dir with `.git` — file or
/// dir, so worktrees count). Found repos are not descended into. `group` is
/// the repo's parent directory relative to `root` ("" for direct children),
/// used to title clusters of sibling repos in the UI and agent context.
/// ponytail: depth cap 4 and a two-entry junk skip-list; make configurable if
/// users have deeper trees.
fn scan_git_repos_in(root: &Path) -> Vec<GitRepo> {
    const MAX_DEPTH: usize = 4;
    let mut repos = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_dir()
                || name.starts_with('.')
                || matches!(name.as_str(), "node_modules" | "target")
            {
                continue;
            }
            if path.join(".git").exists() {
                let group = path
                    .parent()
                    .and_then(|p| p.strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                repos.push(GitRepo {
                    name,
                    group,
                    context: repo_context(&path),
                    path: path.to_string_lossy().into_owned(),
                });
            } else if depth + 1 < MAX_DEPTH {
                stack.push((path, depth + 1));
            }
        }
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    repos
}

/// Clone destination for a validated `owner/name` repo under the projects
/// root: the sanitized repo name. Sanitizing (dots trimmed, `..` falls back
/// to "project") guarantees the destination stays under the root.
fn clone_dest(root: &Path, repo: &str) -> PathBuf {
    let name = repo.split_once('/').map_or("", |(_, n)| n);
    root.join(sanitize_name(name))
}

/// The importable repo entry for a checkout on disk: name from the directory,
/// flat (no group), context from AGENTS.md/CLAUDE.md.
fn cloned_repo(dest: &Path) -> GitRepo {
    GitRepo {
        name: dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: dest.to_string_lossy().into_owned(),
        group: String::new(),
        context: repo_context(dest),
    }
}

/// Whether the checkout at `dest` is `owner/name`, by looking for its remote in
/// `.git/config`. The `/` or `:` boundary keeps "acme/api" from matching
/// "acme/api-client" or "notacme/api"; the comparison is case-insensitive
/// because GitHub owner and repo names are. A `.git` file (worktree, submodule)
/// has no config to read and reads as "not this repo", which fails safe.
fn checkout_is(dest: &Path, repo: &str) -> bool {
    let want = repo.to_lowercase();
    std::fs::read_to_string(dest.join(".git").join("config")).is_ok_and(|config| {
        config.lines().any(|line| {
            let url = line.trim().to_lowercase();
            let url = url.strip_suffix(".git").unwrap_or(&url);
            url.strip_suffix(&want)
                .is_some_and(|prefix| prefix.ends_with(['/', ':']))
        })
    })
}

/// Clone via `clone()` unless `dest` already holds a checkout of the same repo
/// (idempotent re-import), then return the entry for the existing import flow.
/// A checkout of a *different* repo is an error rather than a silent adoption -
/// the destination drops the owner, so two owners' "desktop" collide, as does
/// any repo the user cloned there by hand. A directory this call created is
/// removed when the clone fails, so a half-finished clone cannot wedge the
/// destination; one that already existed is left alone. `clone` is injected so
/// both paths are testable without network.
fn ensure_clone(
    clone: impl FnOnce() -> Result<(), String>,
    dest: &Path,
    repo: &str,
) -> Result<GitRepo, String> {
    if dest.join(".git").exists() {
        if !checkout_is(dest, repo) {
            return Err(format!(
                "{} already holds a different repository - remove or rename it first",
                dest.display()
            ));
        }
        return Ok(cloned_repo(dest));
    }
    let existed = dest.exists();
    std::fs::create_dir_all(dest.parent().unwrap_or(dest))
        .map_err(|e| format!("Failed to create projects root: {e}"))?;
    clone().inspect_err(|_| {
        if !existed {
            let _ = std::fs::remove_dir_all(dest);
        }
    })?;
    Ok(cloned_repo(dest))
}

/// Clone a GitHub repository under the projects root and return it ready for
/// importProjects. `gh repo clone` handles auth and protocol; on failure its
/// stderr is the error.
#[tauri::command]
pub(crate) async fn clone_github_repo(repo: String) -> Result<GitRepo, String> {
    if !crate::scheduler::valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    let dest = clone_dest(
        &PathBuf::from(expand_home(&read_config().projects_root)),
        &repo,
    );
    let target = dest.to_string_lossy().into_owned();
    tokio::task::spawn_blocking(move || {
        ensure_clone(
            || crate::scheduler::gh_output(&["repo", "clone", &repo, &target]).map(|_| ()),
            &dest,
            &repo,
        )
    })
    .await
    .map_err(|e| format!("clone task failed: {e}"))?
}

/// Scan a root directory for importable git repositories.
#[tauri::command]
pub(crate) async fn scan_git_repos(root: String) -> Result<Vec<GitRepo>, String> {
    let raw = root.trim();
    if raw.is_empty() {
        return Err("No root directory given".into());
    }
    let path = PathBuf::from(expand_home(raw));
    if !path.is_dir() {
        return Err(format!("Not a directory: {}", path.display()));
    }
    tokio::task::spawn_blocking(move || scan_git_repos_in(&path))
        .await
        .map_err(|e| format!("scan task failed: {e}"))
}

/// Names of projects whose resolved directory is a git repository; powers the
/// git indicator in the UI. Derived live from the filesystem, nothing stored.
#[tauri::command]
pub(crate) fn git_project_names() -> Vec<String> {
    let names = project_names();
    let root = PathBuf::from(read_config().projects_root);
    resolved_dirs(&root, &names, &project_paths())
        .into_iter()
        .filter(|(_, dir)| dir.join(".git").exists())
        .map(|(name, _)| name)
        .collect()
}

/// Whether the project's resolved directory exists on disk; gates the Init
/// action so a stale import fails visibly instead of cryptically.
#[tauri::command]
pub(crate) fn project_dir_exists(name: String) -> bool {
    project_dir(&name).is_some_and(|dir| dir.is_dir())
}

/// Platform command that opens a folder in VS Code: `open -a` on macOS, the
/// `code` CLI elsewhere. Both exit promptly after handing the folder over.
fn vscode_launch(dir: &Path) -> std::process::Command {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(not(target_os = "macos"))]
    let mut cmd = std::process::Command::new("code");
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

/// Create (if needed) and return the files directory for a project.
#[tauri::command]
pub(crate) fn create_project_dir(name: String) -> Result<String, String> {
    let dir = project_dir(&name).ok_or("project directory not resolved")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create project directory: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

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

fn list_local_files(dir: &Path) -> Vec<ProjectFile> {
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
        _ => save_to_local(&project, fname, bytes),
    })
    .await
    .map_err(|e| format!("upload task failed: {e}"))?
}

fn save_to_local(project: &str, fname: String, bytes: Vec<u8>) -> Result<String, String> {
    let dir = project_dir(project).ok_or("project directory not resolved")?;
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
    fn resolved_dirs_prefers_override_and_keeps_defaults() {
        let root = Path::new("/tmp/projects-root");
        let names: Vec<String> = vec!["A".into(), "B".into()];
        let overrides = BTreeMap::from([(
            "A".to_string(),
            PathBuf::from("/elsewhere/repo with spaces"),
        )]);
        let dirs = resolved_dirs(root, &names, &overrides);
        assert_eq!(dirs["A"], PathBuf::from("/elsewhere/repo with spaces"));
        assert_eq!(dirs["B"], root.join("B"));
        let unknown = BTreeMap::from([("Ghost".to_string(), PathBuf::from("/x"))]);
        assert!(!resolved_dirs(root, &names, &unknown).contains_key("Ghost"));
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

    #[test]
    fn list_local_files_lists_regular_files_sorted_with_sizes() {
        let dir = std::env::temp_dir().join(format!("igd-list-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.txt"), "hey").unwrap();
        std::fs::write(dir.join("a.pdf"), [0u8; 5]).unwrap();
        let files = list_local_files(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["a.pdf", "b.txt"], "dirs skipped, sorted by name");
        assert_eq!(files[0].size, 5);
        assert_eq!(files[1].size, 3);
    }

    #[test]
    fn scan_finds_nested_repos_and_skips_junk() {
        let root = std::env::temp_dir().join(format!("igd-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("repo-a/.git")).unwrap();
        std::fs::write(root.join("repo-a/AGENTS.md"), "agents rules").unwrap();
        std::fs::write(root.join("repo-a/CLAUDE.md"), "claude rules").unwrap();
        std::fs::create_dir_all(root.join("group/repo-b/.git")).unwrap();
        std::fs::write(root.join("group/repo-b/CLAUDE.md"), "claude rules").unwrap();
        std::fs::create_dir_all(root.join("worktree")).unwrap();
        std::fs::write(root.join("worktree/.git"), "gitdir: /elsewhere").unwrap();
        std::fs::create_dir_all(root.join("node_modules/dep/.git")).unwrap();
        std::fs::create_dir_all(root.join(".hidden/repo-c/.git")).unwrap();
        std::fs::create_dir_all(root.join("repo-a/vendored/.git")).unwrap();
        std::fs::create_dir_all(root.join("d1/d2/d3/d4/too-deep/.git")).unwrap();
        std::fs::create_dir_all(root.join("plain")).unwrap();
        let repos = scan_git_repos_in(&root);
        let _ = std::fs::remove_dir_all(&root);
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["repo-a", "repo-b", "worktree"], "{repos:?}");
        assert!(repos[1].path.ends_with("group/repo-b"), "{repos:?}");
        assert_eq!(repos[0].context.as_deref(), Some("agents rules"));
        assert_eq!(repos[1].context.as_deref(), Some("claude rules"));
        assert_eq!(repos[2].context, None);
        assert_eq!(repos[0].group, "");
        assert_eq!(repos[1].group, "group");
        assert_eq!(repos[2].group, "");
    }

    #[test]
    fn every_default_allowlisted_extension_maps_to_a_mime() {
        for ext in "pdf,png,jpg,jpeg,gif,webp,mp4,mov,txt,md,csv".split(',') {
            assert!(mime_for_ext(ext).is_some(), "{ext}");
        }
    }

    #[test]
    fn clone_dest_keeps_the_repo_name_under_the_root() {
        let root = Path::new("/tmp/projects-root");
        assert_eq!(clone_dest(root, "owner/my-repo"), root.join("my-repo"));
        assert_eq!(
            clone_dest(root, "inference-gateway/desktop"),
            root.join("desktop")
        );
        assert_eq!(clone_dest(root, "owner/.."), root.join("project"));
    }

    /// A checkout of `repo` at `dest`, with the remote `.git/config` git writes.
    fn fake_checkout(dest: &Path, repo: &str) {
        std::fs::create_dir_all(dest.join(".git")).unwrap();
        std::fs::write(
            dest.join(".git").join("config"),
            format!("[remote \"origin\"]\n\turl = https://github.com/{repo}.git\n"),
        )
        .unwrap();
    }

    #[test]
    fn ensure_clone_skips_an_existing_checkout() {
        let root = std::env::temp_dir().join(format!("igd-clone-skip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = clone_dest(&root, "owner/repo-a");
        fake_checkout(&dest, "owner/repo-a");
        std::fs::write(dest.join("AGENTS.md"), "agents rules").unwrap();
        let mut cloned = false;
        let repo = ensure_clone(
            || {
                cloned = true;
                Ok(())
            },
            &dest,
            "owner/repo-a",
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(!cloned, "existing checkout must not be re-cloned");
        assert_eq!(repo.name, "repo-a");
        assert_eq!(repo.path, dest.to_string_lossy());
        assert_eq!(repo.group, "");
        assert_eq!(repo.context.as_deref(), Some("agents rules"));
    }

    #[test]
    fn ensure_clone_refuses_a_checkout_of_a_different_repo() {
        let root = std::env::temp_dir().join(format!("igd-clone-other-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = clone_dest(&root, "owner/desktop");

        for squatter in ["someone-else/desktop", "owner/desktop-legacy"] {
            fake_checkout(&dest, squatter);
            let mut cloned = false;
            let err = ensure_clone(
                || {
                    cloned = true;
                    Ok(())
                },
                &dest,
                "owner/desktop",
            )
            .unwrap_err();
            assert!(!cloned, "{squatter} must not be adopted or overwritten");
            assert!(err.contains("different repository"), "{err}");
        }

        std::fs::write(
            dest.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:owner/desktop.git\n",
        )
        .unwrap();
        assert!(
            ensure_clone(|| Ok(()), &dest, "owner/desktop").is_ok(),
            "the ssh remote form is the same repo"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_clone_clones_missing_and_surfaces_failures() {
        let root = std::env::temp_dir().join(format!("igd-clone-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = clone_dest(&root, "owner/repo-b");
        let err = ensure_clone(
            || {
                std::fs::create_dir_all(dest.join("half-written")).unwrap();
                Err("gh blew up".into())
            },
            &dest,
            "owner/repo-b",
        )
        .unwrap_err();
        assert_eq!(err, "gh blew up");
        assert!(
            !dest.exists(),
            "a partial clone must not wedge the destination"
        );
        let repo = ensure_clone(
            || {
                fake_checkout(&dest, "owner/repo-b");
                Ok(())
            },
            &dest,
            "owner/repo-b",
        )
        .unwrap();
        assert!(dest.join(".git").exists());
        assert_eq!(repo.name, "repo-b");
        assert_eq!(repo.context, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_clone_keeps_a_directory_it_did_not_create() {
        let root = std::env::temp_dir().join(format!("igd-clone-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = clone_dest(&root, "owner/repo-c");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("notes.txt"), "the user's own file").unwrap();
        assert!(ensure_clone(|| Err("clone refused".into()), &dest, "owner/repo-c").is_err());
        assert!(
            dest.join("notes.txt").exists(),
            "a pre-existing directory must never be removed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
