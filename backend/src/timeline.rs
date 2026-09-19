// Timeline files (<stem>.timeline.json) inside a project directory: the
// contract between the video-editing skill and the desktop timeline view.
// The desktop only reads and writes the JSON; ffmpeg and TTS run in the agent.
use crate::download::ProgressEvent;
use crate::projects::{ProjectFile, list_local_files, project_dir};
use crate::stt::{download_binary, ensure_whisper_model, find_on_path, owned_bin};
use notify::{RecursiveMode, Watcher};
use std::io::Write;
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
/// Where exports land inside the project, out of the media pool's way.
const EXPORT_DIR: &str = "export";

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

/// Whether the ffmpeg lists every wanted entry under `query` (`-filters`,
/// `-encoders`). A build can have the filters and still have no H.264
/// encoder, and the export needs both.
fn lists(ffmpeg: &Path, query: &str, wanted: &[&str]) -> bool {
    std::process::Command::new(ffmpeg)
        .args(["-hide_banner", query])
        .output()
        .map(|out| {
            let listed = String::from_utf8_lossy(&out.stdout);
            wanted.iter().all(|f| listed.contains(f))
        })
        .unwrap_or(false)
}

const EXPORT_FILTERS: [&str; 3] = [" adelay ", " amix ", " apad "];
const EXPORT_ENCODERS: [&str; 2] = [" libx264 ", " aac "];

