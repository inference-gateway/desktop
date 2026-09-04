// Timeline files (<stem>.timeline.json) inside a project directory: the
// contract between the video-editing skill and the desktop timeline view.
// The desktop only reads and writes the JSON; ffmpeg and TTS run in the agent.
use crate::download::ProgressEvent;
use crate::projects::project_dir;
use crate::stt::{bin_path, download_binary, ensure_whisper_model};
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "webm"];

const SUFFIX: &str = ".timeline.json";

#[derive(Debug, serde::Serialize)]
pub(crate) struct Timelines {
    pub(crate) dir: String,
    pub(crate) names: Vec<String>,
}

fn dir_for(project: &str) -> Result<PathBuf, String> {
    project_dir(project).ok_or_else(|| "project directory not resolved".to_string())
}

/// Confine a file name to the project directory: bare names only.
fn bare_name(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(format!("invalid file name {name:?}: bare file names only"));
    }
    Ok(dir.join(name))
}

fn timeline_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !name.ends_with(SUFFIX) || name.len() == SUFFIX.len() {
        return Err(format!(
            "invalid timeline name {name:?}: expected <stem>{SUFFIX}"
        ));
    }
    bare_name(dir, name)
}

fn list_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(SUFFIX) && n.len() > SUFFIX.len())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tauri::command]
pub(crate) fn list_timelines(project: String) -> Result<Timelines, String> {
    let dir = dir_for(&project)?;
    Ok(Timelines {
        dir: dir.to_string_lossy().into_owned(),
        names: list_in(&dir),
    })
}

#[tauri::command]
pub(crate) fn read_timeline(project: String, name: String) -> Result<String, String> {
    let path = timeline_path(&dir_for(&project)?, &name)?;
    std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))
}

#[tauri::command]
pub(crate) fn write_timeline(project: String, name: String, data: String) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(&data)
        .map_err(|e| format!("invalid timeline JSON: {e}"))?;
    let dir = dir_for(&project)?;
    let path = timeline_path(&dir, &name)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| format!("writing {}: {e}", path.display()))
}

const SKILL_MD: &str = include_str!("../skills/video-editing/SKILL.md");

/// Write the bundled video-editing skill to ~/.infer/skills so the agent
/// always finds it, whether or not the catalog copy is installed.
fn install_bundled_skill() -> Result<(), String> {
    let dir = crate::env::home_dir()
        .join(".infer")
        .join("skills")
        .join("video-editing");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("SKILL.md");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SKILL_MD) {
        std::fs::write(&path, SKILL_MD).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Install everything the video-editing skill needs so the agent finds the
/// tools ready in ~/.infer/bin: ffmpeg, whisper-cli and the whisper model.
/// Called when a project is switched to the content type.
#[tauri::command]
pub(crate) async fn prepare_content_tools(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if crate::env::mock_mode() {
        let _ = on_event.send(ProgressEvent::Ready);
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        let _ = on_event.send(ProgressEvent::Checking);
        install_bundled_skill()?;
        for name in ["ffmpeg", "whisper-cli"] {
            if bin_path(name).is_none() {
                let _ = on_event.send(ProgressEvent::Installing);
                download_binary(name, &on_event)?;
            }
        }
        ensure_whisper_model(|received, total| {
            let _ = on_event.send(ProgressEvent::Downloading { received, total });
        })?;
        let _ = on_event.send(ProgressEvent::Ready);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pick a video with the native file dialog and copy it into the project
/// directory under its base name. Returns `None` when the user cancels.
/// A plain copy: no base64 round trip through the webview for large files.
#[tauri::command]
pub(crate) async fn add_project_video(
    app: tauri::AppHandle,
    project: String,
) -> Result<Option<String>, String> {
    let dir = dir_for(&project)?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Video", &VIDEO_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("file dialog failed: {e}"))?;
    let Some(tauri_plugin_dialog::FilePath::Path(src)) = picked else {
        return Ok(None);
    };
    let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
        return Err(format!("invalid file name: {}", src.display()));
    };
    let dest = bare_name(&dir, name)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::copy(&src, &dest).map_err(|e| format!("copying {}: {e}", src.display()))?;
    Ok(Some(name.to_string()))
}

/// Reveal a project file in the platform file manager (Finder on macOS).
#[tauri::command]
pub(crate) fn reveal_project_file(project: String, name: String) -> Result<(), String> {
    let path = bare_name(&dir_for(&project)?, &name)?;
    if !path.is_file() {
        return Err(format!("{} does not exist yet", path.display()));
    }
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .status();
    #[cfg(not(target_os = "macos"))]
    let status = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(&path))
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("file manager exited with {s}")),
        Err(e) => Err(format!("launching file manager: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_path_accepts_only_bare_timeline_names() {
        let dir = Path::new("/tmp/project");
        for bad in [
            "",
            ".timeline.json",
            "a/b.timeline.json",
            "..\\x.timeline.json",
            "demo.json",
            "demo.timeline.json/",
        ] {
            assert!(
                timeline_path(dir, bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert_eq!(
            timeline_path(dir, "demo.timeline.json").unwrap(),
            dir.join("demo.timeline.json")
        );
    }

    #[test]
    fn list_in_returns_sorted_timeline_files_only() {
        let dir = std::env::temp_dir().join(format!("infer-timelines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.timeline.json"), "{}").unwrap();
        std::fs::write(dir.join("a.timeline.json"), "{}").unwrap();
        std::fs::write(dir.join("a.mov"), "x").unwrap();
        std::fs::create_dir(dir.join("c.timeline.json")).unwrap();
        assert_eq!(list_in(&dir), vec!["a.timeline.json", "b.timeline.json"]);
        assert!(list_in(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
