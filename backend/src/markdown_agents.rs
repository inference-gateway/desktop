use crate::env::home_dir;
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::DialogExt;

/// A Markdown agent definition (`<name>.md`, Claude Code / infer format): the
/// frontmatter is its profile, the body its system prompt. `emoji`,
/// `workspace` (the computer its chats run on), `project` (the project its
/// new chats open in), `email` (its AgentMail inbox address) and `email_to`
/// (the comma-separated addresses it may send to) are desktop-only keys the
/// CLI ignores, the picture is a sibling `<name>.<ext>`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MarkdownAgent {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) emoji: String,
    #[serde(default)]
    pub(crate) model: String,
    #[serde(default)]
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) project: String,
    #[serde(default)]
    pub(crate) email: String,
    #[serde(default)]
    pub(crate) email_to: String,
    #[serde(default)]
    pub(crate) instructions: String,
    #[serde(default)]
    pub(crate) avatar: Option<String>,
    #[serde(default)]
    pub(crate) path: String,
}

#[derive(Default, serde::Deserialize)]
struct Frontmatter {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    emoji: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    workspace: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    email_to: String,
}

const AVATAR_EXTENSIONS: [&str; 5] = ["gif", "png", "webp", "jpg", "jpeg"];
/// ponytail: avatars travel inline as data URLs, so cap them rather than stream them.
/// Fluent's animated PNGs run up to 3.4 MB. Serve them through the asset protocol if the list gets heavy.
const AVATAR_MAX_BYTES: u64 = 4 * 1024 * 1024;
const NAME_MAX_LEN: usize = 64;
/// The frontmatter keys the agent editor owns. Every other key is kept as found.
const MANAGED_KEYS: [&str; 8] = [
    "name",
    "description",
    "emoji",
    "model",
    "workspace",
    "project",
    "email",
    "email_to",
];
/// Microsoft's Fluent Emoji animated PNGs (MIT) and Google's Noto Animated
/// Emoji (Apache 2.0): the only remote sources the avatar downloader accepts,
/// so a crafted URL cannot pull arbitrary bytes.
const ANIMATED_EMOJI_BASES: [&str; 2] = [
    "https://media.githubusercontent.com/media/microsoft/fluentui-emoji-animated/",
    "https://fonts.gstatic.com/s/e/notoemoji/",
];

/// The CLI's rule for a named agent: lowercase letters, digits, `-` and `_`.
pub(crate) fn valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Where agents live, first match winning on a name - the CLI's layout for a
/// chat run in `chat_dir`: its `.infer/agents` and `.agents/agents`, then the
/// home dir's. The first home dir, `~/.infer/agents`, takes new agents.
pub(crate) fn agent_dirs_for(chat_dir: &Path, home: &Path) -> Vec<PathBuf> {
    [chat_dir, home]
        .into_iter()
        .flat_map(|root| {
            [
                root.join(".infer").join("agents"),
                root.join(".agents").join("agents"),
            ]
        })
        .collect()
}

pub(crate) fn agent_dirs() -> Vec<PathBuf> {
    agent_dirs_for(&crate::env::agent_cwd(), &home_dir())
}

/// The directory holding agent `name`'s file and picture: the first of `dirs`
/// with `<name>.md`, else `new_agents` for an agent not saved yet.
pub(crate) fn dir_of(name: &str, dirs: &[PathBuf], new_agents: PathBuf) -> PathBuf {
    dirs.iter()
        .find(|dir| dir.join(format!("{name}.md")).is_file())
        .cloned()
        .unwrap_or(new_agents)
}

fn agent_dir(name: &str) -> PathBuf {
    dir_of(
        name,
        &agent_dirs(),
        home_dir().join(".infer").join("agents"),
    )
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---")?;
    let body = &rest[end + 4..];
    let body = body
        .strip_prefix('\n')
        .or_else(|| body.strip_prefix("\r\n"))
        .unwrap_or(body);
    Some((&rest[..end], body))
}

