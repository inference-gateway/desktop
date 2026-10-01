use crate::binaries::{self, owned_bin};
use crate::download::ProgressEvent;
use crate::env::{home_dir, mock_mode};
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::ipc::Channel;

// --- Speech-to-text (desktop-owned whisper.cpp) ---
// The desktop owns its own STT: it resolves a whisper.cpp binary (the CLI-managed
// copy, see binaries.rs, then the CLI's whisper-cli/whisper-cpp candidates on
// PATH) and downloads the GGML model from HuggingFace into ~/.infer/models/whisper.
// Audio is captured in the WebView and handed here as WAV bytes; whisper turns it
// into text. No ffmpeg, no CGO.

pub(crate) const WHISPER_MODEL_FILE: &str = "ggml-tiny.bin";
pub(crate) const WHISPER_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";

pub(crate) fn whisper_model_path() -> PathBuf {
    home_dir()
        .join(".infer")
        .join("models")
        .join("whisper")
        .join(WHISPER_MODEL_FILE)
}

pub(crate) fn is_executable_file(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// First matching executable named `name` on PATH.
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(&crate::env::composed_path()) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{}.exe", name));
            if is_executable_file(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

/// Resolve the whisper binary: WHISPER_BIN override, then the CLI-managed copy in
/// ~/.infer/bin/tools, then whisper-cli/whisper-cpp on PATH. `None` drives the
/// greyed-out mic in the UI.
pub(crate) fn whisper_bin_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("WHISPER_BIN") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(p) = owned_bin("whisper-cli") {
        return Some(p);
    }
    ["whisper-cli", "whisper-cpp"]
        .iter()
        .find_map(|name| find_on_path(name))
}

/// Stream `reader` into `tmp`, reporting (received, total) progress. Leaves `tmp`
/// on error for the caller to remove.
pub(crate) fn stream_to_file(
    mut reader: impl Read,
    tmp: &std::path::Path,
    total: u64,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let mut file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 8192];
    let mut received = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        received += n as u64;
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        on_progress(received, total);
    }
    Ok(())
}

/// Ensure the GGML model exists, downloading it once (atomic temp -> rename).
/// Not checksum-verified: HuggingFace's resolve/ layout has no per-file digest,
/// matching the CLI.
pub(crate) fn ensure_whisper_model(on_progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let dest = whisper_model_path();
    if dest.exists() {
        return Ok(dest);
    }
    let dir = dest.parent().ok_or("bad model path")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.partial", WHISPER_MODEL_FILE));
    let resp = ureq::get(WHISPER_MODEL_URL)
        .call()
        .map_err(|e| format!("Failed to download model: {}", e))?;
    let total: u64 = resp
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let reader = resp.into_body().into_reader();
    if let Err(e) = stream_to_file(reader, &tmp, total, on_progress) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Strip whisper's bracketed non-speech markers ([BLANK_AUDIO], (music), ...)
/// and collapse whitespace. A no-speech clip therefore yields "".
pub(crate) fn clean_transcript(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut sq = 0i32;
    let mut rp = 0i32;
    for c in raw.chars() {
        match c {
            '[' => sq += 1,
            ']' if sq > 0 => sq -= 1,
            '(' => rp += 1,
            ')' if rp > 0 => rp -= 1,
            _ if sq == 0 && rp == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn next_tmp_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub(crate) fn run_whisper(
    bin: &std::path::Path,
    model: &std::path::Path,
    wav: &std::path::Path,
) -> Result<String, String> {
    let output = std::process::Command::new(bin)
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .arg("-nt")
        .arg("-np")
        .output()
        .map_err(|e| format!("Failed to run whisper: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "whisper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(clean_transcript(&String::from_utf8_lossy(&output.stdout)))
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct SttStatus {
    binary: bool,
    model: bool,
    downloadable: bool,
    hint: String,
}

/// Whether STT is usable and, if not, why - so the frontend can grey the mic and
/// show the right tooltip.
#[tauri::command]
pub(crate) async fn stt_status() -> Result<SttStatus, String> {
    if mock_mode() {
        return Ok(SttStatus {
            binary: true,
            model: true,
            downloadable: false,
            hint: String::new(),
        });
    }
    let binary = whisper_bin_path().is_some();
    let downloadable = !binary
        && tokio::task::spawn_blocking(|| binaries::has_prebuilt("whisper-cli"))
            .await
            .unwrap_or(true);
    let hint = if !binary && !downloadable {
        "No prebuilt voice tools for this platform - install whisper-cli or whisper-cpp on PATH (or set WHISPER_BIN) to enable voice input".into()
    } else {
        String::new()
    };
    Ok(SttStatus {
        binary,
        model: whisper_model_path().exists(),
        downloadable,
        hint,
    })
}

/// Ensure the whisper binary (installed by the CLI where a prebuilt exists) and
/// model are present, streaming progress through the same channel as the CLI install UI.
#[tauri::command]
pub(crate) async fn prepare_stt(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if mock_mode() {
        let _ = on_event.send(ProgressEvent::Ready);
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        let _ = on_event.send(ProgressEvent::Checking);
        if whisper_bin_path().is_none() {
            binaries::ensure(&["whisper-cli"], &on_event)?;
        }
        ensure_whisper_model(|received, total| {
            let _ = on_event.send(ProgressEvent::Downloading { received, total });
        })?;
        let _ = on_event.send(ProgressEvent::Ready);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Transcribe WAV bytes (16kHz mono, produced by the WebView) to text.
#[tauri::command]
pub(crate) async fn transcribe_audio(wav: Vec<u8>) -> Result<String, String> {
    if mock_mode() {
        return Ok("this is a mock transcription".into());
    }
    tokio::task::spawn_blocking(move || {
        let bin = whisper_bin_path().ok_or("whisper-cli not found")?;
        let model = ensure_whisper_model(|_, _| {})?;
        let tmp = std::env::temp_dir().join(format!(
            "infer-stt-{}-{}.wav",
            std::process::id(),
            next_tmp_id()
        ));
        std::fs::write(&tmp, &wav).map_err(|e| e.to_string())?;
        let result = run_whisper(&bin, &model, &tmp);
        let _ = std::fs::remove_file(&tmp);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_transcript_strips_markers() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO] "), "");
        assert_eq!(clean_transcript("(music) hello  world\n"), "hello world");
        assert_eq!(
            clean_transcript("  Create a file  named test.txt  "),
            "Create a file named test.txt"
        );
        assert_eq!(clean_transcript("hi (typing) there [noise]"), "hi there");
    }
}
