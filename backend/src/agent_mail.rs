//! AgentMail (agentmail.to) is how a Markdown agent becomes reachable by mail:
//! an inbox address for its identity, the API key that owns it, and a send
//! allowlist keeping the agent's mail inside approved addresses.

use crate::config;
use crate::env::{home_dir, mock_mode};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The hosted MCP server a thread with an agent inbox gets, authenticated by
/// the agent's API key in the `x-api-key` header.
pub(crate) const MCP_SERVER_URL: &str = "https://mcp.agentmail.to/mcp";

/// The name the thread's worker knows the AgentMail MCP server by.
pub(crate) const MCP_SERVER_NAME: &str = "agentmail";

const API_BASE: &str = "https://api.agentmail.to";

/// AgentMail answers are small JSON objects; the cap keeps a broken server
/// from growing the buffer without bound.
const ANSWER_MAX_BYTES: u64 = 1 << 20;

/// Provisions `<name>@agentmail.to` (display name = agent name) and answers
/// the address the inbox got.
fn create_inbox(name: &str, api_key: &str) -> Result<String, String> {
    let resp = ureq::post(format!("{API_BASE}/v0/inboxes"))
        .header("x-api-key", api_key)
        .send_json(json!({ "username": name, "display_name": name }))
        .map_err(|e| format!("creating the AgentMail inbox: {e}"))?;
    let bytes = resp
        .into_body()
        .with_config()
        .limit(ANSWER_MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("reading the AgentMail answer: {e}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("parsing the AgentMail answer: {e}"))?;
    let address = value["email"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| value["inbox_id"].as_str())
        .unwrap_or_default();
    if address.is_empty() {
        return Err("AgentMail answered without an inbox address".into());
    }
    Ok(address.to_owned())
}

fn list_allow(api_key: &str) -> Result<Vec<String>, String> {
    let resp = ureq::get(format!("{API_BASE}/v0/lists/send/allow"))
        .header("x-api-key", api_key)
        .call()
        .map_err(|e| format!("reading the AgentMail send allowlist: {e}"))?;
    let bytes = resp
        .into_body()
        .with_config()
        .limit(ANSWER_MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("reading the AgentMail answer: {e}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("parsing the AgentMail answer: {e}"))?;
    Ok(value["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e["entry"].as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default())
}

fn add_allow(api_key: &str, entry: &str) -> Result<(), String> {
    ureq::post(format!("{API_BASE}/v0/lists/send/allow"))
        .header("x-api-key", api_key)
        .send_json(json!({ "entry": entry }))
        .map_err(|e| format!("adding {entry} to the AgentMail send allowlist: {e}"))?;
    Ok(())
}

fn remove_allow(api_key: &str, entry: &str) -> Result<(), String> {
    ureq::delete(format!(
        "{API_BASE}/v0/lists/send/allow/{}",
        escape_segment(entry)
    ))
    .header("x-api-key", api_key)
    .call()
    .map_err(|e| format!("removing {entry} from the AgentMail send allowlist: {e}"))?;
    Ok(())
}

/// Percent-escapes an allowlist entry for its place in a URL path, so a
/// crafted entry cannot reach past its segment.
fn escape_segment(entry: &str) -> String {
    let mut out = String::with_capacity(entry.len());
    for byte in entry.as_bytes() {
        match byte {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The allowlist as lowercase entries, so the diff matches case-insensitively.
fn allow_set(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// Reconciles AgentMail's send allowlist with `entries`: addresses the file
/// adds go up, ones no longer listed come down.
pub(crate) fn sync_send_allowlist(api_key: &str, entries: &[String]) -> Result<(), String> {
    let wanted = allow_set(entries);
    let current = allow_set(&list_allow(api_key)?);
    for entry in wanted.difference(&current) {
        add_allow(api_key, entry)?;
    }
    for entry in current.difference(&wanted) {
        remove_allow(api_key, entry)?;
    }
    Ok(())
}

/// The API key that lets a chat with `agent` mail as its saved address: the
/// agent's .md needs an `email:` and auth.yaml its key.
pub(crate) fn thread_key(agent: Option<&str>) -> Option<String> {
    thread_key_in(&crate::markdown_agents::agent_dirs(), &home_dir(), agent)
}

/// `thread_key` rooted at explicit directories and home, for tests.
pub(crate) fn thread_key_in(dirs: &[PathBuf], home: &Path, agent: Option<&str>) -> Option<String> {
    let name = agent?.trim();
    if name.is_empty() {
        return None;
    }
    let _ = crate::markdown_agents::by_name_in(dirs, name)
        .filter(|agent| !agent.email.trim().is_empty())?;
    let key = config::agent_api_key_in(home, name);
    (!key.is_empty()).then_some(key)
}

/// Provisions `<name>@agentmail.to` and answers the address the inbox got.
/// Mock mode answers the address without touching AgentMail.
#[tauri::command]
pub(crate) async fn create_agent_inbox(name: String, api_key: String) -> Result<String, String> {
    if !crate::markdown_agents::valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    if api_key.trim().is_empty() {
        return Err("an AgentMail API key is needed to create an inbox".into());
    }
    if mock_mode() {
        return Ok(format!("{name}@agentmail.to"));
    }
    let (name, api_key) = (name, api_key.trim().to_owned());
    tokio::task::spawn_blocking(move || create_inbox(&name, &api_key))
        .await
        .map_err(|e| format!("inbox task failed: {e}"))?
}

/// Saves the agent's AgentMail API key into auth.yaml, beside the provider
/// keys and never into the agent's .md.
#[tauri::command]
pub(crate) fn set_agent_email_key(name: String, api_key: String) -> Result<(), String> {
    if !crate::markdown_agents::valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    config::set_agent_api_key_in(&home_dir(), &name, &api_key)
}

/// Moves the AgentMail API key to the agent's new name, on a rename. A missing
/// key is not an error.
#[tauri::command]
pub(crate) fn move_agent_email_key(from: String, to: String) -> Result<(), String> {
    for name in [&from, &to] {
        if !crate::markdown_agents::valid_agent_name(name) {
            return Err(format!("invalid agent name: {name}"));
        }
    }
    move_agent_email_key_in(&home_dir(), &from, &to)
}

/// `move_agent_email_key` rooted at an arbitrary home, for tests.
pub(crate) fn move_agent_email_key_in(home: &Path, from: &str, to: &str) -> Result<(), String> {
    let key = config::agent_api_key_in(home, from);
    if key.is_empty() {
        return Ok(());
    }
    config::set_agent_api_key_in(home, to, &key)?;
    config::set_agent_api_key_in(home, from, "")
}

/// Reconciles AgentMail's send allowlist with the agent's whitelist entries.
#[tauri::command]
pub(crate) async fn sync_agent_send_allowlist(
    name: String,
    entries: Vec<String>,
) -> Result<(), String> {
    if !crate::markdown_agents::valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    let api_key = config::agent_api_key(&name);
    if api_key.is_empty() {
        return Err(format!(
            "agent {name} has no AgentMail API key to sync its send allowlist with"
        ));
    }
    if mock_mode() {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || sync_send_allowlist(&api_key, &entries))
        .await
        .map_err(|e| format!("allowlist task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-mail-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A home whose agent has an address on file, read straight from the file
    /// to prove what the split stores where.
    fn seed_agent(home: &Path) {
        let dir = home.join(".infer").join("agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("code-reviewer.md"),
            "---\nname: code-reviewer\ndescription: Reviews code\nemail: code-reviewer@agentmail.to\n---\nReview the diff.\n",
        )
        .unwrap();
    }

    fn auth_of(home: &Path) -> BTreeSet<(String, String)> {
        let text = std::fs::read_to_string(home.join(".infer").join("auth.yaml")).unwrap();
        serde_norway::from_str::<std::collections::BTreeMap<String, String>>(&text)
            .unwrap()
            .into_iter()
            .collect()
    }

    fn agent_dirs_of(home: &Path) -> Vec<PathBuf> {
        vec![home.join(".infer").join("agents")]
    }

    #[cfg(unix)]
    #[test]
    fn the_address_travels_in_the_agent_file_and_the_key_only_in_auth_yaml() {
        let home = temp_home("split");
        seed_agent(&home);
        config::set_agent_api_key_in(&home, "code-reviewer", " am_us_secret ").unwrap();

        let text = std::fs::read_to_string(home.join(".infer/agents/code-reviewer.md")).unwrap();
        assert_eq!(
            text.lines()
                .filter(|l| *l == "email: code-reviewer@agentmail.to")
                .count(),
            1
        );
        assert!(!text.contains("am_us_secret"), "{text}");

        assert_eq!(
            auth_of(&home),
            [(
                "AGENTMAIL_API_KEY_code-reviewer".to_owned(),
                "am_us_secret".to_owned()
            )]
            .into_iter()
            .collect()
        );
        assert_eq!(
            config::agent_api_key_in(&home, "code-reviewer"),
            "am_us_secret"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_key_replaces_and_a_blank_one_clears_leaving_the_rest() {
        let home = temp_home("replace");
        std::fs::create_dir_all(home.join(".infer")).unwrap();
        std::fs::write(
            home.join(".infer/auth.yaml"),
            "OPENAI_API_KEY: sk-provider\n",
        )
        .unwrap();

        config::set_agent_api_key_in(&home, "code-reviewer", "am_us_one").unwrap();
        assert_eq!(
            config::agent_api_key_in(&home, "code-reviewer"),
            "am_us_one"
        );
        config::set_agent_api_key_in(&home, "code-reviewer", "am_us_two").unwrap();
        assert_eq!(
            config::agent_api_key_in(&home, "code-reviewer"),
            "am_us_two"
        );

        config::set_agent_api_key_in(&home, "code-reviewer", "").unwrap();
        assert!(config::agent_api_key_in(&home, "code-reviewer").is_empty());
        assert_eq!(
            auth_of(&home),
            [("OPENAI_API_KEY".to_owned(), "sk-provider".to_owned())]
                .into_iter()
                .collect()
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_move_renames_the_key_and_a_move_without_one_is_a_no_op() {
        let home = temp_home("move");
        config::set_agent_api_key_in(&home, "old-name", "am_us_secret").unwrap();
        move_agent_email_key_in(&home, "old-name", "new-name").unwrap();
        assert_eq!(config::agent_api_key_in(&home, "new-name"), "am_us_secret");
        assert!(config::agent_api_key_in(&home, "old-name").is_empty());

        move_agent_email_key_in(&home, "ghost", "other").unwrap();
        assert!(config::agent_api_key_in(&home, "other").is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_thread_key_needs_both_an_address_and_a_key() {
        let home = temp_home("thread");
        seed_agent(&home);
        let dirs = agent_dirs_of(&home);
        assert!(thread_key_in(&dirs, &home, Some("code-reviewer")).is_none());
        config::set_agent_api_key_in(&home, "code-reviewer", "am_us_secret").unwrap();
        assert_eq!(
            thread_key_in(&dirs, &home, Some("code-reviewer")),
            Some("am_us_secret".to_owned())
        );
        assert_eq!(thread_key_in(&dirs, &home, Some("other")), None);
        assert_eq!(thread_key_in(&dirs, &home, None), None);
        assert_eq!(thread_key_in(&dirs, &home, Some(" ")), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_escape_keeps_the_entry_inside_its_url_segment() {
        assert_eq!(escape_segment("a@b.example"), "a%40b.example");
        assert_eq!(escape_segment("a/b"), "a%2Fb");
        assert_eq!(escape_segment("a..b-2_3~x"), "a..b-2_3~x");
    }

    #[test]
    fn allow_sets_are_lowercased_and_blankless() {
        let got = allow_set(&["A@Example.com".into(), " ".into(), "a@example.com".into()]);
        assert_eq!(got, ["a@example.com".to_owned()].into_iter().collect());
    }
}
