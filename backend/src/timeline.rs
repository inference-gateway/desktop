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

/// Whether the ffmpeg's filter list contains every wanted filter.
fn has_filters(ffmpeg: &Path, wanted: &[&str]) -> bool {
    std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output()
        .map(|out| {
            let filters = String::from_utf8_lossy(&out.stdout);
            wanted.iter().all(|f| filters.contains(f))
        })
        .unwrap_or(false)
}

const VIDEO_FILTERS: [&str; 5] = [" adelay ", " amix ", " apad ", " scale ", " overlay "];

/// The ffmpeg used for keyframes and the export: the desktop-owned copy when
/// it has the video filters, else a full build on PATH.
/// ponytail: the binaries release is audio-only until inference-gateway/binaries#26 ships;
/// drop the PATH fallback once the pinned release has the filters.
fn video_ffmpeg() -> Result<PathBuf, String> {
    [owned_bin("ffmpeg"), find_on_path("ffmpeg")]
        .into_iter()
        .flatten()
        .find(|p| has_filters(p, &VIDEO_FILTERS))
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
    resolution: Option<String>,
    source_audio: Option<String>,
    #[serde(default)]
    tracks: Vec<TrackFile>,
}

#[derive(serde::Deserialize)]
struct TrackFile {
    kind: String,
    gain: Option<f64>,
    style: Option<String>,
    position: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    #[serde(default)]
    clips: Vec<ClipFile>,
}

#[derive(serde::Deserialize)]
struct ClipFile {
    start: f64,
    end: Option<f64>,
    offset: Option<f64>,
    src: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    text: Option<String>,
    #[serde(default)]
    words: Vec<WordFile>,
}

/// When one word of a caption clip is spoken (absolute seconds), from the
/// transcript; `text` falls back to the i-th word of the clip's text.
#[derive(serde::Deserialize)]
struct WordFile {
    text: Option<String>,
    start: f64,
    end: f64,
}

impl ClipFile {
    fn has_src(&self) -> bool {
        self.src.as_deref().is_some_and(|s| !s.is_empty())
    }
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

/// The filter that composites one overlay clip onto the running picture
/// label: the card is shifted to its start time, scaled against the output
/// frame (`width`/`height`, else the full frame width, as the preview does)
/// and shown only inside its range.
fn overlay_filter(
    c: &ClipFile,
    input: usize,
    prev: &str,
    end: f64,
    (fw, fh): (u32, u32),
) -> String {
    let start = c.start.max(0.0);
    let px = |v: Option<f64>, side: u32| {
        v.map_or("-2".to_string(), |v| {
            ((f64::from(side) * v).round() as i64).max(2).to_string()
        })
    };
    let width = c
        .width
        .or(if c.height.is_none() { Some(1.0) } else { None });
    format!(
        "[{input}:v]setpts=PTS-STARTPTS+{start}/TB,scale=w={}:h={}[o{input}];{prev}[o{input}]overlay=x=W*{}:y=H*{}:eof_action=pass:enable='between(t,{start},{end})'[v{input}]",
        px(width, fw),
        px(c.height, fh),
        c.x.unwrap_or(0.0),
        c.y.unwrap_or(0.0)
    )
}

/// Caption style presets, mirrored from CAPTION_STYLES in
/// frontend/lib/timeline.ts and CaptionOverlay in TimelineView.tsx: the
/// preview and the burned-in export must match. `fontsize` is a fraction of
/// the frame height (ASS PlayResY) and `outline` a fraction of the font size;
/// colours are ASS &HAABBGGRR, where `primary` is a word once it has been
/// spoken and `secondary` before, which is what the `\k` tags swap.
/// ponytail: ASS cannot express per-axis band padding or a semi-bold weight;
/// the closest standing-in values stand in for the preview's.
struct CaptionPreset {
    name: &'static str,
    fontsize: f64,
    outline: f64,
    uppercase: bool,
    bold: bool,
    band: bool,
    primary: &'static str,
    secondary: &'static str,
    karaoke: Option<&'static str>,
}

const CAPTION_PRESETS: [CaptionPreset; 4] = [
    // Broadcast subtitle: white on a translucent box hugging the text.
    CaptionPreset {
        name: "Classic",
        fontsize: 0.048,
        outline: 0.25,
        uppercase: false,
        bold: false,
        band: true,
        primary: "&H00FFFFFF",
        secondary: "&H00FFFFFF",
        karaoke: None,
    },
    // The short-form punch line: huge uppercase yellow with a thick outline.
    CaptionPreset {
        name: "Bold",
        fontsize: 0.09,
        outline: 0.09,
        uppercase: true,
        bold: true,
        band: false,
        primary: "&H004DE1FF",
        secondary: "&H004DE1FF",
        karaoke: None,
    },
    // Word-by-word colour pop: white until spoken, then green, and it stays.
    CaptionPreset {
        name: "Highlight",
        fontsize: 0.07,
        outline: 0.07,
        uppercase: false,
        bold: true,
        band: false,
        primary: "&H0088FF2B",
        secondary: "&H00FFFFFF",
        karaoke: Some("k"),
    },
    // Filled as spoken: dim ahead of the playhead, white behind it.
    CaptionPreset {
        name: "Karaoke",
        fontsize: 0.06,
        outline: 0.05,
        uppercase: false,
        bold: false,
        band: false,
        primary: "&H00FFFFFF",
        secondary: "&H008A8A8A",
        karaoke: Some("kf"),
    },
];

/// The captions track's preset; unknown names fall back to the default
/// (Classic), like the preview's captionStyle.
fn caption_preset(style: Option<&str>) -> &'static CaptionPreset {
    CAPTION_PRESETS
        .iter()
        .find(|p| style.is_some_and(|s| p.name.eq_ignore_ascii_case(s)))
        .unwrap_or(&CAPTION_PRESETS[0])
}

