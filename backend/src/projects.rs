use crate::config::{DesktopConfig, read_config};
use base64::Engine as _;
use notify::{RecursiveMode, Watcher};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tauri::Emitter;

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
/// re-derived from projects.json alone and repeated calls are idempotent.
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

/// Raw ~/.infer/projects.json (the sidebar's persisted state); Null when
/// missing or unparseable.
fn projects_json() -> serde_json::Value {
    let path = crate::env::home_dir().join(".infer").join("projects.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Project names known to the app: the explicit `names` list plus every value
/// in `assignments`, from ~/.infer/projects.json (same sources the sidebar uses).
fn project_names() -> Vec<String> {
    let val = projects_json();
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
    let val = projects_json();
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

/// Per-project sidebar group from the `groups` object in projects.json.
fn project_groups() -> BTreeMap<String, String> {
    let val = projects_json();
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

/// Files directory of the project a chat is assigned to in projects.json.
pub(crate) fn assigned_dir(session_id: &str) -> Option<PathBuf> {
    let name = projects_json()["assignments"][session_id]
        .as_str()?
        .to_owned();
    project_dir(&name)
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
pub(crate) fn repo_context(dir: &Path) -> Option<String> {
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

/// The `owner/name` of a URL that points at github.com (https or ssh forms),
/// validated so callers can trust it as a clone target; None for any other host.
fn parse_github_owner_name(url: &str) -> Option<String> {
    let url = url.trim();
    let url = url.strip_suffix(".git").unwrap_or(url);
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    crate::scheduler::valid_repo(rest).then(|| rest.to_string())
}

/// The GitHub `owner/name` of the checkout's remote, read from `.git/config` (the
/// first `url =` that resolves to github.com). None for a non-repo, a non-GitHub
/// or missing remote, or a `.git` file (worktree/submodule) whose config lives
/// elsewhere - all of which keep the project's context embedded in the export.
pub(crate) fn git_remote_repo(dir: &Path) -> Option<String> {
    let config = std::fs::read_to_string(dir.join(".git").join("config")).ok()?;
    config.lines().find_map(|line| {
        let url = line.trim().strip_prefix("url")?.trim_start();
        parse_github_owner_name(url.strip_prefix('=')?.trim())
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

/// Clone `owner/name` under `root` (idempotent via `ensure_clone`) and return the
/// import entry. Blocking `gh repo clone` (handles auth and protocol); on failure
/// its stderr is the error. Shared by the clone command and the desktop import.
pub(crate) fn clone_repo_under(root: &Path, repo: &str) -> Result<GitRepo, String> {
    let dest = clone_dest(root, repo);
    let target = dest.to_string_lossy().into_owned();
    let repo_arg = repo.to_string();
    ensure_clone(
        || crate::scheduler::gh_output(&["repo", "clone", &repo_arg, &target]).map(|_| ()),
        &dest,
        repo,
    )
}

/// Clone a GitHub repository under the projects root and return it ready for
/// importProjects.
#[tauri::command]
pub(crate) async fn clone_github_repo(repo: String) -> Result<GitRepo, String> {
    if !crate::scheduler::valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    let root = PathBuf::from(expand_home(&read_config().projects_root));
    tokio::task::spawn_blocking(move || clone_repo_under(&root, &repo))
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

/// Which projects are git checkouts and which of those have uncommitted
/// changes (staged, modified or untracked non-ignored files - anything
/// `git status --porcelain` reports); powers the git indicator in the UI.
/// Derived live from the filesystem, nothing stored.
#[derive(Default, serde::Serialize)]
pub(crate) struct GitProjectStatus {
    pub(crate) git: Vec<String>,
    pub(crate) dirty: Vec<String>,
    pub(crate) branches: BTreeMap<String, String>,
    pub(crate) default_branches: BTreeMap<String, String>,
}

/// The actual git directory for a checkout: `.git` itself, or the directory
/// named by a `gitdir:` gitfile (worktrees, submodules).
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dot_git = dir.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let contents = std::fs::read_to_string(&dot_git).ok()?;
    let target = contents.strip_prefix("gitdir:")?.trim();
    let path = Path::new(target);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    })
}

/// Branch name from the contents of `.git/HEAD`: `ref: refs/heads/<branch>`
/// on a branch, a bare commit hash (shortened to 7 chars) when detached.
fn parse_head(contents: &str) -> Option<String> {
    let line = contents.trim();
    if line.is_empty() {
        return None;
    }
    match line.strip_prefix("ref: ") {
        Some(r) => Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
        None => Some(line.chars().take(7).collect()),
    }
}

fn read_head_branch(dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(dir)?.join("HEAD")).ok()?;
    parse_head(&head)
}

/// Default branch from `refs/remotes/origin/HEAD` (`ref: refs/remotes/origin/<name>`),
/// falling back to `main` when origin/HEAD was never recorded.
fn read_default_branch(dir: &Path) -> String {
    git_dir(dir)
        .and_then(|g| std::fs::read_to_string(g.join("refs/remotes/origin/HEAD")).ok())
        .and_then(|c| {
            c.trim()
                .strip_prefix("ref: refs/remotes/origin/")
                .map(str::to_string)
        })
        .unwrap_or_else(|| "main".to_string())
}

/// Whether the checkout at `dir` has uncommitted changes: `git status
/// --porcelain` prints one line per change and we stop reading at the first,
/// so a large listing is never transferred. A missing `git` binary or an
/// unreadable repo reads as clean - the indicator gracefully stays gray.
/// ponytail: git still enumerates internally before printing; switch to a
/// libgit2 backend only if status on giant worktrees is ever slow in practice.
fn has_uncommitted_changes(dir: &Path) -> bool {
    use std::io::Read;
    let Ok(mut child) = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    // One byte of output means `git status --porcelain` found at least one
    // change; zero bytes means the worktree is clean.
    let dirty = child
        .stdout
        .take()
        .is_some_and(|mut out| out.read(&mut [0u8; 1]).is_ok_and(|n| n > 0));
    let _ = child.kill();
    let _ = child.wait();
    dirty
}

/// One live watcher over every git project's git dir, rebuilt on each status
/// sweep so it always tracks the current project set.
pub(crate) struct GitWatcher(pub(crate) std::sync::Mutex<Option<notify::RecommendedWatcher>>);

/// Whether a filesystem event is about a `HEAD` file. Only `HEAD` is watched
/// because `git status` (run by the sweep) may rewrite `index`, and reacting
/// to that would loop sweep -> index write -> event -> sweep.
fn touches_head(event: &notify::Event) -> bool {
    event
        .paths
        .iter()
        .any(|p| p.file_name().is_some_and(|f| f == "HEAD"))
}

/// Watch each git dir non-recursively and call `on_head_change` whenever a
/// `HEAD` file in one of them changes. Dirs that cannot be watched are skipped.
/// ponytail: the worktree itself is not watched, so the dirty indicator only
/// refreshes on HEAD changes (checkout, commit, reset) - a recursive watch of
/// every project tree is the upgrade path if live dirty state is ever wanted.
fn watch_git_dirs(
    dirs: &[PathBuf],
    on_head_change: impl Fn() + Send + 'static,
) -> notify::Result<notify::RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.as_ref().is_ok_and(touches_head) {
            on_head_change();
        }
    })?;
    for dir in dirs {
        let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
    }
    Ok(watcher)
}

/// ponytail: every HEAD change re-sweeps all projects; emit the project name
/// and add a single-project command if this is ever slow with many repos.
#[tauri::command]
pub(crate) async fn git_project_status(
    app: tauri::AppHandle,
    state: tauri::State<'_, GitWatcher>,
) -> Result<GitProjectStatus, String> {
    let (status, git_dirs) = tokio::task::spawn_blocking(move || {
        let names = project_names();
        let root = PathBuf::from(read_config().projects_root);
        let mut status = GitProjectStatus::default();
        let mut git_dirs = Vec::new();
        for (name, dir) in resolved_dirs(&root, &names, &project_groups(), &project_paths()) {
            let Some(git_dir) = git_dir(&dir) else {
                continue;
            };
            git_dirs.push(git_dir);
            status.git.push(name.clone());
            if has_uncommitted_changes(&dir) {
                status.dirty.push(name.clone());
            }
            if let Some(branch) = read_head_branch(&dir) {
                status.branches.insert(name.clone(), branch);
                status
                    .default_branches
                    .insert(name, read_default_branch(&dir));
            }
        }
        (status, git_dirs)
    })
    .await
    .map_err(|e| format!("git status task failed: {e}"))?;
    let watcher = watch_git_dirs(&git_dirs, move || {
        let _ = app.emit("git-changed", ());
    })
    .map_err(|e| format!("watching git dirs: {e}"))?;
    *state.0.lock().map_err(|e| e.to_string())? = Some(watcher);
    Ok(status)
}

/// Run `git -C <dir> <args>`, returning stdout on success and stderr (or the
/// spawn error) on failure.
fn git_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The branch `origin/HEAD` points at, falling back to `main`.
fn origin_default_branch(dir: &Path) -> String {
    git_output(
        dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|r| r.strip_prefix("origin/").map(str::to_string))
    .unwrap_or_else(|| "main".to_string())
}

/// Check out the repository's default branch and fast-forward pull. Refuses
/// over uncommitted changes; `--ff-only` guarantees the tree is never left
/// worse than before. Returns the branch now checked out.
#[tauri::command]
pub(crate) async fn sync_default_branch(name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir(&name)
            .filter(|d| d.is_dir())
            .ok_or_else(|| format!("no directory for project {name}"))?;
        if has_uncommitted_changes(&dir) {
            return Err("uncommitted changes - commit or stash first".to_string());
        }
        let default = origin_default_branch(&dir);
        if read_head_branch(&dir).as_deref() != Some(default.as_str()) {
            git_output(&dir, &["checkout", &default]).map_err(|e| format!("checkout: {e}"))?;
        }
        git_output(&dir, &["pull", "--ff-only"]).map_err(|e| format!("pull: {e}"))?;
        Ok(default)
    })
    .await
    .map_err(|e| format!("sync task failed: {e}"))?
}

/// Check out the default branch, fast-forward pull, delete every other local
/// branch and prune stale remote-tracking refs. Refuses over uncommitted
/// changes. Returns a short summary.
#[tauri::command]
pub(crate) async fn cleanup_project(name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir(&name)
            .filter(|d| d.is_dir())
            .ok_or_else(|| format!("no directory for project {name}"))?;
        if has_uncommitted_changes(&dir) {
            return Err("uncommitted changes - commit or stash first".to_string());
        }
        cleanup_repo(&dir)
    })
    .await
    .map_err(|e| format!("cleanup task failed: {e}"))?
}

