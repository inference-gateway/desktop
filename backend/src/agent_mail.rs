use crate::config;
use crate::env::{home_dir, mock_mode};
use crate::markdown_agents::{self, MarkdownAgent};
use crate::thread::ThreadOptions;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_BASE: &str = "https://api.agentmail.to";

/// How long one AgentMail call may take before the editor or thread gives up.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct Inbox {
    email: String,
}

#[derive(Deserialize)]
struct ListPage {
    entries: Vec<ListEntry>,
    #[serde(default)]
    next_page_token: Option<String>,
}

/// One entry of an inbox's send allowlist, spelled the way AgentMail stores it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ListEntry {
    entry: String,
    #[serde(default)]
    read_only: bool,
}

/// The AgentMail REST API as the owner of an API key, every call bounded by
/// `REQUEST_TIMEOUT`.
struct Client {
    http: ureq::Agent,
    authorization: String,
}

impl Client {
    fn new(api_key: &str) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        Self {
            http: ureq::Agent::new_with_config(config),
            authorization: format!("Bearer {}", api_key.trim()),
        }
    }

    /// Provisions `<name>@agentmail.to` (display name = agent name) and
    /// answers the address the inbox got.
    fn create_inbox(&self, name: &str) -> Result<String, String> {
        let inbox: Inbox = self
            .http
            .post(format!("{API_BASE}/v0/inboxes"))
            .header("Authorization", &self.authorization)
            .send_json(json!({ "username": name, "display_name": name }))
            .and_then(|mut resp| resp.body_mut().read_json())
            .map_err(|e| format!("creating the AgentMail inbox: {e}"))?;
        Ok(inbox.email)
    }

    /// Every entry of `inbox`'s send allowlist, across all pages.
    fn allowlist(&self, inbox: &str) -> Result<Vec<ListEntry>, String> {
        let mut entries = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut request = self
                .http
                .get(allowlist_url(inbox))
                .header("Authorization", &self.authorization);
            if let Some(token) = &page_token {
                request = request.query("page_token", token);
            }
            let page: ListPage = request
                .call()
                .and_then(|mut resp| resp.body_mut().read_json())
                .map_err(|e| format!("reading the send allowlist of {inbox}: {e}"))?;
            entries.extend(page.entries);
            page_token = page.next_page_token.filter(|token| !token.is_empty());
            if page_token.is_none() {
                return Ok(entries);
            }
        }
    }

    fn allow(&self, inbox: &str, entry: &str) -> Result<(), String> {
        self.http
            .post(allowlist_url(inbox))
            .header("Authorization", &self.authorization)
            .send_json(json!({ "entry": entry }))
            .map_err(|e| format!("adding {entry} to the send allowlist of {inbox}: {e}"))?;
        Ok(())
    }

    fn disallow(&self, inbox: &str, entry: &str) -> Result<(), String> {
        self.http
            .delete(format!(
                "{}/{}",
                allowlist_url(inbox),
                escape_segment(entry)
            ))
            .header("Authorization", &self.authorization)
            .call()
            .map_err(|e| format!("removing {entry} from the send allowlist of {inbox}: {e}"))?;
        Ok(())
    }
}

/// The send allowlist of one inbox, never the organization's, so agents
/// sharing an owner key keep separate lists.
fn allowlist_url(inbox: &str) -> String {
    format!(
        "{API_BASE}/v0/inboxes/{}/lists/send/allow",
        escape_segment(inbox)
    )
}