/// ASS timestamp: H:MM:SS.CC.
fn ass_time(s: f64) -> String {
    let cs = (s.max(0.0) * 100.0).round() as i64;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
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

/// A bare file name as a filtergraph option value: two unescape rounds run
/// (the filtergraph parser, then the subtitles filter's own option split on
/// `:`), so `:` and `\` carry doubled backslashes.
fn filter_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            ':' => out.push_str("\\\\:"),
            '\\' => out.push_str("\\\\\\\\"),
            ',' | '[' | ']' | ';' => out.push_str(&format!("\\{c}")),
            _ => out.push(c),
        }
    }
    out
}

/// A caption clip's ASS text: per-word `\k`/`\kf` centisecond tags for the
/// word presets (timing from `words`, the word itself falling back to the
/// i-th token of the text), else the raw text, uppercased for Bold. A pause
/// before a word gets its own tag so the burn-in tracks the preview, which
/// times every word from its absolute `start`.
fn caption_text(preset: &CaptionPreset, c: &ClipFile) -> String {
    let raw = c.text.as_deref().unwrap_or_default();
    if let Some(tag) = preset.karaoke
        && !c.words.is_empty()
    {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        let mut out = String::new();
        let mut cursor = c.start;
        for (i, w) in c.words.iter().enumerate() {
            let gap = ((w.start - cursor).max(0.0) * 100.0).round() as i64;
            if gap > 0 {
                out.push_str(&format!("{{\\{tag}{gap}}}"));
            }
            if i > 0 {
                out.push(' ');
            }
            let cs = ((w.end - w.start).max(0.0) * 100.0).round() as i64;
            out.push_str(&format!("{{\\{tag}{cs}}}"));
            out.push_str(
                w.text
                    .as_deref()
                    .unwrap_or(tokens.get(i).copied().unwrap_or("")),
            );
            cursor = w.end;
        }
        return out;
    }
    if preset.uppercase {
        raw.to_uppercase()
    } else {
        raw.to_string()
    }
}