fn parse_agent(path: &Path, text: &str) -> Result<MarkdownAgent, String> {
    let (front, body) = split_frontmatter(text).ok_or("missing frontmatter")?;
    let fm: Frontmatter = serde_norway::from_str(front).map_err(|e| e.to_string())?;
    if !valid_agent_name(&fm.name) {
        return Err(format!("invalid name {:?}", fm.name));
    }
    if fm.description.trim().is_empty() {
        return Err("missing description".into());
    }
    Ok(MarkdownAgent {
        name: fm.name,
        description: fm.description,
        emoji: fm.emoji,
        model: fm.model,
        workspace: fm.workspace,
        project: fm.project,
        email: fm.email,
        email_to: fm.email_to,
        instructions: body.trim().to_string(),
        avatar: None,
        path: path.display().to_string(),
    })
}

fn avatar_path_in(dir: &Path, name: &str) -> Option<PathBuf> {
    AVATAR_EXTENSIONS
        .iter()
        .map(|ext| dir.join(format!("{name}.{ext}")))
        .find(|p| p.is_file())
}

fn avatar_data_url(path: &Path) -> Option<String> {
    use base64::Engine as _;
    let size = std::fs::metadata(path).ok()?.len();
    if size > AVATAR_MAX_BYTES {
        return None;
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "gif" => "image/gif",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    let bytes = std::fs::read(path).ok()?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Every valid agent under `dir`, sorted by name. Invalid files are skipped
/// with a log line so one broken agent never hides the rest.
pub(crate) fn list_in(dir: &Path) -> Vec<MarkdownAgent> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut agents: Vec<MarkdownAgent> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            match parse_agent(&p, &text) {
                Ok(mut agent) => {
                    agent.avatar =
                        avatar_path_in(dir, &agent.name).and_then(|a| avatar_data_url(&a));
                    Some(agent)
                }
                Err(e) => {
                    eprintln!("skipping agent {}: {e}", p.display());
                    None
                }
            }
        })
        .collect();
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Every agent in `dirs`, the first one found winning on a name, sorted by name.
pub(crate) fn list_from(dirs: &[PathBuf]) -> Vec<MarkdownAgent> {
    let mut seen = std::collections::HashSet::new();
    let mut agents: Vec<MarkdownAgent> = dirs
        .iter()
        .flat_map(|dir| list_in(dir))
        .filter(|agent| seen.insert(agent.name.clone()))
        .collect();
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// The agent `name`: the first matching file wins like `list_from`. Its
/// picture is left unattached, an address lookup never needs it.
pub(crate) fn by_name_in(dirs: &[PathBuf], name: &str) -> Option<MarkdownAgent> {
    let path = dirs
        .iter()
        .find(|dir| dir.join(format!("{name}.md")).is_file())?;
    let path = path.join(format!("{name}.md"));
    let text = std::fs::read_to_string(&path).ok()?;
    parse_agent(&path, &text).ok()
}

/// The frontmatter of the agent file at `path` without the keys the editor
/// manages, so a save keeps what other tools put there (`tools`,
/// `disallowedTools`, `color`). Empty when the file is missing or unreadable.
fn unmanaged_frontmatter(path: &Path) -> serde_norway::Mapping {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    split_frontmatter(&text)
        .and_then(|(front, _)| serde_norway::from_str::<serde_norway::Mapping>(front).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| !key.as_str().is_some_and(|k| MANAGED_KEYS.contains(&k)))
        .collect()
}

fn render(agent: &MarkdownAgent, kept: serde_norway::Mapping) -> Result<String, String> {
    let mut front = serde_norway::Mapping::new();
    front.insert("name".into(), agent.name.clone().into());
    front.insert("description".into(), agent.description.trim().into());
    if !agent.emoji.trim().is_empty() {
        front.insert("emoji".into(), agent.emoji.trim().into());
    }
    let model = agent.model.trim();
    if !model.is_empty() && model != "inherit" {
        front.insert("model".into(), model.into());
    }
    if !agent.workspace.trim().is_empty() {
        front.insert("workspace".into(), agent.workspace.trim().into());
    }
    if !agent.project.trim().is_empty() {
        front.insert("project".into(), agent.project.trim().into());
    }
    if !agent.email.trim().is_empty() {
        front.insert("email".into(), agent.email.trim().into());
    }
    if !agent.email_to.trim().is_empty() {
        front.insert("email_to".into(), agent.email_to.trim().into());
    }
    front.extend(kept);
    let yaml = serde_norway::to_string(&front).map_err(|e| e.to_string())?;
    Ok(format!("---\n{yaml}---\n\n{}\n", agent.instructions.trim()))
}

/// Writes `<dir>/<name>.md` in place of `<dir>/<replaced>.md`, the agent's
/// previous name on a rename, creating the directory. The keys the editor does
/// not manage carry over, and a cleared model, computer or project leaves the file.
pub(crate) fn write_in(
    dir: &Path,
    agent: &MarkdownAgent,
    replaced: &str,
) -> Result<PathBuf, String> {
    if !valid_agent_name(&agent.name) {
        return Err(format!(
            "invalid agent name {:?}: use lowercase letters, digits, - and _ (max {NAME_MAX_LEN})",
            agent.name
        ));
    }
    if agent.description.trim().is_empty() {
        return Err("an agent needs a description".into());
    }
    if agent.instructions.trim().is_empty() {
        return Err("an agent needs instructions".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let kept = unmanaged_frontmatter(&dir.join(format!("{replaced}.md")));
    let path = dir.join(format!("{}.md", agent.name));
    std::fs::write(&path, render(agent, kept)?)
        .map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

fn remove_avatar_in(dir: &Path, name: &str) {
    while let Some(existing) = avatar_path_in(dir, name) {
        if std::fs::remove_file(&existing).is_err() {
            break;
        }
    }
}

/// Removes the agent file and its picture. A missing agent is not an error.
pub(crate) fn delete_in(dir: &Path, name: &str) -> Result<(), String> {
    if !valid_agent_name(name) {
        return Err(format!("invalid agent name: {name}"));
    }
    let path = dir.join(format!("{name}.md"));
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("removing {}: {e}", path.display())),
    }
    remove_avatar_in(dir, name);
    Ok(())
}

fn avatar_extension(path: &str) -> Result<String, String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| AVATAR_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| "pick a gif, png, webp or jpg picture".to_string())
}

