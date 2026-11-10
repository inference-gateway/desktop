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
    let url = format!("{}/v1/models", gateway_url().trim_end_matches('/'));
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
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let models = body
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("Unexpected /v1/models response shape")?
        .iter()
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