/// The captions sidecars to write before running ffmpeg, as (path, body):
/// the ASS burn-in source next to the timeline and an SRT upload file next
/// to the output. Empty when the timeline has no captions. One ASS style per
/// preset; alignment carries the track's `position`.
/// ponytail: a timeline stem with a `'` cannot be embedded in the subtitles
/// argument (ffmpeg then fails loudly, which beats a silent skip); port
/// av_escape if that ever matters.
fn caption_sidecars(
    dir: &Path,
    stem: &str,
    t: &TimelineFile,
    output: &str,
    (fw, fh): (u32, u32),
) -> Result<Vec<Sidecar>, String> {
    let Some(track) = t.tracks.iter().find(|tr| tr.kind == "captions") else {
        return Ok(Vec::new());
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
        return Ok(Vec::new());
    }
    let preset = caption_preset(track.style.as_deref());
    let an = match track.position.as_deref() {
        Some("center") => 5,
        Some("top") => 8,
        _ => 2,
    };
    let fontsize = (preset.fontsize * f64::from(fh)).round() as i32;
    let margin = (0.06 * f64::from(fh)).round() as i32;
    let (border, outline_colour) = if preset.band {
        (3, "&H99000000")
    } else {
        (1, "&H00000000")
    };
    let outline = (preset.outline * f64::from(fontsize)).round() as i32;
    let bold = if preset.bold { -1 } else { 0 };
    // A caption block the user dragged is anchored by its centre, as the
    // preview places it; otherwise the style's alignment and margin place it.
    let place = match (track.x, track.y) {
        (Some(x), Some(y)) => format!(
            "{{\\an5\\pos({},{})}}",
            (x.clamp(0.0, 1.0) * f64::from(fw)).round() as i32,
            (y.clamp(0.0, 1.0) * f64::from(fh)).round() as i32
        ),
        _ => format!("{{\\an{an}}}"),
    };
    let mut dialogue = String::new();
    for c in &clips {
        let end = c
            .end
            .filter(|e| *e > c.start)
            .ok_or_else(|| format!("caption clip at {} needs an end after its start", c.start))?;
        dialogue.push_str(&format!(
            "Dialogue: 0,{},{},{},,0,0,0,,{place}{}\n",
            ass_time(c.start),
            ass_time(end),
            preset.name,
            caption_text(preset, c)
        ));
    }
    let style_line = format!(
        "Style: {},Arial,{fontsize},{},{},{outline_colour},&H00000000,{bold},0,0,0,100,100,0,0,{border},{outline},0,{an},60,60,{margin},1",
        preset.name, preset.primary, preset.secondary
    );
    let ass = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {fw}\nPlayResY: {fh}\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n{style_line}\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n{dialogue}"
    );
    let srt_name = Path::new(output)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("invalid output name: {output}"))?
        .to_string();
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
    Ok(vec![
        (dir.join(format!("{stem}.ass")), ass),
        (dir.join(EXPORT_DIR).join(format!("{srt_name}.srt")), srt),
    ])
}

/// One file the export stages before running ffmpeg: (path, body).
type Sidecar = (PathBuf, String);