/// The ffmpeg used for keyframes and the export: the desktop-owned copy when
/// it can mix audio and encode H.264, else a full build on PATH.
/// ponytail: the binaries release is audio-only until inference-gateway/binaries#26 ships;
/// drop the PATH fallback once the pinned release carries the encoders.
fn video_ffmpeg() -> Result<PathBuf, String> {
    [owned_bin("ffmpeg"), find_on_path("ffmpeg")]
        .into_iter()
        .flatten()
        .find(|p| lists(p, "-filters", &EXPORT_FILTERS) && lists(p, "-encoders", &EXPORT_ENCODERS))
        .ok_or_else(|| {
            "no ffmpeg that can mix audio and encode H.264 found: the bundled build is audio-only; install a full ffmpeg (brew install ffmpeg) until a newer inference-gateway/binaries release ships".to_string()
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
    resolution: Option<String>,
    fps: Option<f64>,
    duration: Option<f64>,
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

/// Only what the audio mix and the SRT need. Placement, sizes, styles and word
/// timings are the canvas renderer's business now; speed is the exception,
/// because a retimed clip's own sound has to be stretched to match. Serde
/// ignores the fields it is not asked for, so the JSON contract is unchanged.
#[derive(serde::Deserialize)]
struct ClipFile {
    start: f64,
    end: Option<f64>,
    offset: Option<f64>,
    src: Option<String>,
    text: Option<String>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    speed_ease: Option<String>,
}

impl ClipFile {
    fn has_src(&self) -> bool {
        self.src.as_deref().is_some_and(|s| !s.is_empty())
    }

    /// Source-seconds per timeline-second over the whole clip: what the kept
    /// audio is sped up or slowed by so it still fills the clip's slot. Constant
    /// is the speed itself; the ease-in-out bell averages to `(speed+1)/2`,
    /// mirroring the frontend's `avgSpeed`.
    fn avg_speed(&self, _len: f64) -> f64 {
        let s = self.speed.unwrap_or(1.0);
        let avg = if self.speed_ease.as_deref() == Some("easeInOut") {
            (s + 1.0) / 2.0
        } else {
            s
        };
        avg.max(0.05)
    }

    /// Seconds of source a clip of timeline length `len` consumes: the average
    /// speed over its length. `atrim` takes this whole span and `atempo` stretches
    /// it back to `len`.
    fn source_consumed(&self, len: f64) -> f64 {
        self.avg_speed(len) * len.max(0.0)
    }
}

/// How long a video clip plays: to its `end`, else up to the clip after it.
/// The last open-ended clip plays its file out. The canvas stops drawing a
/// clip at the same boundary, so its sound has to stop there too.
fn clip_len(clips: &[&ClipFile], k: usize) -> Result<Option<f64>, String> {
    let c = clips[k];
    let start = c.start.max(0.0);
    match c.end {
        Some(end) if end > start => Ok(Some(end - start)),
        Some(_) => Err("video clip needs an end after its start".into()),
        None => Ok(clips
            .get(k + 1)
            .map(|n| (n.start.max(0.0) - start).max(0.0))),
    }
}

/// The part of the source file a clip plays: from `offset` for `len` seconds.
/// Empty when the clip plays its file from the top.
fn atrim_filter(offset: f64, len: Option<f64>) -> String {
    let offset = offset.max(0.0);
    match len {
        Some(len) => format!(
            "atrim=start={offset}:end={},asetpts=PTS-STARTPTS,",
            offset + len
        ),
        None if offset > 0.0 => format!("atrim=start={offset},asetpts=PTS-STARTPTS,"),
        None => String::new(),
    }
}

/// Stretch a kept clip's audio to match its speed: `factor` is source-seconds
/// per timeline-second. ffmpeg's `atempo` only takes 0.5..2.0, so a bigger
/// change is a chain of them; empty (no filter) when the speed is ~1. Ends with
/// a comma so it slots between the atrim and the adelay in the filter string.
fn atempo_filter(factor: f64) -> String {
    if (factor - 1.0).abs() < 1e-3 || factor <= 0.0 {
        return String::new();
    }
    let mut f = factor;
    let mut parts: Vec<String> = Vec::new();
    while f > 2.0 {
        parts.push("atempo=2.0".into());
        f /= 2.0;
    }
    while f < 0.5 {
        parts.push("atempo=0.5".into());
        f *= 2.0;
    }
    parts.push(format!("atempo={f}"));
    format!("{},", parts.join(","))
}

/// A clip's own length, for the clips that are not part of the video sequence.
fn own_len(c: &ClipFile) -> Option<f64> {
    c.end.filter(|e| *e > c.start).map(|e| e - c.start)
}

fn resolve_src(dir: &Path, src: &str) -> PathBuf {
    let p = Path::new(src);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        dir.join(p)
    }
}

/// Output frame size as `WxH`, a standard delivery size (1920x1080,
/// 1080x1920, 1350x1350). The default is full HD landscape.
const DEFAULT_RESOLUTION: (u32, u32) = (1920, 1080);

fn parse_resolution(raw: Option<&str>) -> Result<(u32, u32), String> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_RESOLUTION);
    };
    let dims = raw
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .filter(|(w, h)| {
            (2..=8192).contains(w) && (2..=8192).contains(h) && w % 2 == 0 && h % 2 == 0
        });
    dims.ok_or_else(|| format!("resolution must be WxH with even sides, e.g. 1920x1080: {raw}"))
}

/// SRT timestamp: HH:MM:SS,mmm.
fn srt_time(s: f64) -> String {
    let ms = (s.max(0.0) * 1000.0).round() as i64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// The SRT to stage next to the export, as (path, body): the upload sidecar
/// the short-form platforms take. `None` when the timeline has no captions.
/// The burn-in itself is drawn on the canvas with the rest of the frame, so
/// there is no ASS file and no libass any more.
fn srt_sidecar(dir: &Path, t: &TimelineFile, output: &str) -> Result<Option<Sidecar>, String> {
    let Some(track) = t.tracks.iter().find(|tr| tr.kind == "captions") else {
        return Ok(None);
    };
    let mut clips: Vec<&ClipFile> = track
        .clips
        .iter()
        .filter(|c| c.text.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .collect();
    clips.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if clips.is_empty() {
        return Ok(None);
    }
    let stem = Path::new(output)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid output name: {output}"))?;
    let srt = clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                srt_time(c.start),
                srt_time(c.end.unwrap_or(c.start)),
                c.text.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some((
        dir.join(EXPORT_DIR).join(format!("{stem}.srt")),
        srt,
    )))
}

/// One file the export stages before running ffmpeg: (path, body).
type Sidecar = (PathBuf, String);

/// Frames a second the export renders at when the timeline does not say.
const DEFAULT_FPS: f64 = 30.0;

/// What the frontend renders and what ffmpeg is waiting for: the canvas size,
/// how many frames to push at what rate, and where the file lands. One
/// computation, handed to the frontend, so the canvas size and the loop bound
/// cannot drift from the encoder's idea of them.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ExportPlan {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: f64,
    pub(crate) frames: u64,
    pub(crate) output: String,
}

