use super::infer::gateway_url;
use crate::env::mock_mode;
use std::io::Read;

#[tauri::command]
pub(crate) async fn list_models() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(fetch_models)
        .await
        .map_err(|e| format!("model list task failed: {e}"))?
}

/// The gateway asks every configured provider for its models, so the listing
/// gets a generous ceiling - it only has to stop a hung gateway pinning a thread.
const MODELS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn fetch_models() -> Result<Vec<String>, String> {
    if mock_mode() {
        return mock_models();
    }
    let url = format!(
        "{}/v1/models?include=modalities",
        gateway_url().trim_end_matches('/')
    );
    let resp = ureq::get(&url)
        .config()
        .timeout_global(Some(MODELS_TIMEOUT))
        .build()
        .call()
        .map_err(|e| format!("Gateway unreachable at {}: {}", url, e))?;
    let mut text = String::new();
    resp.into_body()
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    chat_model_ids(&text)
}

const TEXT_MODALITY: &str = "text";

/// Whether a listed model can hold a chat: it takes text and answers in text.
/// A model the gateway knows no modalities of stays in, since a local model is
/// usually one of those.
fn chats(model: &serde_json::Value) -> bool {
    let Some(modalities) = model.get("modalities").filter(|m| !m.is_null()) else {
        return true;
    };
    let includes_text = |side: &str| {
        modalities
            .get(side)
            .and_then(|listed| listed.as_array())
            .is_some_and(|listed| listed.iter().any(|m| m == TEXT_MODALITY))
    };
    includes_text("input") && includes_text("output")
}

/// The ids of the chat models in a `/v1/models?include=modalities` listing.
/// Speech, music, image and video models are served by the same gateway and
/// cannot run an agent, so they stay out of every model picker.
fn chat_model_ids(listing: &str) -> Result<Vec<String>, String> {
    let body: serde_json::Value = serde_json::from_str(listing).map_err(|e| e.to_string())?;
    let models = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("Unexpected /v1/models response shape")?
        .iter()
        .filter(|m| chats(m))
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    Ok(models)
}

/// Mock mode's one source of truth for the model list: the `models:` block
/// of the same scenarios file the infer children serve turns from
/// (`INFER_GATEWAY_MOCK_SCENARIOS`, set by the e2e driver or manual mock
/// dev). No scenarios file configured means no models to select.
pub(crate) fn mock_models() -> Result<Vec<String>, String> {
    let path = std::env::var("INFER_GATEWAY_MOCK_SCENARIOS").map_err(|_| {
        "mock mode needs INFER_GATEWAY_MOCK_SCENARIOS pointing at a scenarios.yaml".to_string()
    })?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("reading {path}: {e}"))?;
    parse_model_ids(&text)
}

/// The tokenless `models:` block of a scenarios file as bare model ids.
fn parse_model_ids(yaml: &str) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct ScenarioDefs {
        #[serde(default)]
        models: Vec<ModelDef>,
    }
    #[derive(serde::Deserialize)]
    struct ModelDef {
        id: String,
    }
    let defs: ScenarioDefs =
        serde_norway::from_str(yaml).map_err(|e| format!("parsing scenarios: {e}"))?;
    Ok(defs.models.into_iter().map(|m| m.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_models_that_take_and_answer_in_text_are_chat_models() {
        let listing = r#"{"object":"list","data":[
            {"id":"openai/gpt-4o","modalities":{"input":["text","image"],"output":["text"]}},
            {"id":"elevenlabs/eleven_v4","modalities":{"input":["text"],"output":["audio"]}},
            {"id":"elevenlabs/creatify-aurora","modalities":{"input":["image","audio"],"output":["video"]}},
            {"id":"elevenlabs/gpt-image-2","modalities":{"input":["text","image"],"output":["image"]}},
            {"id":"google/gemini-native-audio","modalities":{"input":["text","audio"],"output":["text","audio"]}},
            {"id":"ollama/my-finetune"},
            {"id":"llamacpp/local","modalities":null}
        ]}"#;
        assert_eq!(
            chat_model_ids(listing).unwrap(),
            vec![
                "openai/gpt-4o".to_string(),
                "google/gemini-native-audio".to_string(),
                "ollama/my-finetune".to_string(),
                "llamacpp/local".to_string(),
            ]
        );
        assert!(chat_model_ids(r#"{"error":"down"}"#).is_err());
        assert!(chat_model_ids("not json").is_err());
    }

    #[test]
    fn mock_models_parse_the_scenarios_models_block() {
        let yaml = "models:\n  - id: openai/gpt-4o\n    object: model\n  - id: anthropic/claude-sonnet-4-5\n    object: model\nfallback:\n  content: Done.\nscenarios: []\n";
        assert_eq!(
            parse_model_ids(yaml).unwrap(),
            vec![
                "openai/gpt-4o".to_string(),
                "anthropic/claude-sonnet-4-5".to_string()
            ]
        );
        assert_eq!(
            parse_model_ids("scenarios: []\n").unwrap(),
            Vec::<String>::new()
        );
    }
}
