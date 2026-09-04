use crate::download::ProgressEvent;
use crate::download::{download, find_checksum, sha256_digest};
use crate::env::{home_dir, mock_mode};
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::ipc::Channel;

// --- Speech-to-text (desktop-owned whisper.cpp) ---
// The desktop owns its own STT: it resolves a whisper.cpp binary (PATH first,
// like the CLI's whisper-cli/whisper-cpp candidates; download fallback on the
// platforms that have a prebuilt asset) and downloads the GGML model from
// HuggingFace into ~/.infer/models/whisper. Audio is captured in the WebView and
// handed here as WAV bytes; whisper turns it into text. No ffmpeg, no CGO.

pub(crate) const WHISPER_MODEL_FILE: &str = "ggml-tiny.bin";
pub(crate) const WHISPER_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
/// Prebuilt static ffmpeg / whisper-cli / llama-tts release shared with the CLI.
pub(crate) const BINARIES_BASE: &str =
    "https://github.com/inference-gateway/binaries/releases/download/v0.3.0";
/// Desktop-owned tools (ffmpeg, whisper-cli) live in ~/.infer/tools, apart
/// from the CLI's and gateway's own downloads in ~/.infer/bin.
pub(crate) fn tools_dir() -> PathBuf {
    home_dir().join(".infer").join("tools")
}

pub(crate) fn whisper_model_path() -> PathBuf {
    home_dir()
        .join(".infer")
        .join("models")
        .join("whisper")
        .join(WHISPER_MODEL_FILE)
}

/// Prebuilt whisper-cli asset for this platform, or `None` where none is published.
pub(crate) fn stt_bin_asset() -> Option<String> {
    bin_asset("whisper-cli")
}

/// Release asset name (`<name>-<os>-<arch>`) for this platform.
pub(crate) fn bin_asset(name: &str) -> Option<String> {
    bin_asset_for(name, std::env::consts::OS, std::env::consts::ARCH)
}

pub(crate) fn bin_asset_for(name: &str, os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    match os {
        "linux" => Some(format!("{name}-linux-{arch}")),
        "macos" => Some(format!("{name}-darwin-{arch}")),
        "windows" if arch == "amd64" => Some(format!("{name}-windows-amd64.exe")),
        _ => None,
    }
}