/// Copies `source` in as the agent's picture, replacing any previous one.
pub(crate) fn set_avatar_in(dir: &Path, name: &str, source: &Path) -> Result<PathBuf, String> {
    let ext = avatar_extension(&source.to_string_lossy())?;
    let size = std::fs::metadata(source).map_err(|e| e.to_string())?.len();
    if size > AVATAR_MAX_BYTES {
        return Err("pictures are capped at 4 MB".into());
    }
    let bytes = std::fs::read(source).map_err(|e| format!("reading picture: {e}"))?;
    store_avatar_in(dir, name, &ext, &bytes)
}

fn store_avatar_in(dir: &Path, name: &str, ext: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    if !valid_agent_name(name) {
        return Err(format!("invalid agent name: {name}"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    remove_avatar_in(dir, name);
    let target = dir.join(format!("{name}.{ext}"));
    std::fs::write(&target, bytes).map_err(|e| format!("writing picture: {e}"))?;
    Ok(target)
}

fn fetch_animated_emoji(url: &str) -> Result<Vec<u8>, String> {
    if !ANIMATED_EMOJI_BASES
        .iter()
        .any(|base| url.starts_with(base))
    {
        return Err("only Fluent or Noto animated emoji can be fetched".into());
    }
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("fetching {url}: {e}"))?;
    resp.into_body()
        .with_config()
        .limit(AVATAR_MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("reading {url}: {e}"))
}

#[tauri::command]
pub(crate) fn list_markdown_agents() -> Vec<MarkdownAgent> {
    list_from(&agent_dirs())
}

/// Creates or overwrites an agent. A rename passes `previous_name`, whose
/// file and picture move along with it.
#[tauri::command]
pub(crate) fn save_markdown_agent(
    agent: MarkdownAgent,
    previous_name: Option<String>,
) -> Result<String, String> {
    let replaced = previous_name.as_deref().unwrap_or(&agent.name);
    let dir = agent_dir(replaced);
    let path = write_in(&dir, &agent, replaced)?;
    if let Some(old) = previous_name.filter(|old| *old != agent.name) {
        if let Some(picture) = avatar_path_in(&dir, &old) {
            set_avatar_in(&dir, &agent.name, &picture)?;
        }
        delete_in(&dir, &old)?;
    }
    Ok(path.display().to_string())
}

#[tauri::command]
pub(crate) fn delete_markdown_agent(name: String) -> Result<(), String> {
    delete_in(&agent_dir(&name), &name)
}

/// Lets the user pick a picture for the agent. Returns false when they cancel.
#[tauri::command]
pub(crate) async fn pick_agent_avatar(app: tauri::AppHandle, name: String) -> Result<bool, String> {
    if !valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("Picture", &AVATAR_EXTENSIONS)
            .blocking_pick_file();
        let Some(picture) = picked.and_then(|fp| fp.into_path().ok()) else {
            return Ok(false);
        };
        set_avatar_in(&agent_dir(&name), &name, &picture).map(|_| true)
    })
    .await
    .map_err(|e| format!("avatar task failed: {e}"))?
}