/// Build the ffmpeg invocation that renders a timeline: the video track as a
/// sequence (every clip trimmed by its `offset` and range, freeze-framed across
/// gaps and concatenated in timeline order, so all other tracks keep their
/// absolute times), every audio clip delayed to its start time and mixed
/// together, plus the original sound when `source_audio` is `keep`, and every
/// overlay card composited over the picture. The picture is scaled to fit and
/// padded to the timeline's `resolution` so the export is always a standard
/// delivery size. Captions burn in: a captions track is written to an ASS file
/// next to the timeline (chained with `subtitles=` onto the final picture) and
/// an SRT is staged next to the output, both returned as sidecars for the
/// caller to write before running ffmpeg. Returns the arguments, the output
/// file name and the sidecars. Pure, so it is testable.
fn export_args(
    dir: &Path,
    stem: &str,
    json: &str,
) -> Result<(Vec<String>, String, Vec<Sidecar>), String> {
    let t: TimelineFile =
        serde_json::from_str(json).map_err(|e| format!("invalid timeline: {e}"))?;
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
    if clips.is_empty() {
        return Err("timeline has no video clip".into());
    }
    let output = t
        .output
        .clone()
        .unwrap_or_else(|| format!("{stem}.with-voice.mp4"));
    if output.contains('/') || output.contains('\\') {
        return Err(format!("output must be a bare file name: {output}"));
    }
    let (fw, fh) = parse_resolution(t.resolution.as_deref())?;
    let mut args: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    let simple =
        clips.len() == 1 && clips[0].offset.unwrap_or(0.0) == 0.0 && clips[0].end.is_none();
    let keep = t.source_audio.as_deref() == Some("keep");
    let mut mix: Vec<String> = Vec::new();
    let mut filters: Vec<String>;
    let mut picture;
    if simple {
        args.push("-i".into());
        args.push(
            resolve_src(dir, clips[0].src.as_deref().unwrap_or_default())
                .to_string_lossy()
                .into_owned(),
        );
        filters = vec![format!(
            "[0:v]scale=w={fw}:h={fh}:force_original_aspect_ratio=decrease,pad=w={fw}:h={fh}:x=(ow-iw)/2:y=(oh-ih)/2,setsar=1[v0]"
        )];
        picture = "[v0]".to_string();
        if keep {
            mix.push("[0:a]".into());
        }
    } else {
        filters = Vec::new();
        let mut concat_in = String::new();
        let mut prev_end = 0.0;
        for (k, c) in clips.iter().enumerate() {
            args.push("-i".into());
            args.push(
                resolve_src(dir, c.src.as_deref().unwrap_or_default())
                    .to_string_lossy()
                    .into_owned(),
            );
            let start = c.start.max(0.0);
            let off = c.offset.unwrap_or(0.0).max(0.0);
            let len = match c.end {
                Some(end) if end > start => end - start,
                Some(_) => return Err("video clip needs an end after its start".into()),
                None => clips
                    .get(k + 1)
                    .map_or(f64::INFINITY, |n| (n.start.max(0.0) - start).max(0.0)),
            };
            let gap_before = (start - prev_end).max(0.0);
            prev_end = start + len;
            let trim = if len.is_finite() {
                format!("trim=start={off}:end={}", off + len)
            } else {
                format!("trim=start={off}")
            };
            filters.push(format!(
                "[{k}:v]{trim},setpts=PTS-STARTPTS,scale=w={fw}:h={fh}:force_original_aspect_ratio=decrease,pad=w={fw}:h={fh}:x=(ow-iw)/2:y=(oh-ih)/2,setsar=1,format=yuv420p,tpad=start_duration={gap_before}[vs{k}]"
            ));
            concat_in.push_str(&format!("[vs{k}]"));
            if keep {
                let ms = (start * 1000.0).round() as u64;
                filters.push(format!("[{k}:a]{}adelay={ms}|{ms}[av{k}]", trim_filter(c)));
                mix.push(format!("[av{k}]"));
            }
        }
        filters.push(format!("{concat_in}concat=n={}:v=1:a=0[vcat]", clips.len()));
        picture = "[vcat]".to_string();
    }
    let mut input = if simple { 1 } else { clips.len() };
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
    for tr in t.tracks.iter().filter(|tr| tr.kind == "overlay") {
        for c in tr.clips.iter().filter(|c| c.has_src()) {
            let src = resolve_src(dir, c.src.as_deref().unwrap_or_default());
            if !src.is_file() {
                return Err(format!("missing overlay: {}", src.display()));
            }
            let end = c.end.filter(|e| *e > c.start).ok_or_else(|| {
                format!(
                    "overlay clip {} needs an end after its start",
                    src.display()
                )
            })?;
            if src
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("webm"))
            {
                args.push("-c:v".into());
                args.push("libvpx-vp9".into());
            }
            args.push("-i".into());
            args.push(src.to_string_lossy().into_owned());
            filters.push(overlay_filter(c, input, &picture, end, (fw, fh)));
            picture = format!("[v{input}]");
            input += 1;
        }
    }
    let sidecars = caption_sidecars(dir, stem, &t, &output, (fw, fh))?;
    if let Some((ass, _)) = sidecars.first() {
        let name = ass.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        filters.push(format!("{picture}subtitles={}[vsub]", filter_name(name)));
        picture = "[vsub]".to_string();
    }
    if mix.is_empty() && picture == "[v0]" {
        return Err("nothing to export: the timeline has no audio or overlay clips".into());
    }
    let audio_map = if mix.is_empty() {
        None
    } else {
        filters.push(format!(
            "{}amix=inputs={}:normalize=0[mix];[mix]apad[a]",
            mix.concat(),
            mix.len()
        ));
        Some("[a]")
    };
    args.push("-filter_complex".into());
    args.push(filters.join(";"));
    args.extend(["-map", &picture, "-c:v", "libx264", "-pix_fmt", "yuv420p"].map(String::from));
    match audio_map {
        Some(map) => args.extend(["-map", map, "-c:a", "aac"].map(String::from)),
        None if simple => args.extend(["-map", "0:a?", "-c:a", "aac"].map(String::from)),
        None => args.push("-an".into()),
    }
    args.push("-shortest".into());
    let output = format!("{EXPORT_DIR}/{output}");
    args.push(dir.join(&output).to_string_lossy().into_owned());
    Ok((args, output, sidecars))
}

