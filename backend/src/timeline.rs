// Timeline files (<stem>.timeline.json) inside a project directory: the
// contract between the video-editing skill and the desktop timeline view.
// The desktop only reads and writes the JSON; ffmpeg and TTS run in the agent.
use crate::download::ProgressEvent;
use crate::projects::{ProjectFile, list_local_files, project_dir};
use crate::stt::{download_binary, ensure_whisper_model, find_on_path, owned_bin};
use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};
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

/// The media pool: every recording, music file and generated clip of a
/// project lives in `<project>/media` so the timeline's `src` paths and the
/// pool stay one thing. Root-level media files are listed too for projects
/// that predate the folder.
const MEDIA_DIR: &str = "media";

fn media_listing(dir: &Path) -> Vec<ProjectFile> {
    let mut files: Vec<ProjectFile> = list_local_files(&dir.join(MEDIA_DIR))
        .into_iter()
        .map(|f| ProjectFile {
            name: format!("{MEDIA_DIR}/{}", f.name),
            size: f.size,
        })
        .collect();
    files.extend(list_local_files(dir));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

#[tauri::command]
pub(crate) fn list_project_media(project: String) -> Result<Vec<ProjectFile>, String> {
    Ok(media_listing(&dir_for(&project)?))
}

fn media_dest(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let media = dir.join(MEDIA_DIR);
    let dest = bare_name(&media, name)?;
    std::fs::create_dir_all(&media).map_err(|e| e.to_string())?;
    Ok(dest)
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

// Project directories can live anywhere (custom paths, a custom projects
// root), so the asset scope is widened per project when its timeline opens
// instead of being fixed in tauri.conf.json.
#[tauri::command]
pub(crate) fn list_timelines(app: tauri::AppHandle, project: String) -> Result<Timelines, String> {
    let dir = dir_for(&project)?;
    let _ = app.asset_protocol_scope().allow_directory(&dir, true);
    Ok(Timelines {
        dir: dir.to_string_lossy().into_owned(),
        names: list_in(&dir),
    })
}

/// The one live filesystem watcher: only one project is on screen at a time,
/// so replacing it drops (and unwatches) the previous project.
pub(crate) struct ProjectWatcher(pub(crate) Mutex<Option<notify::RecommendedWatcher>>);

/// Watch the project directory and emit `project-changed` with the project
/// name on every change, so the timeline view reloads while the agent writes.
#[tauri::command]
pub(crate) fn watch_project(
    app: tauri::AppHandle,
    state: tauri::State<ProjectWatcher>,
    project: String,
) -> Result<(), String> {
    let dir = dir_for(&project)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = project.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let _ = app.emit("project-changed", &name);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(&dir, RecursiveMode::Recursive)
        .map_err(|e| format!("watching {}: {e}", dir.display()))?;
    *state.0.lock().map_err(|e| e.to_string())? = Some(watcher);
    Ok(())
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

fn has_video_filters(ffmpeg: &Path) -> bool {
    std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output()
        .map(|out| {
            let filters = String::from_utf8_lossy(&out.stdout);
            [" adelay ", " amix ", " apad ", " scale "]
                .iter()
                .all(|f| filters.contains(f))
        })
        .unwrap_or(false)
}

/// The ffmpeg used for keyframes and the export: the desktop-owned copy when
/// it has the video filters, else a full build on PATH.
/// ponytail: the binaries release is audio-only until inference-gateway/binaries#26 ships;
/// drop the PATH fallback once the pinned release has the filters.
fn video_ffmpeg() -> Result<PathBuf, String> {
    [owned_bin("ffmpeg"), find_on_path("ffmpeg")]
        .into_iter()
        .flatten()
        .find(|p| has_video_filters(p))
        .ok_or_else(|| {
            "no ffmpeg with video filters found: the bundled build is audio-only; install a full ffmpeg (brew install ffmpeg) until a newer inference-gateway/binaries release ships".to_string()
        })
}

/// Install everything the video-editing skill needs so the agent finds the
/// tools ready in ~/.infer/bin/tools: ffmpeg, whisper-cli and the whisper model.
/// Called when a project is switched to the content type.
#[tauri::command]
pub(crate) async fn prepare_content_tools(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if crate::env::mock_mode() {
        let _ = on_event.send(ProgressEvent::Ready);
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        let _ = on_event.send(ProgressEvent::Checking);
        crate::skills::install_bundled_skills();
        for name in ["ffmpeg", "whisper-cli"] {
            if owned_bin(name).is_none() {
                let _ = on_event.send(ProgressEvent::Installing);
                download_binary(name, &on_event)?;
            }
        }
        video_ffmpeg()?;
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
    let dest = media_dest(&dir, name)?;
    std::fs::copy(&src, &dest).map_err(|e| format!("copying {}: {e}", src.display()))?;
    Ok(Some(format!("{MEDIA_DIR}/{name}")))
}

/// Files dropped from Finder arrive as raw bytes (the webview keeps Tauri's
/// own drag-drop off so in-page drag-and-drop works). The name travels in a
/// hex-encoded header because header values are ASCII only.
#[tauri::command]
pub(crate) fn import_project_file(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let header = |key: &str| -> Result<String, String> {
        request
            .headers()
            .get(key)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| format!("missing {key} header"))
    };
    let project = header("x-project")?;
    let name = hex_decode(&header("x-name")?)?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw file bytes".into());
    };
    let dest = media_dest(&dir_for(&project)?, &name)?;
    std::fs::write(&dest, bytes).map_err(|e| format!("writing {}: {e}", dest.display()))?;
    Ok(format!("{MEDIA_DIR}/{name}"))
}

fn hex_decode(hex: &str) -> Result<String, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect::<Result<Vec<u8>, _>>()?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct TimelineFile {
    output: Option<String>,
    source_audio: Option<String>,
    #[serde(default)]
    tracks: Vec<TrackFile>,
}

#[derive(serde::Deserialize)]
struct TrackFile {
    kind: String,
    gain: Option<f64>,
    #[serde(default)]
    clips: Vec<ClipFile>,
}

#[derive(serde::Deserialize)]
struct ClipFile {
    start: f64,
    end: Option<f64>,
    offset: Option<f64>,
    src: Option<String>,
}

/// The part of the source file a clip plays: from `offset` for the clip's
/// length. Empty when the clip plays the whole file.
fn trim_filter(c: &ClipFile) -> String {
    let offset = c.offset.unwrap_or(0.0).max(0.0);
    match c.end {
        Some(end) if end > c.start => {
            format!(
                "atrim=start={offset}:end={},asetpts=PTS-STARTPTS,",
                offset + end - c.start
            )
        }
        _ if offset > 0.0 => format!("atrim=start={offset},asetpts=PTS-STARTPTS,"),
        _ => String::new(),
    }
}

fn resolve_src(dir: &Path, src: &str) -> PathBuf {
    let p = Path::new(src);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

/// Build the ffmpeg invocation that renders a timeline: the video's picture,
/// every audio clip (spoken or plain file) delayed to its start time and mixed together,
/// plus the original sound when `source_audio` is `keep`. Returns the
/// arguments and the output file name. Pure, so it is testable.
fn export_args(dir: &Path, stem: &str, json: &str) -> Result<(Vec<String>, String), String> {
    let t: TimelineFile =
        serde_json::from_str(json).map_err(|e| format!("invalid timeline: {e}"))?;
    let video = t
        .tracks
        .iter()
        .find(|tr| tr.kind == "video")
        .and_then(|tr| tr.clips.first())
        .and_then(|c| c.src.as_deref())
        .ok_or("timeline has no video clip")?;
    let output = t
        .output
        .clone()
        .unwrap_or_else(|| format!("{stem}.with-voice.mp4"));
    if output.contains('/') || output.contains('\\') {
        return Err(format!("output must be a bare file name: {output}"));
    }
    let mut args: Vec<String> = vec!["-y".into(), "-hide_banner".into(), "-i".into()];
    args.push(resolve_src(dir, video).to_string_lossy().into_owned());
    let mut filters = Vec::new();
    let mut mix: Vec<String> = Vec::new();
    if t.source_audio.as_deref() == Some("keep") {
        mix.push("[0:a]".into());
    }
    let mut input = 1;
    for tr in t.tracks.iter().filter(|tr| tr.kind != "video") {
        for c in tr
            .clips
            .iter()
            .filter(|c| c.src.as_deref().is_some_and(|s| !s.is_empty()))
        {
            let src = resolve_src(dir, c.src.as_deref().unwrap_or_default());
            if !src.is_file() {
                return Err(format!("missing clip audio: {}", src.display()));
            }
            args.push("-i".into());
            args.push(src.to_string_lossy().into_owned());
            let ms = (c.start.max(0.0) * 1000.0).round() as u64;
            let volume = tr
                .gain
                .filter(|g| (*g - 1.0).abs() > f64::EPSILON)
                .map(|g| format!(",volume={g}"))
                .unwrap_or_default();
            filters.push(format!(
                "[{input}]{}adelay={ms}|{ms}{volume}[a{input}]",
                trim_filter(c)
            ));
            mix.push(format!("[a{input}]"));
            input += 1;
        }
    }
    if mix.is_empty() {
        return Err("nothing to export: the timeline has no audio clips".into());
    }
    filters.push(format!(
        "{}amix=inputs={}:normalize=0[mix];[mix]apad[a]",
        mix.concat(),
        mix.len()
    ));
    args.extend(
        [
            "-filter_complex",
            &filters.join(";"),
            "-map",
            "0:v",
            "-map",
            "[a]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-shortest",
        ]
        .map(String::from),
    );
    args.push(dir.join(&output).to_string_lossy().into_owned());
    Ok((args, output))
}

/// Render `<stem>.timeline.json` with ffmpeg into the project directory and
/// return the output file name. Deterministic: same JSON, same command.
#[tauri::command]
pub(crate) async fn export_timeline(project: String, name: String) -> Result<String, String> {
    let dir = dir_for(&project)?;
    let path = timeline_path(&dir, &name)?;
    let stem = name.trim_end_matches(SUFFIX).to_string();
    let json =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let (args, output) = export_args(&dir, &stem, &json)?;
    let ffmpeg = video_ffmpeg()?;
    tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new(ffmpeg)
            .args(&args)
            .output()
            .map_err(|e| format!("running ffmpeg: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(5).collect();
            return Err(format!(
                "ffmpeg failed: {}",
                tail.into_iter().rev().collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(output)
    })
    .await
    .map_err(|e| e.to_string())?
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
    fn media_listing_prefixes_the_media_folder_and_keeps_root_files() {
        let dir = std::env::temp_dir().join(format!("infer-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("media")).unwrap();
        std::fs::write(dir.join("media/a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("old.mov"), b"y").unwrap();
        let names: Vec<String> = media_listing(&dir).into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["media/a.mp4", "old.mov"]);
        assert!(media_dest(&dir, "../x.wav").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_decode_round_trips_utf8_names() {
        let name = "Screen Recording 2026-09-16 at 18.57.04.mov";
        let hex: String = name.bytes().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex_decode(&hex).unwrap(), name);
        assert!(hex_decode("abc").is_err());
    }

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
    fn export_args_delays_and_mixes_every_clip() {
        let dir = std::env::temp_dir().join(format!("infer-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.wav"), b"x").unwrap();
        std::fs::write(dir.join("music.mp3"), b"x").unwrap();
        let json = r#"{"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"src":"demo.mov"}]},
            {"kind":"audio","clips":[{"start":1.5,"end":4.5,"offset":2,"src":"s1.wav"},{"start":9,"text":"draft"}]},
            {"kind":"audio","gain":0.2,"clips":[{"start":0,"src":"music.mp3"}]}]}"#;
        let (args, output) = export_args(&dir, "demo", json).unwrap();
        assert_eq!(output, "demo.with-voice.mp4");
        let joined = args.join(" ");
        assert!(joined.contains("[1]atrim=start=2:end=5,asetpts=PTS-STARTPTS,adelay=1500|1500[a1];[2]adelay=0|0,volume=0.2[a2];[0:a][a1][a2]amix=inputs=3:normalize=0[mix];[mix]apad[a]"), "{joined}");
        assert!(
            joined.ends_with(
                &dir.join("demo.with-voice.mp4")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert!(joined.contains("-c:v copy -c:a aac -shortest"));

        let muted = json.replace("\"keep\"", "\"mute\"");
        let (args, _) = export_args(&dir, "demo", &muted).unwrap();
        assert!(args.join(" ").contains("[a1][a2]amix=inputs=2"));

        assert!(
            export_args(
                &dir,
                "demo",
                r#"{"tracks":[{"kind":"video","clips":[{"start":0,"src":"d.mov"}]}]}"#
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
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