/// Percent-escapes a value for its place in a URL path, so a crafted inbox or
/// entry cannot reach past its segment.
fn escape_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn normalized(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// True when `cap` lists `entry` itself or, for an address, its domain.
fn cap_allows(cap: &BTreeSet<String>, entry: &str) -> bool {
    cap.contains(entry)
        || entry
            .rsplit_once('@')
            .is_some_and(|(_, domain)| cap.contains(domain))
}

/// The recipients an agent may mail: its own entries the config `cap` allows
/// (an empty cap allows all). Answers the entries the cap dropped beside them.
/// ponytail: an empty AgentMail allow list sends to anyone, so an empty result
/// falls back to the agent's own address - the inbox can then only mail itself.
fn effective_allowlist(
    entries: &[String],
    cap: &[String],
    own_address: &str,
) -> (BTreeSet<String>, Vec<String>) {
    let cap = normalized(cap);
    let (allowed, dropped): (BTreeSet<String>, BTreeSet<String>) = normalized(entries)
        .into_iter()
        .partition(|entry| cap.is_empty() || cap_allows(&cap, entry));
    let allowed = if allowed.is_empty() {
        BTreeSet::from([own_address.trim().to_ascii_lowercase()])
    } else {
        allowed
    };
    (allowed, dropped.into_iter().collect())
}

/// What turns `current` into `wanted`: entries to add, and the server's own
/// spelling of the entries to remove. Read-only entries cannot be removed.
fn plan_sync(current: &[ListEntry], wanted: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    let spelled = |entry: &ListEntry| entry.entry.trim().to_ascii_lowercase();
    let present: BTreeSet<String> = current.iter().map(spelled).collect();
    let adds = wanted.difference(&present).cloned().collect();
    let removes = current
        .iter()
        .filter(|entry| !entry.read_only && !wanted.contains(&spelled(entry)))
        .map(|entry| entry.entry.clone())
        .collect();
    (adds, removes)
}

/// Reconciles `agent`'s inbox allowlist with its effective allowlist and
/// answers the entries the cap dropped. Adds go first so the list never
/// passes through empty, which AgentMail reads as allow all.
fn sync_allowlist(
    client: &Client,
    agent: &MarkdownAgent,
    cap: &[String],
) -> Result<Vec<String>, String> {
    let inbox = agent.email.trim();
    let (wanted, dropped) = effective_allowlist(&agent.allowed_recipients, cap, inbox);
    let (adds, removes) = plan_sync(&client.allowlist(inbox)?, &wanted);
    for entry in &adds {
        client.allow(inbox, entry)?;
    }
    for entry in &removes {
        client.disallow(inbox, entry)?;
    }
    Ok(dropped)
}

/// The agent `name` when it has an inbox address, from the first agent
/// directory holding it.
fn mail_agent_in(dirs: &[PathBuf], name: &str) -> Option<MarkdownAgent> {
    if !markdown_agents::valid_agent_name(name) {
        return None;
    }
    markdown_agents::by_name_in(dirs, name).filter(|agent| !agent.email.trim().is_empty())
}

/// Re-syncs the inbox allowlist of the agent a thread runs as from its agent
/// file, so a hand edit is enforced before the agent mails. An agent without an
/// inbox or owner key is left alone, and a failed sync never blocks the open.
pub(crate) fn resync_allowlist(options: &ThreadOptions) {
    let Some(name) = options.agent.as_deref() else {
        return;
    };
    let (dirs, home) = (markdown_agents::agent_dirs(), home_dir());
    if mail_agent_in(&dirs, name).is_none() || config::agent_api_key_in(&home, name).is_empty() {
        return;
    }
    if let Err(e) = sync_agent_mail_in(&dirs, &home, name) {
        eprintln!("agent {name} opens with its allowlist unsynced: {e}");
    }
}

/// `sync_agent_mail` rooted at explicit agent directories and home.
fn sync_agent_mail_in(dirs: &[PathBuf], home: &Path, name: &str) -> Result<Vec<String>, String> {
    let agent =
        mail_agent_in(dirs, name).ok_or_else(|| format!("agent {name} has no inbox address"))?;
    let owner_key = config::agent_api_key_in(home, name);
    if owner_key.is_empty() {
        return Err(format!("agent {name} has no AgentMail API key"));
    }
    let cap = config::agent_mail_cap_in(home);
    if mock_mode() {
        return Ok(effective_allowlist(&agent.allowed_recipients, &cap, &agent.email).1);
    }
    sync_allowlist(&Client::new(&owner_key), &agent, &cap)
}

/// Provisions `<name>@agentmail.to` and answers the address the inbox got.
/// Mock mode answers the address without touching AgentMail.
#[tauri::command]
pub(crate) async fn create_agent_inbox(name: String, api_key: String) -> Result<String, String> {
    if !markdown_agents::valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    if api_key.trim().is_empty() {
        return Err("an AgentMail API key is needed to create an inbox".into());
    }
    if mock_mode() {
        return Ok(format!("{name}@agentmail.to"));
    }
    tokio::task::spawn_blocking(move || Client::new(&api_key).create_inbox(&name))
        .await
        .map_err(|e| format!("inbox task failed: {e}"))?
}

/// Saves the agent's AgentMail API key into auth.yaml, beside the provider
/// keys and never into the agent's .md.
#[tauri::command]
pub(crate) fn set_agent_email_key(name: String, api_key: String) -> Result<(), String> {
    if !markdown_agents::valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    config::set_agent_api_key_in(&home_dir(), &name, &api_key)
}

/// Moves the AgentMail API key to the agent's new name, on a rename. A missing
/// key is not an error.
#[tauri::command]
pub(crate) fn move_agent_email_key(from: String, to: String) -> Result<(), String> {
    for name in [&from, &to] {
        if !markdown_agents::valid_agent_name(name) {
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

/// Pushes agent `name`'s allowed recipients to its inbox's AgentMail allowlist.
/// Answers the entries `agent_mail.allowed_recipients` in config.yaml dropped.
#[tauri::command]
pub(crate) async fn sync_agent_mail(name: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        sync_agent_mail_in(&markdown_agents::agent_dirs(), &home_dir(), &name)
    })
    .await
    .map_err(|e| format!("allowlist task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::infer_dir_in;
    use crate::test_support::TempDir;

    /// A home whose agent has an address on file, read straight from the file
    /// to prove what the split stores where.
    fn seed_agent(home: &Path) {
        let dir = infer_dir_in(home).join("agents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("code-reviewer.md"),
            "---\nname: code-reviewer\ndescription: Reviews code\nemail: code-reviewer@agentmail.to\n---\nReview the diff.\n",
        )
        .unwrap();
    }

    fn auth_of(home: &Path) -> BTreeSet<(String, String)> {
        let text = std::fs::read_to_string(infer_dir_in(home).join("auth.yaml")).unwrap();
        serde_norway::from_str::<std::collections::BTreeMap<String, String>>(&text)
            .unwrap()
            .into_iter()
            .collect()
    }

    fn agent_dirs_of(home: &Path) -> Vec<PathBuf> {
        vec![infer_dir_in(home).join("agents")]
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn entry(spelling: &str, read_only: bool) -> ListEntry {
        ListEntry {
            entry: spelling.into(),
            read_only,
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_address_travels_in_the_agent_file_and_the_key_only_in_auth_yaml() {
        let home = TempDir::new("split");
        seed_agent(&home);
        config::set_agent_api_key_in(&home, "code-reviewer", " am_us_secret ").unwrap();

        let text =
            std::fs::read_to_string(infer_dir_in(&home).join("agents/code-reviewer.md")).unwrap();
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
    }

    #[cfg(unix)]
    #[test]
    fn a_new_key_replaces_and_a_blank_one_clears_leaving_the_rest() {
        let home = TempDir::new("replace");
        std::fs::create_dir_all(infer_dir_in(&home)).unwrap();
        std::fs::write(
            infer_dir_in(&home).join("auth.yaml"),
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
    }

    #[cfg(unix)]
    #[test]
    fn a_move_renames_the_key_and_a_move_without_one_is_a_no_op() {
        let home = TempDir::new("move");
        config::set_agent_api_key_in(&home, "old-name", "am_us_secret").unwrap();
        move_agent_email_key_in(&home, "old-name", "new-name").unwrap();
        assert_eq!(config::agent_api_key_in(&home, "new-name"), "am_us_secret");
        assert!(config::agent_api_key_in(&home, "old-name").is_empty());

        move_agent_email_key_in(&home, "ghost", "other").unwrap();
        assert!(config::agent_api_key_in(&home, "other").is_empty());
    }

    #[test]
    fn only_a_valid_agent_with_an_address_has_mail() {
        let home = TempDir::new("mail-agent");
        seed_agent(&home);
        let dirs = agent_dirs_of(&home);
        assert_eq!(
            mail_agent_in(&dirs, "code-reviewer").map(|a| a.email),
            Some("code-reviewer@agentmail.to".to_owned())
        );
        assert!(mail_agent_in(&dirs, "other").is_none());
        assert!(mail_agent_in(&dirs, "../agents/code-reviewer").is_none());
        assert!(mail_agent_in(&dirs, "").is_none());
    }

    #[test]
    fn the_escape_keeps_a_value_inside_its_url_segment() {
        assert_eq!(escape_segment("a@b.example"), "a%40b.example");
        assert_eq!(escape_segment("a/b"), "a%2Fb");
        assert_eq!(escape_segment("a..b-2_3~x"), "a..b-2_3~x");
        assert_eq!(
            allowlist_url("me@agentmail.to"),
            "https://api.agentmail.to/v0/inboxes/me%40agentmail.to/lists/send/allow"
        );
    }

    #[test]
    fn the_effective_allowlist_honours_the_cap_and_never_opens_up() {
        let check = |entries: &[&str], cap: &[&str], allowed: &[&str], dropped: &[&str]| {
            let got = effective_allowlist(&strings(entries), &strings(cap), "Me@AgentMail.to");
            assert_eq!(got.0, normalized(&strings(allowed)), "{entries:?} {cap:?}");
            assert_eq!(got.1, strings(dropped), "{entries:?} {cap:?}");
        };
        check(
            &["A@Example.com", " ", "a@example.com"],
            &[],
            &["a@example.com"],
            &[],
        );
        check(&[], &[], &["me@agentmail.to"], &[]);
        check(
            &["boss@corp.example", "x@evil.example", "peer@agentmail.to"],
            &["corp.example", "peer@agentmail.to"],
            &["boss@corp.example", "peer@agentmail.to"],
            &["x@evil.example"],
        );
        check(
            &["x@evil.example"],
            &["corp.example"],
            &["me@agentmail.to"],
            &["x@evil.example"],
        );
        check(&["corp.example"], &["CORP.example"], &["corp.example"], &[]);
    }

    #[test]
    fn a_sync_plan_adds_the_missing_and_removes_the_rest_as_spelled() {
        let current = [
            entry("Keep@Example.com", false),
            entry("Stale@Example.com", false),
            entry("locked@example.com", true),
        ];
        let wanted = normalized(&strings(&["keep@example.com", "new@example.com"]));
        assert_eq!(
            plan_sync(&current, &wanted),
            (
                strings(&["new@example.com"]),
                strings(&["Stale@Example.com"])
            )
        );
        assert_eq!(plan_sync(&[], &wanted).1, Vec::<String>::new());
    }
}