fn cleanup_repo(dir: &Path) -> Result<String, String> {
    let default = origin_default_branch(dir);
    if read_head_branch(dir).as_deref() != Some(default.as_str()) {
        git_output(dir, &["checkout", &default]).map_err(|e| format!("checkout: {e}"))?;
    }
    git_output(dir, &["pull", "--ff-only"]).map_err(|e| format!("pull: {e}"))?;
    let branches = git_output(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .map_err(|e| format!("list branches: {e}"))?;
    let stale: Vec<&str> = branches.lines().filter(|b| *b != default).collect();
    if !stale.is_empty() {
        git_output(dir, &[&["branch", "-D"][..], &stale].concat())
            .map_err(|e| format!("delete branches: {e}"))?;
    }
    git_output(dir, &["fetch", "--prune"]).map_err(|e| format!("fetch: {e}"))?;
    Ok(format!("{default} - deleted {} branches", stale.len()))
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
    fn touches_head_only_for_head_files() {
        let ev = |p: &str| notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from(p));
        assert!(touches_head(&ev("/r/.git/HEAD")));
        assert!(!touches_head(&ev("/r/.git/index")));
        assert!(!touches_head(&ev("/r/.git/HEAD.lock")));
        assert!(!touches_head(&notify::Event::new(notify::EventKind::Any)));
    }

    #[test]
    fn watcher_fires_on_checkout() {
        let repo = std::env::temp_dir().join(format!("igd-watch-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
                    .args(args)
                    .status()
                    .expect("git is available in the test environment")
                    .success()
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "base"]);
        let (tx, rx) = std::sync::mpsc::channel();
        let _watcher = watch_git_dirs(&[repo.join(".git")], move || {
            let _ = tx.send(());
        })
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        git(&["checkout", "-q", "-b", "feat"]);
        assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
        assert_eq!(read_head_branch(&repo).as_deref(), Some("feat"));
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn parse_head_branch_detached_and_empty() {
        assert_eq!(
            parse_head("ref: refs/heads/main\n"),
            Some("main".to_string())
        );
        assert_eq!(
            parse_head("ref: refs/heads/feat/nested-branch\n"),
            Some("feat/nested-branch".to_string())
        );
        assert_eq!(
            parse_head("a1b2c3d4e5f6a7b8c9d0a1b2c3d4e5f6a7b8c9d0\n"),
            Some("a1b2c3d".to_string())
        );
        assert_eq!(parse_head(""), None);
    }

    #[test]
    fn git_dir_follows_worktree_gitfile() {
        let root = std::env::temp_dir().join(format!("git-dir-test-{}", std::process::id()));
        let wt = root.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(git_dir(&wt), Some(wt.join("../repo/.git/worktrees/wt")));
        std::fs::remove_dir_all(&root).unwrap();
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
        for ext in "pdf,png,jpg,jpeg,heic,heif,gif,webp,mp4,mov,txt,md,csv".split(',') {
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

    /// The dirty check follows `git status`: untracked, modified and staged
    /// files count, .gitignore is honored, and anything unreadable is clean.
    #[test]
    fn uncommitted_change_detection_follows_git_status() {
        let root = std::env::temp_dir().join(format!("igd-dirty-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
                    .args(args)
                    .status()
                    .expect("git is available in the test environment")
                    .success(),
                "git {args:?} failed"
            );
        };
        git(&["init", "-q"]);
        assert!(!has_uncommitted_changes(&repo), "empty repo is clean");

        std::fs::write(repo.join("AGENTS.md"), "rules").unwrap();
        assert!(has_uncommitted_changes(&repo), "untracked file is dirty");

        git(&["add", "-A"]);
        assert!(has_uncommitted_changes(&repo), "staged file is dirty");
        git(&["commit", "-q", "-m", "base"]);
        assert!(!has_uncommitted_changes(&repo), "committed is clean");

        std::fs::write(repo.join("AGENTS.md"), "other rules").unwrap();
        assert!(has_uncommitted_changes(&repo), "modified file is dirty");
        git(&["commit", "-q", "-am", "next"]);

        std::fs::write(repo.join(".gitignore"), "ignored.txt\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "ignore"]);
        std::fs::write(repo.join("ignored.txt"), "shh").unwrap();
        assert!(!has_uncommitted_changes(&repo), "ignored-only stays clean");

        assert!(
            !has_uncommitted_changes(repo.parent().unwrap()),
            "a directory without a repo is not dirty"
        );
        assert!(
            !has_uncommitted_changes(&root.join("missing-dir")),
            "a missing directory is not dirty"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cleanup_keeps_only_the_default_branch() {
        let root = std::env::temp_dir().join(format!("igd-cleanup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(dir)
                    .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
                    .args(args)
                    .status()
                    .expect("git is available in the test environment")
                    .success(),
                "git {args:?} failed"
            );
        };
        let origin = root.join("origin.git");
        let repo = root.join("repo");
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), "repo"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["remote", "set-head", "origin", "main"]);
        git(&repo, &["branch", "feat-a"]);
        git(&repo, &["switch", "-q", "-c", "feat-b"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "unmerged"]);

        assert_eq!(cleanup_repo(&repo).unwrap(), "main - deleted 2 branches");
        assert_eq!(read_head_branch(&repo).as_deref(), Some("main"));
        assert_eq!(
            git_output(
                &repo,
                &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
            )
            .unwrap(),
            "main"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn git_remote_repo_reads_github_owner_name_from_config() {
        assert_eq!(
            parse_github_owner_name("https://github.com/owner/repo.git"),
            Some("owner/repo".into())
        );
        assert_eq!(
            parse_github_owner_name("git@github.com:owner/repo.git"),
            Some("owner/repo".into())
        );
        assert_eq!(
            parse_github_owner_name("ssh://git@github.com/owner/repo"),
            Some("owner/repo".into())
        );
        assert_eq!(parse_github_owner_name("https://gitlab.com/o/r.git"), None);

        let root = std::env::temp_dir().join(format!("igd-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dest = root.join("repo");
        fake_checkout(&dest, "owner/repo");
        assert_eq!(git_remote_repo(&dest).as_deref(), Some("owner/repo"));

        // A `.git` file (worktree) has no readable config here and reads as None.
        let wt = root.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: /elsewhere").unwrap();
        assert_eq!(git_remote_repo(&wt), None);

        // A non-GitHub remote keeps the project's context embedded.
        let gl = root.join("gl");
        std::fs::create_dir_all(gl.join(".git")).unwrap();
        std::fs::write(
            gl.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://gitlab.com/owner/repo.git\n",
        )
        .unwrap();
        assert_eq!(git_remote_repo(&gl), None);
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
