use crate::env::{home_dir, uploads_dir};
use std::path::{Component, Path, PathBuf};

/// Whether `path` lies in one of the CLI's runtime dirs `names`, either
/// userspace (`~/.infer/<name>`) or per project (`~/.infer/projects/<slug>/<name>`).
/// The CLI writes media and artifacts to the project's dirs while one is open.
fn in_infer_runtime_dir(path: &str, names: &[&str]) -> bool {
    let project_rest = path
        .split_once("/.infer/projects/")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, rest)| rest);
    names.iter().any(|name| {
        path.contains(&format!("/.infer/{name}/"))
            || project_rest.is_some_and(|rest| rest.starts_with(&format!("{name}/")))
    })
}

/// Validate a generated-image path before copying it out: absolute, under
/// `home`, inside an `.infer` artifacts or tmp dir, no `..` traversal. Split
/// from `save_image` so the guard is testable without touching the filesystem.
pub(crate) fn safe_image_source(path: &str, home: &Path) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let ok = p.starts_with(home)
        && !p.components().any(|c| c == Component::ParentDir)
        && in_infer_runtime_dir(path, &["artifacts", "tmp"]);
    if !ok {
        return Err(format!(
            "Refusing to save image outside .infer/artifacts or .infer/tmp: {path}"
        ));
    }
    Ok(p.to_path_buf())
}

/// Copy a generated image (the absolute path `infer` reported) to the user's
/// Downloads folder. Returns the destination path on success.
#[tauri::command]
pub(crate) fn save_image(path: String) -> Result<String, String> {
    let home = home_dir();
    let src = safe_image_source(&path, &home)?;
    copy_to_downloads(&src, &home)
}

/// Copy a generated WAV (transcript speech or a voice sample) to the user's
/// Downloads folder. Same guard shape as `safe_image_source`, scoped to the
/// TTS output and samples-library directories.
#[tauri::command]
pub(crate) fn save_audio(path: String) -> Result<String, String> {
    let home = home_dir();
    let src = safe_audio_source(&path, &home)?;
    copy_to_downloads(&src, &home)
}

pub(crate) fn safe_audio_source(path: &str, home: &Path) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let ok = p.starts_with(home)
        && !p.components().any(|c| c == Component::ParentDir)
        && (in_infer_runtime_dir(path, &["tmp/media/tts", "tmp/tts"])
            || path.contains("/.infer/models/tts/samples/"))
        && path.to_ascii_lowercase().ends_with(".wav");
    if !ok {
        return Err(format!(
            "Refusing to save audio outside the .infer tts or models/tts/samples dirs: {path}"
        ));
    }
    Ok(p.to_path_buf())
}

fn copy_to_downloads(src: &Path, home: &Path) -> Result<String, String> {
    if !src.exists() {
        return Err(format!("File not found: {}", src.display()));
    }
    let name = src.file_name().ok_or("invalid file path")?;
    let downloads = home.join("Downloads");
    std::fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let dest = downloads.join(name);
    std::fs::copy(src, &dest).map_err(|e| e.to_string())?;
    Ok(dest.to_string_lossy().to_string())
}

/// Paths from the `[Attached image: <path>]` lines the composer prepends to a
/// prompt, limited to formats the CLI can decode so they travel as image
/// content parts of the run's user message.
pub(crate) fn attached_image_paths(prompt: &str) -> Vec<String> {
    const DECODABLE: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
    prompt
        .lines()
        .filter_map(|l| l.strip_prefix("[Attached image: ")?.strip_suffix(']'))
        .filter(|p| {
            std::path::Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| DECODABLE.contains(&e.to_ascii_lowercase().as_str()))
        })
        .map(str::to_owned)
        .collect()
}

#[tauri::command]
pub(crate) fn save_upload(data: String, mime: String) -> Result<String, String> {
    let ext = match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        _ => return Err(format!("Unsupported file type: {mime}")),
    };

    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid upload data: {e}"))?;

    const MAX_BYTES: usize = 25 * 1024 * 1024;
    if bytes.len() > MAX_BYTES {
        return Err(format!("File too large: {} bytes (max 25 MB)", bytes.len()));
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let name = format!("{nanos:x}.{ext}");
    let dest = uploads_dir().join(&name);
    std::fs::write(&dest, &bytes).map_err(|e| format!("Failed to save upload: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

// --- A2A Agent configuration ---
// A2A agents live in the `infer` CLI's own config (~/.infer/agents.yaml). Rather
// than reimplement the CLI's agent knowledge - it auto-fills oci/run/model/env
// for known agents (browser-agent, documentation-agent, ...) and distinguishes
// local containers from external URL agents - the desktop shells out to
// `infer agents add|remove|list` so the two stay perfectly in sync. Keyed by
// name (the CLI's primary key).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_guards_accept_userspace_and_per_project_runtime_dirs() {
        let home = Path::new("/Users/x");
        for ok in [
            "/Users/x/.infer/tmp/media/screenshots/computer-1.jpeg",
            "/Users/x/.infer/projects/-Users-x-proj/tmp/media/screenshots/computer-1.jpeg",
            "/Users/x/.infer/projects/-Users-x-proj/artifacts/sid-1/image-1.png",
            "/Users/x/.infer/artifacts/sid-1/image-1.png",
        ] {
            assert!(safe_image_source(ok, home).is_ok(), "{ok}");
        }
        for bad in [
            "/Users/x/.infer/uploads/a.png",
            "/Users/x/.infer/projects/-Users-x-proj/history/a.png",
            "/Users/x/.infer/projects/../tmp/a.png",
            "/etc/.infer/tmp/a.png",
        ] {
            assert!(safe_image_source(bad, home).is_err(), "{bad}");
        }
        for ok in [
            "/Users/x/.infer/tmp/media/tts/speech-1.wav",
            "/Users/x/.infer/projects/-Users-x-proj/tmp/media/tts/speech-1.wav",
            "/Users/x/.infer/models/tts/samples/me.wav",
        ] {
            assert!(safe_audio_source(ok, home).is_ok(), "{ok}");
        }
        for bad in [
            "/Users/x/.infer/tts/speech-1.wav",
            "/Users/x/.infer/projects/-Users-x-proj/tmp/media/music/song.mp3",
            "/Users/x/.infer/projects/-Users-x-proj/conversations/a.wav",
            "/Users/x/.infer/tmp/out.wav",
            "/Users/x/.infer/tmp/media/voice/infer-voice-1.wav",
        ] {
            assert!(safe_audio_source(bad, home).is_err(), "{bad}");
        }
    }

    #[test]
    fn attached_image_paths_picks_decodable_marker_lines_only() {
        let prompt = "[Attached image: /u/.infer/tmp/uploads/a.jpg]\n[Attached image: /u/.infer/tmp/uploads/b.heic]\n[Attached image: /u/x.PNG]\n\nwhat is this? [Attached image: /u/inline.png]";
        assert_eq!(
            attached_image_paths(prompt),
            vec!["/u/.infer/tmp/uploads/a.jpg", "/u/x.PNG"]
        );
        assert!(attached_image_paths("no images").is_empty());
        assert!(
            attached_image_paths("[Attached video: /u/.infer/tmp/uploads/c.mp4]\n[Attached audio: /u/.infer/tmp/uploads/d.mp3]")
                    .is_empty(),
            "video/audio markers must never become image parts"
        );
    }
}