/// Installed file name for a downloaded binary; Windows needs the `.exe` to run.
pub(crate) fn bin_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The desktop-owned copy of a tool in ~/.infer/tools, if installed.
pub(crate) fn owned_bin(name: &str) -> Option<PathBuf> {
    let owned = tools_dir().join(bin_file_name(name));
    is_executable_file(&owned).then_some(owned)
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

/// Resolve the whisper binary: WHISPER_BIN override, then whisper-cli/whisper-cpp
/// on PATH, then the desktop-downloaded copy in ~/.infer/bin. `None` drives the
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

/// Download and checksum-verify the prebuilt whisper-cli into ~/.infer/bin.
pub(crate) fn download_whisper_binary(
    on_event: &Channel<ProgressEvent>,
) -> Result<PathBuf, String> {
    download_binary("whisper-cli", on_event)
}

/// Download and checksum-verify a prebuilt binary from the binaries release
/// into ~/.infer/tools. Copies check_and_install_cli's verify-then-rename-then-chmod flow.
pub(crate) fn download_binary(
    name: &str,
    on_event: &Channel<ProgressEvent>,
) -> Result<PathBuf, String> {
    let asset = bin_asset(name).ok_or_else(|| {
        format!(
            "No prebuilt {name} for {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let bin_dir = tools_dir();
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    let dest = bin_dir.join(bin_file_name(name));
    let tmp = bin_dir.join(format!("{name}.tmp"));

    let _ = on_event.send(ProgressEvent::Downloading {
        received: 0,
        total: 0,
    });
    download(&format!("{}/{}", BINARIES_BASE, asset), &tmp, on_event)?;

    let _ = on_event.send(ProgressEvent::Verifying);
    let checksums_resp = ureq::get(&format!("{}/checksums.txt", BINARIES_BASE))
        .call()
        .map_err(|e| format!("Failed to download checksums.txt: {}", e))?;
    let mut checksums_text = String::new();
    checksums_resp
        .into_body()
        .into_reader()
        .read_to_string(&mut checksums_text)
        .map_err(|e| e.to_string())?;

    let expected = find_checksum(&checksums_text, &asset)
        .ok_or_else(|| format!("Checksum not found for {}", asset))?;
    let actual = sha256_digest(&tmp)?;
    if actual != expected {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "Checksum mismatch for {}: expected {}, got {}",
            asset, expected, actual
        ));
    }

    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&dest)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&dest, perms).map_err(|e| e.to_string())?;
    }
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
    let downloadable = stt_bin_asset().is_some();
    let hint = if !binary && !downloadable {
        "Voice input isn't available on this platform".into()
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

/// Ensure the whisper binary (download where a prebuilt exists) and model are
/// present, streaming progress through the same channel as the CLI install UI.
#[tauri::command]
pub(crate) async fn prepare_stt(on_event: Channel<ProgressEvent>) -> Result<(), String> {
    if mock_mode() {
        let _ = on_event.send(ProgressEvent::Ready);
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        let _ = on_event.send(ProgressEvent::Checking);
        if whisper_bin_path().is_none() {
            let _ = on_event.send(ProgressEvent::Installing);
            download_whisper_binary(&on_event)?;
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

    /// Real network: downloads ffmpeg and whisper-cli from the binaries
    /// release into DL_TEST_HOME (default: a temp dir) and verifies the
    /// checksum flow end to end.
    /// Run with: DL_TEST_HOME=$HOME cargo test download_binaries_install_from_release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn download_binaries_install_from_release() {
        let temp = std::env::temp_dir().join(format!("infer-dl-{}", std::process::id()));
        let home = std::env::var("DL_TEST_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| temp.clone());
        std::fs::create_dir_all(&home).unwrap();
        unsafe { std::env::set_var("HOME", &home) };
        let ch = Channel::new(|_| Ok(()));
        for name in ["ffmpeg", "whisper-cli"] {
            let dest = download_binary(name, &ch).unwrap();
            assert_eq!(dest, home.join(".infer").join("tools").join(name));
            assert!(is_executable_file(&dest));
            assert_eq!(owned_bin(name).as_deref(), Some(dest.as_path()));
            let out = std::process::Command::new(&dest).arg("-version").output();
            let out = out
                .or_else(|_| std::process::Command::new(&dest).arg("--help").output())
                .unwrap();
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                text.contains("version") || text.contains("usage"),
                "{name} did not run: {text}"
            );
            println!("installed {}", dest.display());
        }
        if home == temp {
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn test_stt_bin_asset_mapping() {
        assert_eq!(
            bin_asset_for("whisper-cli", "linux", "x86_64").as_deref(),
            Some("whisper-cli-linux-amd64")
        );
        assert_eq!(
            bin_asset_for("whisper-cli", "linux", "aarch64").as_deref(),
            Some("whisper-cli-linux-arm64")
        );
        assert_eq!(
            bin_asset_for("whisper-cli", "macos", "aarch64").as_deref(),
            Some("whisper-cli-darwin-arm64")
        );
        assert_eq!(
            bin_asset_for("whisper-cli", "macos", "x86_64").as_deref(),
            Some("whisper-cli-darwin-amd64")
        );
        assert_eq!(
            bin_asset_for("whisper-cli", "windows", "x86_64").as_deref(),
            Some("whisper-cli-windows-amd64.exe")
        );
        assert_eq!(bin_asset_for("whisper-cli", "windows", "aarch64"), None);
        assert_eq!(bin_asset_for("whisper-cli", "linux", "riscv64"), None);
    }
}
