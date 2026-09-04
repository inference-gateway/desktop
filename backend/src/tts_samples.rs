// Voice-sample library for TTS voice cloning, stored flat in
// ~/.infer/models/tts/samples/ (next to the CLI's TTS model cache
// ~/.infer/models/tts/). Upload is a native file picker + std::fs::copy; the
// CLI's TextToSpeech tool resolves `voice_sample` names here (once its
// samples-library fallback ships - see cli internal/agent/tools/text_to_speech.go).
use crate::env::home_dir;
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::DialogExt;

/// The samples directory (created on first use).
pub(crate) fn samples_dir() -> PathBuf {
    let dir = home_dir()
        .join(".infer")
        .join("models")
        .join("tts")
        .join("samples");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct VoiceSample {
    pub(crate) name: String,
    pub(crate) path: String,
}

/// Non-recursive listing of the samples directory, .wav files only.
#[tauri::command]
pub(crate) async fn list_voice_samples() -> Result<Vec<VoiceSample>, String> {
    list_wavs(&samples_dir())
}

fn list_wavs(dir: &Path) -> Result<Vec<VoiceSample>, String> {
    let mut samples: Vec<VoiceSample> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("wav"))
        })
        .map(|e| VoiceSample {
            name: e.file_name().to_string_lossy().into_owned(),
            path: e.path().display().to_string(),
        })
        .collect();
    samples.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(samples)
}

/// Confine a sample file name to the samples directory: bare names only.
fn sample_path(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(format!(
            "invalid sample name {name:?}: bare file names only"
        ));
    }
    Ok(dir.join(name))
}

/// Copy a picked WAV into the samples library under its base name, overwriting
/// an existing sample of the same name. Returns `None` when the user cancels
/// the dialog. The dialog runs off the main thread via spawn_blocking (the
/// blocking API deadlocks there), Rust-side so no capability entry is needed.
#[tauri::command]
pub(crate) async fn add_voice_sample(app: tauri::AppHandle) -> Result<Option<VoiceSample>, String> {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("WAV audio", &["wav"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("file dialog failed: {e}"))?;
    let Some(src) = picked.and_then(file_path_buf) else {
        return Ok(None);
    };
    copy_sample(&src, &samples_dir())
}

/// Same FilePath conversion as export.rs (dialog may return a URL variant).
fn file_path_buf(fp: tauri_plugin_dialog::FilePath) -> Option<PathBuf> {
    match fp {
        tauri_plugin_dialog::FilePath::Path(p) => Some(p),
        _ => None,
    }
}

/// Validate + copy one picked file into `dir` under its base name.
fn copy_sample(src: &Path, dir: &Path) -> Result<Option<VoiceSample>, String> {
    let is_wav = src
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return Err(format!(
            "voice samples must be .wav files: {}",
            src.display()
        ));
    }
    let Some(name) = src.file_name().and_then(|n| n.to_str()) else {
        return Err(format!("invalid sample file name: {}", src.display()));
    };
    let dest = sample_path(dir, name)?;
    std::fs::copy(src, &dest).map_err(|e| format!("copying {}: {e}", src.display()))?;
    Ok(Some(VoiceSample {
        name: name.to_string(),
        path: dest.display().to_string(),
    }))
}

/// Persist a WAV recorded in the webview under `name`, overwriting an
/// existing sample of the same name.
#[tauri::command]
pub(crate) async fn save_voice_sample(name: String, wav: Vec<u8>) -> Result<VoiceSample, String> {
    write_sample(&samples_dir(), &name, &wav)
}

fn write_sample(dir: &Path, name: &str, wav: &[u8]) -> Result<VoiceSample, String> {
    if !name.to_ascii_lowercase().ends_with(".wav") {
        return Err(format!("voice samples must be .wav files: {name:?}"));
    }
    let dest = sample_path(dir, name)?;
    std::fs::write(&dest, wav).map_err(|e| format!("writing {}: {e}", dest.display()))?;
    Ok(VoiceSample {
        name: name.to_string(),
        path: dest.display().to_string(),
    })
}

/// Remove one sample by bare file name.
#[tauri::command]
pub(crate) async fn delete_voice_sample(name: String) -> Result<(), String> {
    let path = sample_path(&samples_dir(), &name)?;
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("infer-tts-samples-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sample_path_rejects_traversal_and_accepts_bare_names() {
        let dir = Path::new("/tmp/samples");
        for bad in ["", ".", "..", "a/b", "a\\b", "../x", "dir/name.wav"] {
            assert!(sample_path(dir, bad).is_err(), "{bad:?} should be rejected");
        }
        assert_eq!(sample_path(dir, "me.wav").unwrap(), dir.join("me.wav"));
    }

    #[test]
    fn list_wavs_returns_only_wav_files_sorted() {
        let dir = temp_dir("list");
        std::fs::write(dir.join("b.wav"), b"x").unwrap();
        std::fs::write(dir.join("a.WAV"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(dir.join("dir.wav")).unwrap();
        let names: Vec<String> = list_wavs(&dir)
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["a.WAV", "b.wav"]);
    }

    #[test]
    fn write_sample_writes_wav_and_rejects_bad_names() {
        let dir = temp_dir("write");
        let saved = write_sample(&dir, "rec.wav", b"RIFF").unwrap();
        assert_eq!(saved.name, "rec.wav");
        assert_eq!(std::fs::read(dir.join("rec.wav")).unwrap(), b"RIFF");

        write_sample(&dir, "rec.WAV", b"RIFF2").unwrap();
        assert!(write_sample(&dir, "rec.txt", b"x").is_err());
        assert!(write_sample(&dir, "../rec.wav", b"x").is_err());
    }

    #[test]
    fn copy_sample_copies_wav_under_base_name_and_rejects_other_extensions() {
        let src_dir = temp_dir("src");
        let samples = temp_dir("dst");
        let src = src_dir.join("voice.wav");
        std::fs::write(&src, b"RIFF").unwrap();

        let added = copy_sample(&src, &samples).unwrap().unwrap();
        assert_eq!(added.name, "voice.wav");
        assert_eq!(std::fs::read(samples.join("voice.wav")).unwrap(), b"RIFF");

        // Re-adding the same base name overwrites in place.
        std::fs::write(&src, b"RIFF2").unwrap();
        copy_sample(&src, &samples).unwrap();
        assert_eq!(std::fs::read(samples.join("voice.wav")).unwrap(), b"RIFF2");

        let txt = src_dir.join("voice.txt");
        std::fs::write(&txt, b"nope").unwrap();
        assert!(copy_sample(&txt, &samples).is_err());
    }
}
