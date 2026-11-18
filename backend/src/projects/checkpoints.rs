use super::git::git_output;
use super::project_dir;
use std::path::Path;

/// The tracked-file set for a content repo is the timeline JSON and text only.
/// Media (large, append-only) and the derived export/ dir stay untracked, so
/// git history is tiny and revert never touches the media pool. The extension
/// globs are belt-and-suspenders in case a media file lands outside media/.
/// tmp/ holds the video-editing skill's scratch files (frames, transcripts).
const CONTENT_GITIGNORE: &str =
    "media/\n*.mp4\n*.mov\n*.m4v\n*.webm\n*.wav\n*.mp3\n*.m4a\nexport/\ntmp/\n";

/// Initialize the content project's local git repo if absent and lay down the
/// media .gitignore. Idempotent: an existing repo is left alone, and an existing
/// .gitignore only gains the template lines it lacks.
pub(crate) fn ensure_content_repo(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if !dir.join(".git").exists() {
        git_output(dir, &["init", "-b", "main"])?;
    }
    let gitignore = dir.join(".gitignore");
    let mut current = std::fs::read_to_string(&gitignore).unwrap_or_default();
    let missing: Vec<&str> = CONTENT_GITIGNORE
        .lines()
        .filter(|line| !current.lines().any(|have| have.trim() == *line))
        .collect();
    if !missing.is_empty() {
        if !current.is_empty() && !current.ends_with('\n') {
            current.push('\n');
        }
        for line in missing {
            current.push_str(line);
            current.push('\n');
        }
        std::fs::write(&gitignore, current).map_err(|e| format!("writing .gitignore: {e}"))?;
    }
    Ok(())
}

/// The email every checkpoint is committed under, which is also how a
/// checkpoint is told from a commit made by hand or by an agent.
/// shortcut: GIT_COMMITTER_EMAIL or a `committer.email` setting wins over it
/// and switches undo off, commit through the environment if someone hits that.
const CHECKPOINT_EMAIL: &str = "desktop@localhost";

fn head_sha(dir: &Path) -> Result<String, String> {
    git_output(dir, &["rev-parse", "HEAD"])
}

/// Whether HEAD has a parent commit - i.e. there is a prior state to undo to.
fn head_has_parent(dir: &Path) -> bool {
    git_output(dir, &["rev-parse", "--verify", "--quiet", "HEAD~1"]).is_ok()
}

fn head_is_checkpoint(dir: &Path) -> bool {
    git_output(dir, &["log", "-1", "--format=%ce"]).is_ok_and(|email| email == CHECKPOINT_EMAIL)
}

/// Whether undo has a checkpoint to take back. A commit the app did not make
/// is never one, so a repository that became a content project keeps the
/// history it came with.
fn can_undo(dir: &Path) -> bool {
    head_has_parent(dir) && head_is_checkpoint(dir)
}

/// Commit every change under `dir` with a fixed identity (a fresh machine may
/// have no git config). Returns the new HEAD sha, or None when the tree was
/// already clean so empty commits never pile up in the undo history.
pub(crate) fn checkpoint(dir: &Path, message: &str) -> Result<Option<String>, String> {
    git_output(dir, &["add", "-A"])?;
    if git_output(dir, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(None);
    }
    git_output(
        dir,
        &[
            "-c",
            &format!("user.email={CHECKPOINT_EMAIL}"),
            "-c",
            "user.name=Inference Gateway Desktop",
            "commit",
            "-m",
            message,
            "--no-verify",
        ],
    )?;
    Ok(Some(head_sha(dir)?))
}

/// The redo target and the commit HEAD must still sit on for a redo to be safe.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UndoResult {
    popped: String,
    base: String,
    can_undo: bool,
}

/// Commit the current state of a content project. Called after every timeline
/// save so each saved change is one undo step, and lazily inits the repo on the
/// first edit. Returns whether an undo is now possible.
#[tauri::command]
pub(crate) async fn checkpoint_content_project(
    name: String,
    message: String,
) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir(&name).ok_or_else(|| format!("no directory for project {name}"))?;
        ensure_content_repo(&dir)?;
        checkpoint(&dir, &message)?;
        Ok(can_undo(&dir))
    })
    .await
    .map_err(|e| format!("checkpoint task failed: {e}"))?
}

/// Undo one checkpoint: hard-reset to HEAD~1. Returns the popped commit (the
/// redo target), the commit now at HEAD (the base a later redo must match), and
/// whether a further undo is possible.
fn undo(dir: &Path) -> Result<UndoResult, String> {
    if !can_undo(dir) {
        return Err("nothing to undo".to_string());
    }
    let popped = head_sha(dir)?;
    git_output(dir, &["reset", "--hard", "HEAD~1"])?;
    Ok(UndoResult {
        popped,
        base: head_sha(dir)?,
        can_undo: can_undo(dir),
    })
}

