use crate::agent::{infer_command, run_infer};
use crate::env::home_dir;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use tauri::ipc::Channel;

/// A model repository as the Hugging Face search lists it.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HubModel {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) downloads: u64,
    #[serde(default)]
    pub(crate) likes: u64,
    #[serde(default)]
    pub(crate) pipeline_tag: String,
}

/// One file of a repository, with its size in bytes.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct HubFile {
    pub(crate) path: String,
    pub(crate) size: u64,
}

/// A repository downloaded to this machine: where it is and how much of it.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DownloadedModel {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

/// Reject a value the CLI would parse as a flag. The CLI checks the rest.
fn positional(what: &str, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return Err(format!("invalid {what} {value:?}"));
    }
    Ok(value.to_string())
}

fn listing<T: serde::de::DeserializeOwned>(dump: &str) -> Result<Vec<T>, String> {
    serde_json::from_str(dump).map_err(|e| format!("unexpected infer models output: {e}"))
}

/// The CLI owns the search, the downloads and the library in
/// ~/.infer/models/huggingface, so every command here shells out to
/// `infer models`.
#[tauri::command]
pub(crate) async fn search_hub_models(query: String) -> Result<Vec<HubModel>, String> {
    let query = positional("search", &query)?;
    let dump = run_infer(&["models", "search", "--format", "json", &query]).await?;
    listing(&dump)
}

#[tauri::command]
pub(crate) async fn list_hub_model_files(id: String) -> Result<Vec<HubFile>, String> {
    let id = positional("model", &id)?;
    let dump = run_infer(&["models", "files", "--format", "json", &id]).await?;
    listing(&dump)
}

#[tauri::command]
pub(crate) async fn list_downloaded_models() -> Result<Vec<DownloadedModel>, String> {
    let dump = run_infer(&["models", "list", "--format", "json"]).await?;
    listing(&dump)
}

#[tauri::command]
pub(crate) async fn delete_downloaded_model(id: String) -> Result<(), String> {
    let id = positional("model", &id)?;
    run_infer(&["models", "delete", &id]).await?;
    Ok(())
}

/// Download a model, or only the files named, forwarding each progress line.
#[tauri::command]
pub(crate) async fn download_hub_model(
    id: String,
    files: Vec<String>,
    on_line: Channel<String>,
) -> Result<(), String> {
    let id = positional("model", &id)?;
    let files = files
        .iter()
        .map(|file| positional("file", file))
        .collect::<Result<Vec<_>, _>>()?;
    tauri::async_runtime::spawn_blocking(move || download(&id, &files, &on_line))
        .await
        .map_err(|e| format!("model download task failed: {e}"))?
}

/// Run `infer models download`, forwarding each stdout line as progress.
/// shortcut: stderr is read after stdout closes, fine for the CLI's one error line, a reader thread if it ever streams more than a pipe buffer.
fn download(id: &str, files: &[String], on_line: &Channel<String>) -> Result<(), String> {
    let mut child = infer_command(&home_dir())
        .args(["models", "download", id])
        .args(files)
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
            "infer models download {id} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listings_parse_what_the_cli_prints() {
        let found: Vec<HubModel> = listing(
            r#"[{"id":"mlx-community/MuseTalk-1.5-fp16","downloads":1362,"likes":6,"pipeline_tag":"image-to-image"},{"id":"acme/untagged"}]"#,
        )
        .unwrap();
        assert_eq!(found[0].pipeline_tag, "image-to-image");
        assert_eq!(found[1].downloads, 0);

        let files: Vec<HubFile> =
            listing(r#"[{"path":"v15/unet.safetensors","size":1699963904}]"#).unwrap();
        assert_eq!(files[0].size, 1_699_963_904);

        let library: Vec<DownloadedModel> = listing(
            r#"[{"id":"acme/talker","path":"/home/u/.infer/models/huggingface/acme/talker","files":2,"bytes":9}]"#,
        )
        .unwrap();
        assert_eq!(library[0].files, 2);
        assert!(listing::<HubModel>("not json").is_err());
    }

    #[test]
    fn a_value_that_reads_as_a_flag_is_refused() {
        assert_eq!(
            positional("model", " acme/talker "),
            Ok("acme/talker".into())
        );
        assert!(positional("model", "").is_err());
        assert!(positional("search", "--help").is_err());
        assert!(positional("file", "-o").is_err());
    }
}