/// Render `<stem>.timeline.json` with ffmpeg into the project's `export/` directory and
/// return the output file name. Deterministic: same JSON, same command. When the
/// timeline has captions the sidecars are written first (the burn-in ASS next to
/// the timeline, the SRT next to the output) and the ffmpeg must have libass.
/// ffmpeg runs with the project directory as cwd because the subtitles filter
/// names the ASS file bare.
#[tauri::command]
pub(crate) async fn export_timeline(project: String, name: String) -> Result<String, String> {
    let dir = dir_for(&project)?;
    let path = timeline_path(&dir, &name)?;
    let stem = name.trim_end_matches(SUFFIX).to_string();
    let json =
        std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let (args, output, sidecars) = export_args(&dir, &stem, &json)?;
    let export_dir = dir.join(EXPORT_DIR);
    std::fs::create_dir_all(&export_dir)
        .map_err(|e| format!("creating {}: {e}", export_dir.display()))?;
    let ffmpeg = video_ffmpeg()?;
    if !sidecars.is_empty() && !has_filters(&ffmpeg, &[" subtitles "]) {
        return Err("no ffmpeg with libass found: captions cannot be burned in; install a full ffmpeg (brew install ffmpeg)".into());
    }
    for (path, body) in &sidecars {
        std::fs::write(path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new(ffmpeg)
            .args(&args)
            .current_dir(&dir)
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
        let (args, output, _) = export_args(&dir, "demo", json).unwrap();
        assert_eq!(output, "export/demo.with-voice.mp4");
        let joined = args.join(" ");
        assert!(joined.contains("[1]atrim=start=2:end=5,asetpts=PTS-STARTPTS,adelay=1500|1500[a1];[2]adelay=0|0,volume=0.2[a2];[0:a][a1][a2]amix=inputs=3:normalize=0[mix];[mix]apad[a]"), "{joined}");
        assert!(
            joined.ends_with(
                &dir.join("export/demo.with-voice.mp4")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert!(joined.contains(
            "[0:v]scale=w=1920:h=1080:force_original_aspect_ratio=decrease,pad=w=1920:h=1080:x=(ow-iw)/2:y=(oh-ih)/2,setsar=1[v0]"
        ), "{joined}");
        assert!(
            joined.contains("-map [v0] -c:v libx264 -pix_fmt yuv420p -map [a] -c:a aac -shortest")
        );

        let muted = json.replace("\"keep\"", "\"mute\"");
        let (args, _, _) = export_args(&dir, "demo", &muted).unwrap();
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
    fn export_args_composites_every_overlay_card() {
        let dir = std::env::temp_dir().join(format!("infer-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("title.webm"), b"x").unwrap();
        std::fs::write(dir.join("logo.mov"), b"x").unwrap();
        let json = r#"{"resolution":"1080x1920","tracks":[
            {"kind":"video","clips":[{"start":0,"src":"demo.mov"}]},
            {"kind":"overlay","clips":[
                {"start":1,"end":4,"src":"title.webm"},
                {"start":6,"end":9,"src":"logo.mov","x":0.1,"y":0.8,"width":0.5}]}]}"#;
        let (args, _, _) = export_args(&dir, "demo", json).unwrap();
        let joined = args.join(" ");
        assert!(joined.contains("-c:v libvpx-vp9 -i "), "{joined}");
        assert!(joined.contains("logo.mov -filter_complex "), "{joined}");
        assert!(
            joined.contains(
                "[1:v]setpts=PTS-STARTPTS+1/TB,scale=w=1080:h=-2[o1];[v0][o1]overlay=x=W*0:y=H*0:eof_action=pass:enable='between(t,1,4)'[v1];\
                 [2:v]setpts=PTS-STARTPTS+6/TB,scale=w=540:h=-2[o2];[v1][o2]overlay=x=W*0.1:y=H*0.8:eof_action=pass:enable='between(t,6,9)'[v2]"
            ),
            "{joined}"
        );
        assert!(
            joined.contains("-map [v2] -c:v libx264 -pix_fmt yuv420p -map 0:a? -c:a aac -shortest"),
            "{joined}"
        );
        assert!(export_args(&dir, "demo", &json.replace(r#""end":4,"#, "")).is_err());
        assert!(export_args(&dir, "demo", &json.replace("1080x1920", "1080")).is_err());
        assert_eq!(parse_resolution(None).unwrap(), (1920, 1080));
        assert!(parse_resolution(Some("1921x1080")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_args_burns_captions_to_ass_and_srt() {
        let dir = std::env::temp_dir().join(format!("infer-captions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.wav"), b"x").unwrap();
        let json = r#"{"resolution":"1080x1920","tracks":[
            {"kind":"video","clips":[{"start":0,"src":"demo.mov"}]},
            {"kind":"captions","style":"highlight","position":"top","clips":[
                {"start":1,"end":4,"text":"hello world","words":[{"start":1,"end":2.5},{"start":2.5,"end":4}]},
                {"start":5,"end":7,"text":"second line"}]}]}"#;
        let (args, output, sidecars) = export_args(&dir, "demo", json).unwrap();
        assert_eq!(output, "export/demo.with-voice.mp4");
        assert_eq!(sidecars[0].0, dir.join("demo.ass"));
        assert_eq!(sidecars[1].0, dir.join("export/demo.with-voice.srt"));
        let joined = args.join(" ");
        assert!(joined.contains("[v0]subtitles=demo.ass[vsub]"), "{joined}");
        let ass = &sidecars[0].1;
        assert!(ass.contains("PlayResX: 1080\nPlayResY: 1920"), "{ass}");
        assert!(ass.contains(
            "Style: Highlight,Arial,134,&H0088FF2B,&H00FFFFFF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,9,0,8,60,60,115,1"
        ), "{ass}");
        assert!(ass.contains(
            "Dialogue: 0,0:00:01.00,0:00:04.00,Highlight,,0,0,0,,{\\an8}{\\k150}hello {\\k150}world"
        ), "{ass}");
        assert!(
            ass.contains("Dialogue: 0,0:00:05.00,0:00:07.00,Highlight,,0,0,0,,{\\an8}second line"),
            "{ass}"
        );
        let srt = &sidecars[1].1;
        assert!(srt.contains(
            "1\n00:00:01,000 --> 00:00:04,000\nhello world\n\n2\n00:00:05,000 --> 00:00:07,000\nsecond line\n"
        ), "{srt}");

        let (_, _, sidecars) =
            export_args(&dir, "demo", &json.replace("\"highlight\"", "\"weird\"")).unwrap();
        let ass = &sidecars[0].1;
        assert!(ass.contains("Style: Classic,"), "{ass}");
        assert!(
            ass.contains("Dialogue: 0,0:00:01.00,0:00:04.00,Classic,,0,0,0,,{\\an8}hello world"),
            "{ass}"
        );
        let (_, _, sidecars) =
            export_args(&dir, "demo", &json.replace("\"highlight\"", "\"bold\"")).unwrap();
        assert!(
            sidecars[0]
                .1
                .contains("Dialogue: 0,0:00:01.00,0:00:04.00,Bold,,0,0,0,,{\\an8}HELLO WORLD"),
            "{}",
            sidecars[0].1
        );
        let gapped = json.replace(
            r#""words":[{"start":1,"end":2.5},{"start":2.5,"end":4}]"#,
            r#""words":[{"start":1.5,"end":2.5},{"start":3,"end":4}]"#,
        );
        let (_, _, sidecars) = export_args(&dir, "demo", &gapped).unwrap();
        assert!(
            sidecars[0].1.contains(
                "Dialogue: 0,0:00:01.00,0:00:04.00,Highlight,,0,0,0,,{\\an8}{\\k50}{\\k100}hello{\\k50} {\\k100}world"
            ),
            "{}",
            sidecars[0].1
        );

        let placed = json.replace(
            r#""position":"top""#,
            r#""position":"top","x":0.25,"y":0.8"#,
        );
        let (_, _, sidecars) = export_args(&dir, "demo", &placed).unwrap();
        assert!(
            sidecars[0].1.contains(
                "Dialogue: 0,0:00:05.00,0:00:07.00,Highlight,,0,0,0,,{\\an5\\pos(270,1536)}second line"
            ),
            "{}",
            sidecars[0].1
        );

        let nocap = r#"{"tracks":[
            {"kind":"video","clips":[{"start":0,"src":"demo.mov"}]},
            {"kind":"audio","clips":[{"start":1,"end":4,"src":"s1.wav"}]}]}"#;
        let (args, _, sidecars) = export_args(&dir, "demo", nocap).unwrap();
        assert!(sidecars.is_empty());
        assert!(!args.join(" ").contains("subtitles"), "{}", args.join(" "));
        assert!(export_args(&dir, "demo", &json.replace(r#""end":4,"#, "")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_args_renders_the_video_track_as_a_trimmed_sequence() {
        let dir = std::env::temp_dir().join(format!("infer-seq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.mp4"), b"x").unwrap();
        std::fs::write(dir.join("b.mp4"), b"x").unwrap();
        let json = r#"{"source_audio":"keep","tracks":[
            {"kind":"video","clips":[
                {"start":2,"end":7,"offset":3,"src":"a.mp4"},
                {"start":7,"src":"b.mp4"}]}]}"#;
        let (args, _, _) = export_args(&dir, "demo", json).unwrap();
        let joined = args.join(" ");
        assert!(joined.contains("a.mp4 -i "), "{joined}");
        assert!(joined.contains("b.mp4 -filter_complex "), "{joined}");
        assert!(joined.contains(
            "[0:v]trim=start=3:end=8,setpts=PTS-STARTPTS,scale=w=1920:h=1080:force_original_aspect_ratio=decrease,pad=w=1920:h=1080:x=(ow-iw)/2:y=(oh-ih)/2,setsar=1,format=yuv420p,tpad=start_duration=2[vs0]"
        ), "{joined}");
        assert!(
            joined.contains("trim=start=0,setpts=PTS-STARTPTS"),
            "{joined}"
        );
        assert!(joined.contains("tpad=start_duration=0[vs1]"), "{joined}");
        assert!(
            joined.contains("[vs0][vs1]concat=n=2:v=1:a=0[vcat]"),
            "{joined}"
        );
        assert!(
            joined.contains("[0:a]atrim=start=3:end=8,asetpts=PTS-STARTPTS,adelay=2000|2000[av0]"),
            "{joined}"
        );
        assert!(joined.contains("[1:a]adelay=7000|7000[av1]"), "{joined}");
        assert!(
            joined.contains("[av0][av1]amix=inputs=2:normalize=0[mix];[mix]apad[a]"),
            "{joined}"
        );
        assert!(
            joined
                .contains("-map [vcat] -c:v libx264 -pix_fmt yuv420p -map [a] -c:a aac -shortest"),
            "{joined}"
        );

        let muted = json.replace("\"keep\"", "\"mute\"");
        let (args, _, _) = export_args(&dir, "demo", &muted).unwrap();
        assert!(
            args.join(" ")
                .contains("-map [vcat] -c:v libx264 -pix_fmt yuv420p -an -shortest")
        );

        assert!(export_args(&dir, "demo", &json.replace("\"start\":2,", "\"start\":8,")).is_err());
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