/// Build the ffmpeg invocation that encodes a timeline. The picture arrives on
/// stdin as raw RGBA frames the frontend's canvas composed, one every `1/fps`,
/// so there is no video filter at all - no scale, no pad, no overlay, no
/// subtitles - because the frame already is the frame. Every audio clip is
/// delayed to its start time and mixed, plus the recording's own sound when
/// `source_audio` is `keep`; that is the only reason a video file is opened.
/// Returns the arguments, the plan the frontend renders to and the SRT sidecar
/// for the caller to write first. Pure, so it is testable.
fn export_plan(
    dir: &Path,
    stem: &str,
    json: &str,
) -> Result<(Vec<String>, ExportPlan, Option<Sidecar>), String> {
    let t: TimelineFile =
        serde_json::from_str(json).map_err(|e| format!("invalid timeline: {e}"))?;
    let output = t
        .output
        .clone()
        .unwrap_or_else(|| format!("{stem}.with-voice.mp4"));
    if output.contains('/') || output.contains('\\') {
        return Err(format!("output must be a bare file name: {output}"));
    }
    let (fw, fh) = parse_resolution(t.resolution.as_deref())?;
    let fps = t.fps.filter(|f| *f > 0.0).unwrap_or(DEFAULT_FPS);
    let duration = t.duration.filter(|d| *d > 0.0).unwrap_or_else(|| {
        t.tracks
            .iter()
            .flat_map(|tr| tr.clips.iter())
            .filter_map(|c| c.end)
            .fold(0.0, f64::max)
    });
    if duration <= 0.0 {
        return Err("nothing to export: the timeline is empty".into());
    }
    let mut clips: Vec<&ClipFile> = t
        .tracks
        .iter()
        .filter(|tr| tr.kind == "video")
        .flat_map(|tr| tr.clips.iter().filter(|c| c.has_src()))
        .collect();
    clips.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let lens: Vec<Option<f64>> = (0..clips.len())
        .map(|k| clip_len(&clips, k))
        .collect::<Result<_, _>>()?;
    let mut args: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    args.extend(["-f", "rawvideo", "-pix_fmt", "rgba", "-video_size"].map(String::from));
    args.push(format!("{fw}x{fh}"));
    args.push("-framerate".into());
    args.push(fps.to_string());
    args.extend(["-i", "pipe:0"].map(String::from));
    let mut filters: Vec<String> = Vec::new();
    let mut mix: Vec<String> = Vec::new();
    let mut input = 1;
    if t.source_audio.as_deref() == Some("keep") {
        for (k, c) in clips.iter().enumerate() {
            let src = resolve_src(dir, c.src.as_deref().unwrap_or_default());
            if !src.is_file() {
                return Err(format!("missing clip video: {}", src.display()));
            }
            args.extend(["-vn", "-i"].map(String::from));
            args.push(src.to_string_lossy().into_owned());
            let ms = (c.start.max(0.0) * 1000.0).round() as u64;
            let (src_len, tempo) = match lens[k] {
                Some(len) if len > 0.0 => (
                    Some(c.source_consumed(len)),
                    atempo_filter(c.avg_speed(len)),
                ),
                other => (other, String::new()),
            };
            filters.push(format!(
                "[{input}:a]{}{tempo}adelay={ms}|{ms}[a{input}]",
                atrim_filter(c.offset.unwrap_or(0.0), src_len)
            ));
            mix.push(format!("[a{input}]"));
            input += 1;
        }
    }
    for tr in t
        .tracks
        .iter()
        .filter(|tr| tr.kind == "audio" || tr.kind == "voice")
    {
        for c in tr.clips.iter().filter(|c| c.has_src()) {
            let src = resolve_src(dir, c.src.as_deref().unwrap_or_default());
            if !src.is_file() {
                return Err(format!("missing clip audio: {}", src.display()));
            }
            args.extend(["-vn", "-i"].map(String::from));
            args.push(src.to_string_lossy().into_owned());
            let ms = (c.start.max(0.0) * 1000.0).round() as u64;
            let volume = tr
                .gain
                .filter(|g| (*g - 1.0).abs() > f64::EPSILON)
                .map(|g| format!(",volume={g}"))
                .unwrap_or_default();
            filters.push(format!(
                "[{input}:a]{}adelay={ms}|{ms}{volume}[a{input}]",
                atrim_filter(c.offset.unwrap_or(0.0), own_len(c))
            ));
            mix.push(format!("[a{input}]"));
            input += 1;
        }
    }
    if !mix.is_empty() {
        filters.push(format!(
            "{}amix=inputs={}:normalize=0[mix];[mix]apad[a]",
            mix.concat(),
            mix.len()
        ));
        args.push("-filter_complex".into());
        args.push(filters.join(";"));
    }
    args.extend(
        [
            "-map",
            "0:v",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ]
        .map(String::from),
    );
    if mix.is_empty() {
        args.push("-an".into());
    } else {
        args.extend(["-map", "[a]", "-c:a", "aac"].map(String::from));
    }
    args.push("-shortest".into());
    let output = format!("{EXPORT_DIR}/{output}");
    args.push(dir.join(&output).to_string_lossy().into_owned());
    let sidecar = srt_sidecar(dir, &t, &output)?;
    let plan = ExportPlan {
        width: fw,
        height: fh,
        fps,
        frames: (duration * fps).ceil() as u64,
        output,
    };
    Ok((args, plan, sidecar))
}

