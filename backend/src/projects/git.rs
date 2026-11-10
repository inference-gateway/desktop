use super::{
    expand_home, project_dir, project_groups, project_names, project_paths, resolved_dirs,
    sanitize_name,
};
use crate::config::read_config;
use notify::{RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::Emitter;

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
/// ponytail: 64 KB cap so a runaway file cannot bloat projects.yaml.
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
pub(super) fn scan_git_repos_in(root: &Path) -> Vec<GitRepo> {
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
pub(super) fn ensure_clone(
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

/// Which projects are git checkouts, which of those have uncommitted
/// changes (staged, modified or untracked non-ignored files - anything
/// `git status --porcelain` reports), their branch heads and their GitHub
/// remotes (`owner/name`, for the "open on GitHub" affordances); powers the
/// git indicator in the UI. Derived live from the filesystem, nothing stored.
#[derive(Default, serde::Serialize)]
pub(crate) struct GitProjectStatus {
    pub(crate) git: Vec<String>,
    pub(crate) dirty: Vec<String>,
    pub(crate) branches: BTreeMap<String, String>,
    pub(crate) default_branches: BTreeMap<String, String>,
    pub(crate) remotes: BTreeMap<String, String>,
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
    let Ok(mut child) = crate::env::command("git")
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

/// Whether the checkout has any remote to pull from; content repos made by
/// `ensure_content_repo` are local-only.
fn has_remote(dir: &Path) -> bool {
    git_output(dir, &["remote"]).is_ok_and(|r| !r.is_empty())
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

/// One sweep over the resolved project directories: which are git
/// checkouts, which are dirty, their branch heads and GitHub remotes,
/// plus the git dirs to watch. Pure filesystem reads, so tests can drive
/// it without Tauri.
fn sweep_git_projects(dirs: &BTreeMap<String, PathBuf>) -> (GitProjectStatus, Vec<PathBuf>) {
    let mut status = GitProjectStatus::default();
    let mut git_dirs = Vec::new();
    for (name, dir) in dirs {
        let Some(git_dir) = git_dir(dir) else {
            continue;
        };
        git_dirs.push(git_dir);
        status.git.push(name.clone());
        if has_uncommitted_changes(dir) {
            status.dirty.push(name.clone());
        }
        if let Some(remote) = git_remote_repo(dir) {
            status.remotes.insert(name.clone(), remote);
        }
        if let Some(branch) = read_head_branch(dir) {
            status.branches.insert(name.clone(), branch);
            status
                .default_branches
                .insert(name.clone(), read_default_branch(dir));
        }
    }
    (status, git_dirs)
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
        sweep_git_projects(&resolved_dirs(
            &root,
            &names,
            &project_groups(),
            &project_paths(),
        ))
    })
    .await
    .map_err(|e| format!("git status task failed: {e}"))?;
    let watcher = watch_git_dirs(&git_dirs, move || {
        let _ = app.emit(crate::events::GIT_CHANGED, ());
    })
    .map_err(|e| format!("watching git dirs: {e}"))?;
    *state.0.lock().map_err(|e| e.to_string())? = Some(watcher);
    Ok(status)
}

/// Run `git -C <dir> <args>`, returning stdout on success and stderr (or the
/// spawn error) on failure.
pub(crate) fn git_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::env::command("git")
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
        if !has_remote(&dir) {
            return Err("no remote - local-only repo".to_string());
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

/// Remove every linked worktree, check out the default branch, fast-forward
/// pull, delete every other local branch and prune stale remote-tracking refs.
/// Refuses over uncommitted changes. Returns a short summary.
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

/// Leaves a repo without a remote untouched: there is nothing to pull or
/// prune, and its other branches exist nowhere else, so `branch -D` would
/// destroy them for good. Linked worktrees go first - they pin their branches
/// (and maybe the default one) - and a dirty one aborts before anything is
/// removed. Locked worktrees are unlocked and removed too; `unlock` fails on
/// an unlocked one, which is fine, and plain `remove` keeps git's own
/// clean-tree check as a second guard.
fn cleanup_repo(dir: &Path) -> Result<String, String> {
    if !has_remote(dir) {
        return Ok("no remote - skipped".to_string());
    }
    git_output(dir, &["worktree", "prune"]).map_err(|e| format!("prune worktrees: {e}"))?;
    let here = std::fs::canonicalize(dir).map_err(|e| format!("resolve {}: {e}", dir.display()))?;
    let list = git_output(dir, &["worktree", "list", "--porcelain"])
        .map_err(|e| format!("list worktrees: {e}"))?;
    let worktrees: Vec<&str> = list
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .skip(1)
        .filter(|p| Path::new(p) != here)
        .collect();
    if let Some(dirty) = worktrees
        .iter()
        .find(|p| has_uncommitted_changes(Path::new(p)))
    {
        return Err(format!(
            "uncommitted changes in worktree {dirty} - commit or stash first"
        ));
    }
    for path in &worktrees {
        let _ = git_output(dir, &["worktree", "unlock", path]);
        git_output(dir, &["worktree", "remove", path])
            .map_err(|e| format!("remove worktree: {e}"))?;
    }
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
    Ok(format!(
        "{default} - deleted {} branches, {} worktrees",
        stale.len(),
        worktrees.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

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
    fn touches_head_only_for_head_files() {
        let ev = |p: &str| notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from(p));
        assert!(touches_head(&ev("/r/.git/HEAD")));
        assert!(!touches_head(&ev("/r/.git/index")));
        assert!(!touches_head(&ev("/r/.git/HEAD.lock")));
        assert!(!touches_head(&notify::Event::new(notify::EventKind::Any)));
    }

    #[test]
    fn watcher_fires_on_checkout() {
        let dir = TempDir::new("watch");
        let repo = dir.path().to_path_buf();
        let git = |args: &[&str]| {
            assert!(
                crate::env::command("git")
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
        let root = TempDir::new("git-dir");
        let wt = root.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(git_dir(&wt), Some(wt.join("../repo/.git/worktrees/wt")));
    }

    #[test]
    fn scan_finds_nested_repos_and_skips_junk() {
        let root = TempDir::new("scan");
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
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["repo-a", "repo-b", "worktree"], "{repos:?}");
        assert!(
            Path::new(&repos[1].path).ends_with("group/repo-b"),
            "{repos:?}"
        );
        assert_eq!(repos[0].context.as_deref(), Some("agents rules"));
        assert_eq!(repos[1].context.as_deref(), Some("claude rules"));
        assert_eq!(repos[2].context, None);
        assert_eq!(repos[0].group, "");
        assert_eq!(repos[1].group, "group");
        assert_eq!(repos[2].group, "");
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

    #[test]
    fn ensure_clone_skips_an_existing_checkout() {
        let root = TempDir::new("clone-skip");
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
        assert!(!cloned, "existing checkout must not be re-cloned");
        assert_eq!(repo.name, "repo-a");
        assert_eq!(repo.path, dest.to_string_lossy());
        assert_eq!(repo.group, "");
        assert_eq!(repo.context.as_deref(), Some("agents rules"));
    }

    #[test]
    fn ensure_clone_refuses_a_checkout_of_a_different_repo() {
        let root = TempDir::new("clone-other");
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
    }

    #[test]
    fn ensure_clone_clones_missing_and_surfaces_failures() {
        let root = TempDir::new("clone-new");
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
    }

    /// The dirty check follows `git status`: untracked, modified and staged
    /// files count, .gitignore is honored, and anything unreadable is clean.
    #[test]
    fn uncommitted_change_detection_follows_git_status() {
        let root = TempDir::new("dirty");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            assert!(
                crate::env::command("git")
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
    }

    #[test]
    fn cleanup_keeps_only_the_default_branch() {
        let root = TempDir::new("cleanup");
        let git = |dir: &Path, args: &[&str]| {
            assert!(
                crate::env::command("git")
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
        git(&repo, &["worktree", "add", "-q", "../wt-main", "main"]);
        git(&repo, &["worktree", "add", "-q", "-b", "feat-c", "../wt-c"]);
        git(&repo, &["worktree", "lock", "--reason", "keep", "../wt-c"]);

        std::fs::write(root.join("wt-c").join("draft.md"), "wip").unwrap();
        assert!(
            cleanup_repo(&repo)
                .unwrap_err()
                .starts_with("uncommitted changes in worktree")
        );
        assert!(
            root.join("wt-main").is_dir(),
            "a dirty worktree aborts before any removal"
        );
        std::fs::remove_file(root.join("wt-c").join("draft.md")).unwrap();

        assert_eq!(
            cleanup_repo(&repo).unwrap(),
            "main - deleted 3 branches, 2 worktrees"
        );
        assert!(!root.join("wt-main").exists() && !root.join("wt-c").exists());
        assert_eq!(read_head_branch(&repo).as_deref(), Some("main"));
        assert_eq!(
            git_output(
                &repo,
                &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
            )
            .unwrap(),
            "main"
        );
    }

    #[test]
    fn cleanup_skips_repo_without_remote() {
        let dir = TempDir::new("cleanup-local");
        let repo = dir.path().to_path_buf();
        let git = |args: &[&str]| {
            assert!(
                crate::env::command("git")
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
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "base"]);
        git(&["branch", "feat-a"]);

        assert_eq!(cleanup_repo(&repo).unwrap(), "no remote - skipped");
        assert!(git_output(&repo, &["rev-parse", "--verify", "feat-a"]).is_ok());
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

        let root = TempDir::new("remote");
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
    }

    /// The status sweep reports GitHub remotes only for checkouts whose remote
    /// points at github.com (https or ssh form); non-git projects and other
    /// hosts are absent from the map.
    #[test]
    fn sweep_collects_github_remotes_only_for_github_checkouts() {
        let root = TempDir::new("remotes");
        let gh = root.join("gh");
        fake_checkout(&gh, "owner/repo");
        let ssh = root.join("ssh");
        std::fs::create_dir_all(ssh.join(".git")).unwrap();
        std::fs::write(
            ssh.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@github.com:owner/other.git\n",
        )
        .unwrap();
        let plain = root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let dirs = BTreeMap::from([
            ("gh".to_string(), gh),
            ("ssh".to_string(), ssh),
            ("plain".to_string(), plain),
        ]);
        let (status, git_dirs) = sweep_git_projects(&dirs);
        assert_eq!(
            status.remotes.get("gh").map(String::as_str),
            Some("owner/repo"),
            "https form"
        );
        assert_eq!(
            status.remotes.get("ssh").map(String::as_str),
            Some("owner/other"),
            "ssh form"
        );
        assert!(!status.remotes.contains_key("plain"), "non-git project");
        assert_eq!(status.git, vec!["gh", "ssh"], "plain is not a checkout");
        assert_eq!(git_dirs.len(), 2);
    }

    #[test]
    fn ensure_clone_keeps_a_directory_it_did_not_create() {
        let root = TempDir::new("clone-keep");
        let dest = clone_dest(&root, "owner/repo-c");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("notes.txt"), "the user's own file").unwrap();
        assert!(ensure_clone(|| Err("clone refused".into()), &dest, "owner/repo-c").is_err());
        assert!(
            dest.join("notes.txt").exists(),
            "a pre-existing directory must never be removed"
        );
    }
}