#[tauri::command]
pub(crate) async fn undo_content_project(name: String) -> Result<UndoResult, String> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir(&name)
            .filter(|d| d.join(".git").exists())
            .ok_or_else(|| format!("{name} is not a git project"))?;
        undo(&dir)
    })
    .await
    .map_err(|e| format!("undo task failed: {e}"))?
}

/// Redo a previously undone commit: hard-reset to `sha`, but only if HEAD still
/// sits on `base` (the commit we undid to). A mismatch means a new commit landed
/// since - the redo is stale, so it is refused and the caller drops it.
#[tauri::command]
pub(crate) async fn redo_content_project(
    name: String,
    sha: String,
    base: String,
) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let dir = project_dir(&name)
            .filter(|d| d.join(".git").exists())
            .ok_or_else(|| format!("{name} is not a git project"))?;
        if head_sha(&dir)? != base {
            return Err("redo unavailable - project changed".to_string());
        }
        let commit = format!("{sha}^{{commit}}");
        git_output(&dir, &["rev-parse", "--verify", "--quiet", &commit])
            .map_err(|_| "redo target missing".to_string())?;
        git_output(&dir, &["reset", "--hard", &sha])?;
        Ok(can_undo(&dir))
    })
    .await
    .map_err(|e| format!("redo task failed: {e}"))?
}

/// Whether a content project has an undoable checkpoint, for the toolbar button
/// state on load. A project with no repo (never edited) has nothing to undo.
#[tauri::command]
pub(crate) fn content_can_undo(name: String) -> bool {
    project_dir(&name)
        .filter(|d| d.join(".git").exists())
        .is_some_and(|d| can_undo(&d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn ensure_content_repo_is_idempotent() {
        let repo = TempDir::new("content-init");
        ensure_content_repo(&repo).unwrap();
        ensure_content_repo(&repo).unwrap();
        assert!(repo.join(".git").is_dir());
        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert_eq!(gitignore, CONTENT_GITIGNORE);
        std::fs::write(repo.join(".gitignore"), "media/\nnotes.txt").unwrap();
        ensure_content_repo(&repo).unwrap();
        ensure_content_repo(&repo).unwrap();
        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(gitignore.starts_with("media/\nnotes.txt\n*.mp4\n"));
        assert!(gitignore.ends_with("export/\ntmp/\n"));
        assert_eq!(gitignore.matches("media/").count(), 1);
    }

    #[test]
    fn checkpoint_commits_changes_then_skips_a_clean_tree() {
        let repo = TempDir::new("content-ckpt");
        ensure_content_repo(&repo).unwrap();
        assert!(checkpoint(&repo, "init").unwrap().is_some());
        assert_eq!(checkpoint(&repo, "again").unwrap(), None);
        std::fs::write(repo.join("a.timeline.json"), "1").unwrap();
        assert!(checkpoint(&repo, "edit").unwrap().is_some());
        assert_eq!(checkpoint(&repo, "again").unwrap(), None);
    }

    #[test]
    fn undo_and_redo_walk_one_commit() {
        let repo = TempDir::new("content-undo");
        let file = repo.join("main.timeline.json");
        ensure_content_repo(&repo).unwrap();
        std::fs::write(&file, "v1").unwrap();
        checkpoint(&repo, "A").unwrap();
        let a = head_sha(&repo).unwrap();
        assert!(!head_has_parent(&repo));
        std::fs::write(&file, "v2").unwrap();
        let b = checkpoint(&repo, "B").unwrap().unwrap();
        assert!(head_has_parent(&repo));

        let undone = undo(&repo).unwrap();
        assert_eq!(undone.popped, b);
        assert_eq!(undone.base, a);
        assert!(!undone.can_undo);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1");

        git_output(&repo, &["reset", "--hard", &undone.popped]).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
    }

    fn commit_by_hand(repo: &Path, file: &Path, text: &str) {
        std::fs::write(file, text).unwrap();
        git_output(repo, &["add", "-A"]).unwrap();
        let identity = ["-c", "user.name=t", "-c", "user.email=t@localhost"];
        git_output(repo, &[&identity[..], &["commit", "-m", text]].concat()).unwrap();
    }

    #[test]
    fn undo_stops_at_a_commit_the_app_did_not_make() {
        let repo = TempDir::new("content-by-hand");
        let file = repo.join("main.timeline.json");
        ensure_content_repo(&repo).unwrap();
        commit_by_hand(&repo, &file, "v1");
        commit_by_hand(&repo, &file, "v2");
        assert!(undo(&repo).is_err());

        std::fs::write(&file, "v3").unwrap();
        checkpoint(&repo, "edit").unwrap();
        assert!(!undo(&repo).unwrap().can_undo);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
        assert!(undo(&repo).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
    }
}