/// The one export a window can run: an ffmpeg waiting on its stdin for the raw
/// RGBA frames the frontend's canvas composes. A single slot, like
/// ProjectWatcher; the timeline view disables Export while one is running.
#[derive(Default, Clone)]
pub(crate) struct Export(std::sync::Arc<Mutex<Option<Encoder>>>);

pub(crate) struct Encoder {
    child: std::process::Child,
    frame_bytes: usize,
    output: String,
    log: PathBuf,
}

/// ffmpeg is killed however the export ends - finished, cancelled, or the slot
/// replaced - so no orphan is left blocked on a pipe nobody will write to.
impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Encoder {
    /// ffmpeg's complaint, which is in the last lines of its log.
    fn failure(&self) -> String {
        let log = std::fs::read_to_string(&self.log).unwrap_or_default();
        let tail: Vec<&str> = log.lines().rev().take(5).collect();
        format!(
            "ffmpeg failed: {}",
            tail.into_iter().rev().collect::<Vec<_>>().join(" ")
        )
    }
}

impl Export {
    fn slot(&self) -> Result<std::sync::MutexGuard<'_, Option<Encoder>>, String> {
        self.0.lock().map_err(|e| format!("export state: {e}"))
    }

    fn begin(&self, project: &str, name: &str, timeline: &str) -> Result<ExportPlan, String> {
        let dir = dir_for(project)?;
        timeline_path(&dir, name)?;
        let stem = name.trim_end_matches(SUFFIX);
        let (args, plan, sidecar) = export_plan(&dir, stem, timeline)?;
        let export_dir = dir.join(EXPORT_DIR);
        std::fs::create_dir_all(&export_dir)
            .map_err(|e| format!("creating {}: {e}", export_dir.display()))?;
        if let Some((path, body)) = &sidecar {
            std::fs::write(path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
        }
        let ffmpeg = video_ffmpeg()?;
        let log = export_dir.join(format!("{stem}.ffmpeg.log"));
        let errors =
            std::fs::File::create(&log).map_err(|e| format!("creating {}: {e}", log.display()))?;
        let mut held = self.slot()?;
        if held.is_some() {
            return Err("an export is already running".into());
        }
        let child = std::process::Command::new(ffmpeg)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(errors)
            .spawn()
            .map_err(|e| format!("running ffmpeg: {e}"))?;
        *held = Some(Encoder {
            child,
            frame_bytes: plan.width as usize * plan.height as usize * 4,
            output: plan.output.clone(),
            log,
        });
        Ok(plan)
    }

    /// One composed frame, straight to ffmpeg's stdin. The write blocks until
    /// the encoder has drained the pipe, which is the backpressure: the
    /// frontend's next frame cannot run ahead of the encoder.
    fn write_frame(&self, frame: &[u8]) -> Result<(), String> {
        let mut held = self.slot()?;
        let encoder = held.as_mut().ok_or("no export is running")?;
        if frame.len() != encoder.frame_bytes {
            return Err(format!(
                "frame is {} bytes, expected {}",
                frame.len(),
                encoder.frame_bytes
            ));
        }
        let stdin = encoder.stdin_mut()?;
        if stdin.write_all(frame).is_err() {
            let failed = encoder.failure();
            *held = None;
            return Err(failed);
        }
        Ok(())
    }

    fn finish(&self) -> Result<String, String> {
        let mut encoder = self.slot()?.take().ok_or("no export is running")?;
        encoder.child.stdin.take();
        let status = encoder
            .child
            .wait()
            .map_err(|e| format!("waiting for ffmpeg: {e}"))?;
        if !status.success() {
            return Err(encoder.failure());
        }
        let _ = std::fs::remove_file(&encoder.log);
        Ok(encoder.output.clone())
    }

    /// Drop the running export, if any; Drop kills ffmpeg.
    pub(crate) fn cancel(&self) {
        if let Ok(mut held) = self.0.lock() {
            *held = None;
        }
    }
}