/// Downloads a Noto animated emoji as the agent's picture.
#[tauri::command]
pub(crate) async fn set_agent_avatar_from_url(name: String, url: String) -> Result<(), String> {
    let ext = avatar_extension(&url)?;
    tokio::task::spawn_blocking(move || {
        let bytes = fetch_animated_emoji(&url)?;
        store_avatar_in(&agent_dir(&name), &name, &ext, &bytes).map(|_| ())
    })
    .await
    .map_err(|e| format!("avatar task failed: {e}"))?
}

#[tauri::command]
pub(crate) fn remove_agent_avatar(name: String) -> Result<(), String> {
    if !valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    remove_avatar_in(&agent_dir(&name), &name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-agents-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn sample() -> MarkdownAgent {
        MarkdownAgent {
            name: "code-reviewer".into(),
            description: "Reviews diffs: finds bugs".into(),
            emoji: "🔍".into(),
            model: "inherit".into(),
            workspace: String::new(),
            project: String::new(),
            email: String::new(),
            email_to: String::new(),
            instructions: "You are a senior reviewer.\n\nReport findings by severity.".into(),
            avatar: None,
            path: String::new(),
        }
    }

    #[test]
    fn name_validation_follows_the_cli_rule() {
        assert!(valid_agent_name("code-reviewer"));
        assert!(valid_agent_name("bot_2"));
        assert!(!valid_agent_name(""));
        assert!(!valid_agent_name("Code Reviewer"));
        assert!(!valid_agent_name("../evil"));
        assert!(!valid_agent_name(&"a".repeat(65)));
    }

    #[test]
    fn agent_dirs_follow_the_cli_layout() {
        let dirs = agent_dirs_for(Path::new("/work/proj"), Path::new("/Users/x"));
        assert_eq!(
            dirs,
            [
                "/work/proj/.infer/agents",
                "/work/proj/.agents/agents",
                "/Users/x/.infer/agents",
                "/Users/x/.agents/agents",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn the_first_dir_with_an_agent_wins_and_holds_its_files() {
        let root = temp_dir("layout");
        let dirs = agent_dirs_for(&root.join("proj"), &root.join("home"));
        let shadowing = MarkdownAgent {
            instructions: "The project's reviewer.".into(),
            ..sample()
        };
        write_in(&dirs[1], &shadowing, &shadowing.name).unwrap();
        write_in(&dirs[2], &sample(), &sample().name).unwrap();
        let only_home = MarkdownAgent {
            name: "helper".into(),
            ..sample()
        };
        write_in(&dirs[3], &only_home, &only_home.name).unwrap();

        let listed = list_from(&dirs);
        assert_eq!(
            listed.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["code-reviewer", "helper"]
        );
        assert_eq!(listed[0].instructions, "The project's reviewer.");
        assert_eq!(dir_of("code-reviewer", &dirs, dirs[2].clone()), dirs[1]);
        assert_eq!(dir_of("helper", &dirs, dirs[2].clone()), dirs[3]);
        assert_eq!(dir_of("unsaved", &dirs, dirs[2].clone()), dirs[2]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_then_list_round_trips_and_omits_inherit_model() {
        let dir = temp_dir("roundtrip");
        let path = write_in(&dir, &sample(), &sample().name).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("---\nname: code-reviewer\n"), "{text}");
        assert!(text.contains("emoji: 🔍\n"));
        assert!(!text.contains("model:"));
        assert!(
            text.ends_with("---\n\nYou are a senior reviewer.\n\nReport findings by severity.\n")
        );
        let listed = list_in(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "code-reviewer");
        assert_eq!(listed[0].description, "Reviews diffs: finds bugs");
        assert_eq!(listed[0].emoji, "🔍");
        assert_eq!(listed[0].instructions, sample().instructions);
        assert_eq!(listed[0].avatar, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn desktop_keys_round_trip_and_are_omitted_when_blank() {
        let dir = temp_dir("desktop-keys");
        let path = write_in(&dir, &sample(), &sample().name).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("workspace:"), "{text}");
        assert!(!text.contains("project:"), "{text}");
        let mut agent = sample();
        agent.workspace = "ubuntu-desktop".into();
        agent.project = "desktop".into();
        write_in(&dir, &agent, &agent.name).unwrap();
        let listed = &list_in(&dir)[0];
        assert_eq!(listed.workspace, "ubuntu-desktop");
        assert_eq!(listed.project, "desktop");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn email_trips_the_frontmatter_and_the_whitelist_along() {
        let dir = temp_dir("email");
        let path = write_in(&dir, &sample(), &sample().name).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("email"), "{text}");
        let mut agent = sample();
        agent.email = " code-reviewer@agentmail.to ".into();
        agent.email_to = " human@example.com, other-agent@agentmail.to ".into();
        write_in(&dir, &agent, &agent.name).unwrap();

        let text = std::fs::read_to_string(dir.join("code-reviewer.md")).unwrap();
        assert!(
            text.contains("email: code-reviewer@agentmail.to\n"),
            "{text}"
        );
        assert!(
            text.contains("email_to: human@example.com, other-agent@agentmail.to\n"),
            "{text}"
        );
        let listed = &list_in(&dir)[0];
        assert_eq!(listed.email, "code-reviewer@agentmail.to");
        assert_eq!(
            listed.email_to,
            "human@example.com, other-agent@agentmail.to"
        );
        let by_name = by_name_in(&[dir.clone()], "code-reviewer").unwrap();
        assert_eq!(by_name.email, "code-reviewer@agentmail.to");
        assert!(by_name_in(&[dir.clone()], "ghost").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_save_keeps_unmanaged_keys_and_drops_a_cleared_model() {
        let dir = temp_dir("keep");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("researcher.md"),
            "---\nname: researcher\ndescription: Researches\nmodel: mock/openai/gpt-4o\nworkspace: ubuntu-desktop\ntools:\n  - Read\n  - Grep\ndisallowedTools: Bash\ncolor: blue\n---\nResearch it.\n",
        )
        .unwrap();
        let mut agent = list_in(&dir).remove(0);
        agent.model = String::new();
        agent.workspace = String::new();
        agent.emoji = "🔎".into();
        write_in(&dir, &agent, "researcher").unwrap();

        let text = std::fs::read_to_string(dir.join("researcher.md")).unwrap();
        assert!(!text.contains("model:"), "{text}");
        assert!(!text.contains("workspace:"), "{text}");
        assert!(text.contains("emoji: 🔎\n"), "{text}");
        assert!(text.contains("tools:\n- Read\n- Grep\n"), "{text}");
        assert!(text.contains("disallowedTools: Bash\n"), "{text}");
        assert!(text.contains("color: blue\n"), "{text}");
        assert_eq!(list_in(&dir)[0].model, "");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_rename_carries_the_unmanaged_keys_to_the_new_file() {
        let dir = temp_dir("rename-keep");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("old-name.md"),
            "---\nname: old-name\ndescription: x\ntools: Read\n---\nbody\n",
        )
        .unwrap();
        let mut agent = list_in(&dir).remove(0);
        agent.name = "new-name".into();
        write_in(&dir, &agent, "old-name").unwrap();

        let text = std::fs::read_to_string(dir.join("new-name.md")).unwrap();
        assert!(text.starts_with("---\nname: new-name\n"), "{text}");
        assert!(text.contains("tools: Read\n"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_files_are_skipped_and_claude_files_load() {
        let dir = temp_dir("skip");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.md"), "no frontmatter here").unwrap();
        std::fs::write(
            dir.join("Upper.md"),
            "---\nname: Upper\ndescription: x\n---\nbody",
        )
        .unwrap();
        std::fs::write(
            dir.join("claude.md"),
            "---\nname: planner\ndescription: Plans work\ntools: Read, Grep\nmodel: sonnet\ncolor: blue\n---\nPlan it.\n",
        )
        .unwrap();
        let listed = list_in(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "planner");
        assert_eq!(listed[0].model, "sonnet");
        assert_eq!(listed[0].instructions, "Plan it.");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn avatar_is_attached_replaced_and_deleted_with_the_agent() {
        let dir = temp_dir("avatar");
        write_in(&dir, &sample(), &sample().name).unwrap();
        let src = dir.join("pick.gif");
        std::fs::write(&src, b"GIF89a").unwrap();
        set_avatar_in(&dir, "code-reviewer", &src).unwrap();
        let listed = list_in(&dir);
        assert!(
            listed[0]
                .avatar
                .as_deref()
                .unwrap()
                .starts_with("data:image/gif;base64,")
        );
        let png = dir.join("pick.png");
        std::fs::write(&png, b"\x89PNG").unwrap();
        set_avatar_in(&dir, "code-reviewer", &png).unwrap();
        assert!(!dir.join("code-reviewer.gif").exists());
        assert!(dir.join("code-reviewer.png").exists());
        assert!(set_avatar_in(&dir, "code-reviewer", &dir.join("code-reviewer.md")).is_err());
        delete_in(&dir, "code-reviewer").unwrap();
        assert!(list_in(&dir).is_empty());
        assert!(!dir.join("code-reviewer.png").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn animated_emoji_fetch_only_accepts_the_noto_host() {
        assert!(fetch_animated_emoji("https://evil.example/512.gif").is_err());
        assert_eq!(
            avatar_extension("https://fonts.gstatic.com/s/e/notoemoji/latest/1f916/512.webp")
                .unwrap(),
            "webp"
        );
        assert!(
            avatar_extension("https://fonts.gstatic.com/s/e/notoemoji/latest/1f916/lottie.json")
                .is_err()
        );
        let fluent = "https://media.githubusercontent.com/media/microsoft/fluentui-emoji-animated/main/assets/Robot/animated/robot_animated.png";
        assert_eq!(avatar_extension(fluent).unwrap(), "png");
        assert!(
            fetch_animated_emoji("https://media.githubusercontent.com/media/microsoft/other/x.png")
                .is_err()
        );
    }

    #[test]
    fn write_rejects_incomplete_agents() {
        let dir = temp_dir("reject");
        let mut agent = sample();
        agent.name = "Bad Name".into();
        assert!(write_in(&dir, &agent, &agent.name).is_err());
        let mut agent = sample();
        agent.description = " ".into();
        assert!(write_in(&dir, &agent, &agent.name).is_err());
        let mut agent = sample();
        agent.instructions = String::new();
        assert!(write_in(&dir, &agent, &agent.name).is_err());
        assert!(!dir.exists());
    }
}
