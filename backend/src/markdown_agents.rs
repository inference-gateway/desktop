use crate::env::home_dir;
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::DialogExt;

/// A Markdown agent definition (`<name>.md`, Claude Code / infer format): the
/// frontmatter is its profile, the body its system prompt. `emoji` is a
/// desktop-only key the CLI ignores, the picture is a sibling `<name>.<ext>`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MarkdownAgent {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) emoji: String,
    #[serde(default)]
    pub(crate) model: String,
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
}

const AVATAR_EXTENSIONS: [&str; 5] = ["gif", "png", "webp", "jpg", "jpeg"];
/// ponytail: avatars travel inline as data URLs, so cap them rather than stream them.
const AVATAR_MAX_BYTES: u64 = 2 * 1024 * 1024;
const NAME_MAX_LEN: usize = 64;

/// The CLI's rule for a named agent: lowercase letters, digits, `-` and `_`.
pub(crate) fn valid_agent_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The configured agents directory with a leading `~` expanded, defaulting
/// to `~/.infer/agents` when the setting is blank.
pub(crate) fn agents_dir_from(setting: &str, home: &Path) -> PathBuf {
    let trimmed = setting.trim();
    if trimmed.is_empty() {
        return home.join(".infer").join("agents");
    }
    match trimmed.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if trimmed == "~" => home.to_path_buf(),
        None => PathBuf::from(trimmed),
    }
}

fn agents_dir() -> PathBuf {
    agents_dir_from(&crate::config::read_config().agents_dir, &home_dir())
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

fn render(agent: &MarkdownAgent) -> Result<String, String> {
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
    let yaml = serde_norway::to_string(&front).map_err(|e| e.to_string())?;
    Ok(format!("---\n{yaml}---\n\n{}\n", agent.instructions.trim()))
}

/// Writes `<dir>/<name>.md`, creating the directory. Returns the file path.
pub(crate) fn write_in(dir: &Path, agent: &MarkdownAgent) -> Result<PathBuf, String> {
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
    let path = dir.join(format!("{}.md", agent.name));
    std::fs::write(&path, render(agent)?)
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

/// Copies `source` in as the agent's picture, replacing any previous one.
pub(crate) fn set_avatar_in(dir: &Path, name: &str, source: &Path) -> Result<PathBuf, String> {
    if !valid_agent_name(name) {
        return Err(format!("invalid agent name: {name}"));
    }
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| AVATAR_EXTENSIONS.contains(&e.as_str()))
        .ok_or("pick a gif, png, webp or jpg picture")?;
    let size = std::fs::metadata(source).map_err(|e| e.to_string())?.len();
    if size > AVATAR_MAX_BYTES {
        return Err("pictures are capped at 2 MB".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    remove_avatar_in(dir, name);
    let target = dir.join(format!("{name}.{ext}"));
    std::fs::copy(source, &target).map_err(|e| format!("copying picture: {e}"))?;
    Ok(target)
}

#[tauri::command]
pub(crate) fn list_markdown_agents() -> Vec<MarkdownAgent> {
    list_in(&agents_dir())
}

/// Creates or overwrites an agent. A rename passes `previous_name`, whose
/// file and picture move along with it.
#[tauri::command]
pub(crate) fn save_markdown_agent(
    agent: MarkdownAgent,
    previous_name: Option<String>,
) -> Result<String, String> {
    let dir = agents_dir();
    let path = write_in(&dir, &agent)?;
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
    delete_in(&agents_dir(), &name)
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
        set_avatar_in(&agents_dir(), &name, &picture).map(|_| true)
    })
    .await
    .map_err(|e| format!("avatar task failed: {e}"))?
}

#[tauri::command]
pub(crate) fn remove_agent_avatar(name: String) -> Result<(), String> {
    if !valid_agent_name(&name) {
        return Err(format!("invalid agent name: {name}"));
    }
    remove_avatar_in(&agents_dir(), &name);
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
    fn agents_dir_expands_tilde_and_defaults() {
        let home = Path::new("/Users/x");
        assert_eq!(
            agents_dir_from("", home),
            PathBuf::from("/Users/x/.infer/agents")
        );
        assert_eq!(
            agents_dir_from("~/.agents/agents", home),
            PathBuf::from("/Users/x/.agents/agents")
        );
        assert_eq!(
            agents_dir_from("/srv/agents", home),
            PathBuf::from("/srv/agents")
        );
    }

    #[test]
    fn write_then_list_round_trips_and_omits_inherit_model() {
        let dir = temp_dir("roundtrip");
        let path = write_in(&dir, &sample()).unwrap();
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
        write_in(&dir, &sample()).unwrap();
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
    fn write_rejects_incomplete_agents() {
        let dir = temp_dir("reject");
        let mut agent = sample();
        agent.name = "Bad Name".into();
        assert!(write_in(&dir, &agent).is_err());
        let mut agent = sample();
        agent.description = " ".into();
        assert!(write_in(&dir, &agent).is_err());
        let mut agent = sample();
        agent.instructions = String::new();
        assert!(write_in(&dir, &agent).is_err());
        assert!(!dir.exists());
    }
}