impl Encoder {
    fn stdin_mut(&mut self) -> Result<&mut std::process::ChildStdin, String> {
        self.child
            .stdin
            .as_mut()
            .ok_or("export already finished".into())
    }
}

/// Start an export: spawn the ffmpeg that will encode the frames the frontend
/// is about to render, and answer with the size and count it must render.
/// The timeline comes from the view's memory, not from disk, so an export
/// never races the debounced save.
#[tauri::command]
pub(crate) async fn export_begin(
    state: tauri::State<'_, Export>,
    project: String,
    name: String,
    timeline: String,
) -> Result<ExportPlan, String> {
    let export = state.inner().clone();
    tokio::task::spawn_blocking(move || export.begin(&project, &name, &timeline))
        .await
        .map_err(|e| e.to_string())?
}

/// One frame of raw RGBA, in render order.
#[tauri::command]
pub(crate) async fn export_frame(
    state: tauri::State<'_, Export>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let tauri::ipc::InvokeBody::Raw(frame) = request.body() else {
        return Err("export frame must be raw bytes".into());
    };
    let frame = frame.clone();
    let export = state.inner().clone();
    tokio::task::spawn_blocking(move || export.write_frame(&frame))
        .await
        .map_err(|e| e.to_string())?
}

/// Close the pipe, wait for the encode and return the output file name.
#[tauri::command]
pub(crate) async fn export_end(state: tauri::State<'_, Export>) -> Result<String, String> {
    let export = state.inner().clone();
    tokio::task::spawn_blocking(move || export.finish())
        .await
        .map_err(|e| e.to_string())?
}

