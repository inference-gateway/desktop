use crate::env::infer_dir;
use std::path::PathBuf;

/// Read the prompt history shared with the CLI.
#[tauri::command]
pub(crate) fn read_history() -> Result<Vec<String>, String> {
    read_history_file(prompt_history_path())
}

/// Read the desktop's own bash-command history.
#[tauri::command]
pub(crate) fn read_bash_history() -> Result<Vec<String>, String> {
    read_history_file(bash_history_path())
}

/// Append a line to the prompt history shared with the CLI.
#[tauri::command]
pub(crate) fn append_history(line: String) -> Result<(), String> {
    append_history_line(prompt_history_path(), &line)
}

/// Append a line to the desktop's own bash-command history.
#[tauri::command]
pub(crate) fn append_bash_history(line: String) -> Result<(), String> {
    append_history_line(bash_history_path(), &line)
}

/// Prompt history shared with the CLI: `~/.infer/history`, or
/// `~/.infer/history/history` when that path is a directory (some CLI
/// versions nest the file there).
fn prompt_history_path() -> PathBuf {
    let base = infer_dir().join("history");
    if base.is_dir() {
        base.join("history")
    } else {
        base
    }
}

/// The desktop's own bash-command history (issue #185), a sibling file of
/// the CLI's prompt history.
fn bash_history_path() -> PathBuf {
    infer_dir().join("bash_history")
}

/// Read the lines of a history file. Returns an empty vec when the file
/// does not exist yet.
fn read_history_file(path: PathBuf) -> Result<Vec<String>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(content.lines().map(|l| l.to_string()).collect())
}

/// Append a line to a history file (creates the file and parent
/// directories if missing).
fn append_history_line(path: PathBuf, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{}", line).map_err(|e| e.to_string())?;
    Ok(())
}
