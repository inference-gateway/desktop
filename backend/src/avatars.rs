// Avatar library for the CLI's TextToVideo tool, stored by the CLI in
// ~/.infer/avatars/<name>/ as one or more portraits of the same person. The
// desktop owns none of it: create, list and delete shell out to
// `infer avatars` (cli cmd/avatars), which also generates the extra angles
// through the gateway's image edit API.
use crate::agent::{infer_command, run_infer};
use crate::env::{agent_cwd, home_dir};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::ipc::Channel;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Avatar {
    pub(crate) name: String,
    pub(crate) images: Vec<String>,
}

fn avatars_dir() -> PathBuf {
    home_dir().join(".infer").join("avatars")
}

/// Avatars from an `infer avatars list --format json` dump, with each image
/// name resolved to its absolute path under `dir` so the webview can show it.
fn avatars_from(dump: &str, dir: &Path) -> Result<Vec<Avatar>, String> {
    let listed: Vec<Avatar> = serde_json::from_str(dump).map_err(|e| e.to_string())?;
    Ok(listed
        .into_iter()
        .map(|a| Avatar {
            images: a
                .images
                .iter()
                .map(|img| dir.join(&a.name).join(img).display().to_string())
                .collect(),
            name: a.name,
        })
        .collect())
}

/// Reject names the CLI would parse as a flag; it enforces the rest itself.
fn avatar_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.starts_with('-') {
        return Err(format!("invalid avatar name {name:?}"));
    }
    Ok(name)
}

#[tauri::command]
pub(crate) async fn list_avatars() -> Result<Vec<Avatar>, String> {
    let dump = run_infer(&["avatars", "list", "--format", "json"]).await?;
    avatars_from(&dump, &avatars_dir())
}

#[tauri::command]
pub(crate) async fn delete_avatar(name: String) -> Result<(), String> {
    run_infer(&["avatars", "delete", avatar_name(&name)?]).await?;
    Ok(())
}

/// Create an avatar from a picked photo. Returns false when the user cancels
/// the dialog, which runs Rust-side off the main thread like
/// `add_voice_sample`.
#[tauri::command]
pub(crate) async fn import_avatar(
    app: tauri::AppHandle,
    name: String,
    on_line: Channel<String>,
) -> Result<bool, String> {
    avatar_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("Photo", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file();
        let Some(photo) = picked.and_then(|fp| fp.into_path().ok()) else {
            return Ok(false);
        };
        create(&name, &photo, &on_line).map(|()| true)
    })
    .await
    .map_err(|e| format!("avatar task failed: {e}"))?
}

/// Create an avatar from a camera snapshot taken in the webview. The JPEG is
/// staged in a temp file for `--from` and removed whatever the outcome.
#[tauri::command]
pub(crate) async fn snapshot_avatar(
    name: String,
    jpeg: Vec<u8>,
    on_line: Channel<String>,
) -> Result<(), String> {
    avatar_name(&name)?;
    tauri::async_runtime::spawn_blocking(move || {
        let photo = std::env::temp_dir().join(format!("infer-avatar-{}.jpg", std::process::id()));
        std::fs::write(&photo, jpeg).map_err(|e| format!("writing {}: {e}", photo.display()))?;
        let created = create(&name, &photo, &on_line);
        let _ = std::fs::remove_file(&photo);
        created
    })
    .await
    .map_err(|e| format!("avatar task failed: {e}"))?
}

/// Run `infer avatars create`, forwarding each stdout line as progress.
/// ponytail: stderr is read after stdout closes - fine for the CLI's one
/// error line, a reader thread if it ever streams more than a pipe buffer.
fn create(name: &str, photo: &Path, on_line: &Channel<String>) -> Result<(), String> {
    let mut child = infer_command(&agent_cwd())
        .args(["avatars", "create", name, "--from"])
        .arg(photo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run infer: {e}"))?;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = on_line.send(line);
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "infer avatars create {name} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatars_from_resolves_images_under_the_library() {
        let dump =
            r#"[{"name":"presenter","images":["01-front.jpeg","02-three-quarter-left.png"]}]"#;
        let dir = Path::new("/home/u/.infer/avatars");
        assert_eq!(
            avatars_from(dump, dir).unwrap(),
            vec![Avatar {
                name: "presenter".into(),
                images: vec![
                    "/home/u/.infer/avatars/presenter/01-front.jpeg".into(),
                    "/home/u/.infer/avatars/presenter/02-three-quarter-left.png".into(),
                ],
            }]
        );
        assert_eq!(avatars_from("[]", dir).unwrap(), vec![]);
        assert!(avatars_from("not json", dir).is_err());
    }

    #[test]
    fn avatar_name_rejects_empty_and_flag_like_names() {
        assert_eq!(avatar_name("presenter"), Ok("presenter"));
        assert!(avatar_name("").is_err());
        assert!(avatar_name("--help").is_err());
    }
}