/// Abandon a running export, leaving no ffmpeg behind.
#[tauri::command]
pub(crate) async fn export_cancel(state: tauri::State<'_, Export>) -> Result<(), String> {
    state.cancel();
    Ok(())
}

/// Reveal a project file in the platform file manager (Finder on macOS).
#[tauri::command]
pub(crate) fn reveal_project_file(project: String, name: String) -> Result<(), String> {
    let dir = dir_for(&project)?;
    let path = match name.split_once('/') {
        Some((sub, file)) if sub == MEDIA_DIR || sub == EXPORT_DIR => {
            bare_name(&dir.join(sub), file)?
        }
        Some(_) => return Err(format!("not a project file: {name}")),
        None => bare_name(&dir, &name)?,
    };
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
    fn export_plan_pipes_raw_frames_and_mixes_every_clip() {
        let dir = std::env::temp_dir().join(format!("infer-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["demo.mov", "b.mov", "s1.wav", "music.mp3"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let json = r#"{"duration":10,"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"src":"demo.mov"},{"start":6,"end":10,"offset":2,"src":"b.mov"}]},
            {"kind":"audio","clips":[{"start":1.5,"end":4.5,"offset":2,"src":"s1.wav"},{"start":9,"text":"draft"}]},
            {"kind":"audio","gain":0.2,"clips":[{"start":0,"src":"music.mp3"}]}]}"#;
        let (args, plan, _) = export_plan(&dir, "demo", json).unwrap();
        let joined = args.join(" ");
        assert_eq!(plan.output, "export/demo.with-voice.mp4");
        assert_eq!(
            (plan.width, plan.height, plan.fps, plan.frames),
            (1920, 1080, 30.0, 300)
        );
        assert!(
            joined.starts_with(
                "-y -hide_banner -f rawvideo -pix_fmt rgba -video_size 1920x1080 -framerate 30 -i pipe:0 -vn -i "
            ),
            "{joined}"
        );
        assert!(
            joined.contains(
                "[1:a]atrim=start=0:end=6,asetpts=PTS-STARTPTS,adelay=0|0[a1];[2:a]atrim=start=2:end=6,asetpts=PTS-STARTPTS,adelay=6000|6000[a2];[3:a]atrim=start=2:end=5,asetpts=PTS-STARTPTS,adelay=1500|1500[a3];[4:a]adelay=0|0,volume=0.2[a4];[a1][a2][a3][a4]amix=inputs=4:normalize=0[mix];[mix]apad[a]"
            ),
            "{joined}"
        );
        assert!(joined.contains(
            "-map 0:v -c:v libx264 -preset veryfast -pix_fmt yuv420p -movflags +faststart -map [a] -c:a aac -shortest"
        ), "{joined}");
        assert!(
            joined.ends_with(
                &dir.join("export/demo.with-voice.mp4")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert!(
            !joined.contains("scale="),
            "no video filter survives: {joined}"
        );

        let muted = json.replace("\"keep\"", "\"mute\"");
        let (args, _, _) = export_plan(&dir, "demo", &muted).unwrap();
        let joined = args.join(" ");
        assert!(!joined.contains("demo.mov"), "{joined}");
        assert!(joined.contains("[1:a]atrim=start=2:end=5,asetpts=PTS-STARTPTS,adelay=1500|1500[a1];[2:a]adelay=0|0,volume=0.2[a2];[a1][a2]amix=inputs=2"), "{joined}");

        let silent = r#"{"duration":4,"tracks":[{"kind":"video","clips":[{"start":0,"end":4,"src":"demo.mov"}]}]}"#;
        let (args, plan, sidecar) = export_plan(&dir, "demo", silent).unwrap();
        assert!(!args.contains(&"-filter_complex".to_string()));
        assert!(args.contains(&"-an".to_string()));
        assert_eq!(plan.frames, 120);
        assert!(sidecar.is_none());

        assert!(export_plan(&dir, "demo", r#"{"tracks":[]}"#).is_err());
        assert!(
            export_plan(
                &dir,
                "demo",
                r#"{"duration":4,"resolution":"1080","tracks":[]}"#
            )
            .is_err()
        );
        assert!(
            export_plan(
                &dir,
                "demo",
                r#"{"duration":4,"resolution":"1921x1080","tracks":[{"kind":"video","clips":[{"start":0,"end":4,"src":"demo.mov"}]}]}"#
            )
            .is_err()
        );
        assert!(
            export_plan(
                &dir,
                "demo",
                r#"{"duration":5,"tracks":[{"kind":"video","clips":[{"start":3,"end":1,"src":"demo.mov"}]}]}"#
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_plan_retimes_kept_audio_to_match_a_sped_clip() {
        let dir = std::env::temp_dir().join(format!("infer-export-speed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("demo.mov"), b"x").unwrap();

        let fast = r#"{"duration":4,"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"end":4,"offset":1,"src":"demo.mov","speed":2}]}]}"#;
        let joined = export_plan(&dir, "demo", fast).unwrap().0.join(" ");
        assert!(
            joined
                .contains("[1:a]atrim=start=1:end=9,asetpts=PTS-STARTPTS,atempo=2,adelay=0|0[a1]"),
            "{joined}"
        );

        let eased = r#"{"duration":4,"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"end":4,"src":"demo.mov","speed":2,"speed_ease":"easeInOut"}]}]}"#;
        let joined = export_plan(&dir, "demo", eased).unwrap().0.join(" ");
        assert!(
            joined.contains("atrim=start=0:end=6,asetpts=PTS-STARTPTS,atempo=1.5,"),
            "{joined}"
        );

        let slow = r#"{"duration":4,"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"end":4,"src":"demo.mov","speed":0.5}]}]}"#;
        let joined = export_plan(&dir, "demo", slow).unwrap().0.join(" ");
        assert!(
            joined.contains("atrim=start=0:end=2,asetpts=PTS-STARTPTS,atempo=0.5,"),
            "{joined}"
        );

        let still = r#"{"duration":4,"source_audio":"keep","tracks":[
            {"kind":"video","clips":[{"start":0,"end":4,"src":"demo.mov"}]}]}"#;
        let joined = export_plan(&dir, "demo", still).unwrap().0.join(" ");
        assert!(!joined.contains("atempo"), "{joined}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_plan_stages_an_srt_next_to_the_output() {
        let dir = std::env::temp_dir().join(format!("infer-captions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("demo.mov"), b"x").unwrap();
        let json = r#"{"duration":6,"resolution":"1080x1920","tracks":[
            {"kind":"video","clips":[{"start":0,"end":6,"src":"demo.mov"}]},
            {"kind":"captions","style":"karaoke","clips":[
                {"start":2,"end":4,"text":"second line"},
                {"start":0,"end":1.5,"text":"first line"},
                {"start":5,"end":6,"text":"   "}]}]}"#;
        let (args, plan, sidecar) = export_plan(&dir, "demo", json).unwrap();
        assert!(!args.join(" ").contains("subtitles="));
        assert_eq!((plan.width, plan.height), (1080, 1920));
        let (path, body) = sidecar.unwrap();
        assert_eq!(path, dir.join("export").join("demo.with-voice.srt"));
        assert_eq!(
            body,
            "1\n00:00:00,000 --> 00:00:01,500\nfirst line\n\n2\n00:00:02,000 --> 00:00:04,000\nsecond line\n"
        );

        let none = r#"{"duration":6,"tracks":[{"kind":"video","clips":[{"start":0,"end":6,"src":"demo.mov"}]}]}"#;
        assert!(export_plan(&dir, "demo", none).unwrap().2.is_none());
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
