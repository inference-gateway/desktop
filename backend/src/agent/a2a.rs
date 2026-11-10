use crate::env::{agent_cwd, home_dir, infer_bin_path};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct A2aAgent {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) run: bool,
    pub(crate) model: String,
}

// `infer agents list --format json` fields are PascalCase (Go struct names),
// split into `local` (container) and `external` (URL-only) groups.
#[derive(serde::Deserialize)]
pub(crate) struct CliAgent {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "Run")]
    run: bool,
    #[serde(rename = "Model", default)]
    model: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct CliAgentList {
    local: Option<Vec<CliAgent>>,
    external: Option<Vec<CliAgent>>,
}

pub(crate) fn parse_agent_list(json: &[u8]) -> Result<Vec<A2aAgent>, String> {
    let parsed: CliAgentList = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    Ok([parsed.local, parsed.external]
        .into_iter()
        .flatten()
        .flatten()
        .map(|a| A2aAgent {
            name: a.name,
            url: a.url,
            run: a.run,
            model: a.model,
        })
        .collect())
}

pub(crate) async fn run_infer_agents(args: &[&str]) -> Result<std::process::Output, String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        crate::env::command(infer_bin_path())
            .arg("agents")
            .args(&args)
            .arg("--no-colors")
            .env("HOME", home_dir().to_str().unwrap_or(""))
            .current_dir(agent_cwd())
            .output()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("infer agents task failed: {}", e))?
}

pub(crate) fn agents_command_ok(out: std::process::Output) -> Result<(), String> {
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[tauri::command]
pub(crate) async fn list_a2a_agents() -> Result<Vec<A2aAgent>, String> {
    let out = run_infer_agents(&["list", "--format", "json"]).await?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_agent_list(&out.stdout)
}

#[tauri::command]
pub(crate) async fn add_a2a_agent(name: String, url: String) -> Result<(), String> {
    let mut args = vec!["add", name.as_str()];
    let url = url.trim();
    if !url.is_empty() {
        args.push(url);
    }
    agents_command_ok(run_infer_agents(&args).await?)
}

#[tauri::command]
pub(crate) async fn remove_a2a_agent(name: String) -> Result<(), String> {
    agents_command_ok(run_infer_agents(&["remove", name.as_str()]).await?)
}

#[tauri::command]
pub(crate) async fn set_a2a_agent_model(name: String, model: String) -> Result<(), String> {
    agents_command_ok(
        run_infer_agents(&["update", name.as_str(), "--model", model.as_str()]).await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_list_merges_local_and_external() {
        let json = br#"{"external":[{"Name":"code-reviewer","URL":"https://a.example.com","Run":false}],"local":[{"Name":"browser-agent","URL":"http://localhost:8083","OCI":"x","Run":true,"Model":"deepseek/deepseek-v4-flash"}],"total":2}"#;
        let agents = parse_agent_list(json).unwrap();
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "browser-agent");
        assert_eq!(agents[0].url, "http://localhost:8083");
        assert!(agents[0].run);
        assert_eq!(agents[0].model, "deepseek/deepseek-v4-flash");
        assert_eq!(agents[1].name, "code-reviewer");
        assert!(!agents[1].run);
        assert_eq!(agents[1].model, "");
    }

    #[test]
    fn test_parse_agent_list_handles_null_groups() {
        assert!(
            parse_agent_list(br#"{"external":null,"local":null,"total":0}"#)
                .unwrap()
                .is_empty()
        );
    }
}
