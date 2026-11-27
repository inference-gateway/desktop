use serde::{Deserialize, Serialize};
use serde_norway::{Mapping, Value};
use std::path::{Path, PathBuf};

/// The CLI's rooms settings in ~/.infer: the router and the rooms.
pub(crate) const ROOMS_FILE: &str = "rooms.yaml";

/// The room the CLI keeps when rooms.yaml defines none of that name.
const DEFAULT_ROOM: &str = "public";

/// The agent entry that seats every agent of the user's own, built-ins aside.
const ALL_AGENTS: &str = "*";

/// The router the CLI runs on while rooms.yaml leaves a key out.
const DEFAULT_ROUTER_MODEL: &str = "ollama_cloud/clef:27b";
const DEFAULT_MIN_CONFIDENCE: f64 = 0.5;
const DEFAULT_FALLBACK: &str = "top";

const ROOMS_KEY: &str = "rooms";
const ROUTER_KEY: &str = "router";

/// One room as rooms.yaml holds it. `project` is the directory its members
/// run in, empty for the daemon's default, and `project_name` the sidebar
/// project at that directory, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct RoomEntry {
    name: String,
    project: String,
    project_name: Option<String>,
    agents: Vec<String>,
}

/// A room as the editor saves it. `project` is a sidebar project's name, a
/// directory kept as found, or empty for none.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RoomEdit {
    name: String,
    project: String,
    agents: Vec<String>,
}

/// The router keys the Settings tab edits: the decision model asked whom a
/// post that pings nobody is for, the bar a member must reach, and what
/// happens when nobody does (`top` or `none`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RouterSettings {
    model: String,
    min_confidence: f64,
    fallback: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RoomsSettings {
    router: RouterSettings,
    rooms: Vec<RoomEntry>,
}

/// An agent a room can seat, as `infer agents list` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RoomAgent {
    name: String,
    #[serde(default)]
    built_in: bool,
}

pub(crate) fn rooms_path() -> PathBuf {
    crate::env::infer_dir().join(ROOMS_FILE)
}

/// Whether `name` may name a room: a letter or digit, then letters, digits
/// and underscores, so a member's session id splits at its first dash.
fn valid_room_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_string()
}

fn room_entry(value: &Value, project_name: &impl Fn(&str) -> Option<String>) -> RoomEntry {
    let project = text_of(&value["project"]);
    RoomEntry {
        name: text_of(&value["name"]),
        project_name: (!project.is_empty())
            .then(|| project_name(&project))
            .flatten(),
        project,
        agents: value["agents"]
            .as_sequence()
            .map(|agents| agents.iter().map(text_of).collect())
            .unwrap_or_default(),
    }
}

/// The rooms of rooms.yaml, with the default `public` room appended when the
/// file defines none of that name, as the CLI does.
fn rooms_in(root: &Value, project_name: impl Fn(&str) -> Option<String>) -> Vec<RoomEntry> {
    let mut rooms: Vec<RoomEntry> = root[ROOMS_KEY]
        .as_sequence()
        .map(|rooms| rooms.iter().map(|r| room_entry(r, &project_name)).collect())
        .unwrap_or_default();
    if !rooms.iter().any(|r| r.name == DEFAULT_ROOM) {
        rooms.push(RoomEntry {
            name: DEFAULT_ROOM.into(),
            project: String::new(),
            project_name: None,
            agents: vec![ALL_AGENTS.into()],
        });
    }
    rooms
}

fn router_in(root: &Value) -> RouterSettings {
    let router = &root[ROUTER_KEY];
    RouterSettings {
        model: router["model"]
            .as_str()
            .unwrap_or(DEFAULT_ROUTER_MODEL)
            .to_string(),
        min_confidence: router["min_confidence"]
            .as_f64()
            .unwrap_or(DEFAULT_MIN_CONFIDENCE),
        fallback: router["fallback"]
            .as_str()
            .unwrap_or(DEFAULT_FALLBACK)
            .to_string(),
    }
}

fn rooms_mut(root: &mut Value) -> Result<&mut Vec<Value>, String> {
    let map = root.as_mapping_mut().ok_or("rooms.yaml is not a mapping")?;
    let rooms = map
        .entry(ROOMS_KEY.into())
        .or_insert_with(|| Value::Sequence(Vec::new()));
    if !rooms.is_sequence() {
        *rooms = Value::Sequence(Vec::new());
    }
    rooms
        .as_sequence_mut()
        .ok_or_else(|| "rooms is not a list".into())
}

fn set_or_remove(room: &mut Mapping, key: &str, value: &str) {
    if value.is_empty() {
        room.remove(key);
    } else {
        room.insert(key.into(), value.into());
    }
}

/// rooms.yaml with `room` saved in place of the room named `previous`, or
/// added. Its other keys, such as `mode` and `pickup`, stay as found.
fn merge_room(
    existing: Option<&str>,
    previous: Option<&str>,
    room: &RoomEdit,
    project: &str,
) -> Result<String, String> {
    if !valid_room_name(&room.name) {
        return Err(format!(
            "room name {:?}: start with a letter or digit and keep to letters, digits and underscores",
            room.name
        ));
    }
    if room.agents.is_empty() {
        return Err("a room needs at least one member".into());
    }
    let mut root = crate::cli::config::config_mapping(existing);
    let rooms = rooms_mut(&mut root)?;
    let named = |r: &Value, name: &str| r["name"].as_str() == Some(name);
    if previous != Some(room.name.as_str()) && rooms.iter().any(|r| named(r, &room.name)) {
        return Err(format!("a room named {} exists", room.name));
    }
    let index = previous.and_then(|previous| rooms.iter().position(|r| named(r, previous)));
    if index.is_none() {
        rooms.push(Value::Mapping(Mapping::new()));
    }
    let entry = index.unwrap_or(rooms.len() - 1);
    let map = rooms[entry]
        .as_mapping_mut()
        .ok_or("a room in rooms.yaml is not a mapping")?;
    map.insert("name".into(), room.name.clone().into());
    set_or_remove(map, "project", project);
    map.insert(
        "agents".into(),
        Value::Sequence(room.agents.iter().cloned().map(Value::from).collect()),
    );
    serde_norway::to_string(&root).map_err(|e| e.to_string())
}

/// rooms.yaml without the room named `name`. Deleting `public` brings back
/// the CLI's default one.
fn merge_delete(existing: Option<&str>, name: &str) -> Result<String, String> {
    let mut root = crate::cli::config::config_mapping(existing);
    rooms_mut(&mut root)?.retain(|r| r["name"].as_str() != Some(name));
    serde_norway::to_string(&root).map_err(|e| e.to_string())
}

/// rooms.yaml with the router's model, bar and fallback set and its other
/// keys, such as `max_members`, kept.
fn merge_router(existing: Option<&str>, router: &RouterSettings) -> Result<String, String> {
    if !(0.0..=1.0).contains(&router.min_confidence) {
        return Err("min_confidence must be between 0 and 1".into());
    }
    if !router.model.is_empty() && !router.model.contains('/') {
        return Err(format!(
            "router model {:?}: must be provider/model",
            router.model
        ));
    }
    let mut root = crate::cli::config::config_mapping(existing);
    let map = root.as_mapping_mut().ok_or("rooms.yaml is not a mapping")?;
    crate::cli::config::set_section(
        map,
        ROUTER_KEY,
        vec![
            ("model", router.model.clone().into()),
            ("min_confidence", router.min_confidence.into()),
            ("fallback", router.fallback.clone().into()),
        ],
    );
    serde_norway::to_string(&root).map_err(|e| e.to_string())
}

/// The sidebar project whose directory is `dir`, if any.
fn sidebar_project_at(dir: &str) -> Option<String> {
    let dir = Path::new(dir);
    crate::projects::project_names_at()
        .into_iter()
        .find_map(|(name, at)| (at == dir).then_some(name))
}

/// The directory a saved room's project names: a sidebar project's, a
/// directory as given, or empty for none.
fn project_dir_of(project: &str) -> String {
    if project.is_empty() || Path::new(project).is_absolute() {
        return project.to_string();
    }
    crate::projects::project_dir(project)
        .map(|dir| dir.display().to_string())
        .unwrap_or_default()
}

/// The router and the rooms of rooms.yaml as the editor shows them.
#[tauri::command]
pub(crate) fn read_rooms_settings() -> Result<RoomsSettings, String> {
    let text = crate::cli::config::editable_text(&rooms_path())?;
    let root = crate::cli::config::config_mapping(text.as_deref());
    Ok(RoomsSettings {
        router: router_in(&root),
        rooms: rooms_in(&root, sidebar_project_at),
    })
}

/// Saves `room` to rooms.yaml, in place of the room named `previous` when it
/// is a rename or an edit, else as a new room.
#[tauri::command]
pub(crate) fn save_room(room: RoomEdit, previous: Option<String>) -> Result<(), String> {
    let project = project_dir_of(&room.project);
    crate::cli::config::update_file(&rooms_path(), |existing| {
        merge_room(existing, previous.as_deref(), &room, &project)
    })
}

/// Removes the room named `name` from rooms.yaml. Its log stays on disk.
#[tauri::command]
pub(crate) fn delete_room(name: String) -> Result<(), String> {
    crate::cli::config::update_file(&rooms_path(), |existing| merge_delete(existing, &name))
}

#[tauri::command]
pub(crate) fn save_room_router(router: RouterSettings) -> Result<(), String> {
    crate::cli::config::update_file(&rooms_path(), |existing| merge_router(existing, &router))
}

/// Every agent a room can seat, the CLI's built-ins included.
#[tauri::command]
pub(crate) async fn list_room_agents() -> Result<Vec<RoomAgent>, String> {
    let listed = crate::cli::infer::run_infer(&["agents", "list", "--format", "json"]).await?;
    agents_in(&listed)
}

fn agents_in(listed: &str) -> Result<Vec<RoomAgent>, String> {
    let listed: serde_json::Value = serde_json::from_str(listed).map_err(|e| e.to_string())?;
    Ok(serde_json::from_value(listed["markdown"].clone()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(name: &str, agents: &[&str]) -> RoomEdit {
        RoomEdit {
            name: name.into(),
            project: String::new(),
            agents: agents.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn parsed(text: &str) -> Value {
        serde_norway::from_str(text).unwrap()
    }

    #[test]
    fn a_file_without_public_lists_the_default_public_room() {
        let root = parsed("rooms:\n  - name: release\n    agents:\n      - tester\n");
        let names: Vec<_> = rooms_in(&root, |_| None)
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["release", "public"]);
        assert_eq!(rooms_in(&Value::Null, |_| None)[0].agents, ["*"]);
    }

    #[test]
    fn a_saved_room_keeps_its_other_keys_and_the_files() {
        let existing = "context_posts: 5\nrooms:\n  - name: release\n    mode: plan\n    agents:\n      - tester\n";
        let saved = parsed(
            &merge_room(
                Some(existing),
                Some("release"),
                &edit("ship", &["writer"]),
                "/p",
            )
            .unwrap(),
        );
        assert_eq!(saved["context_posts"], 5);
        assert_eq!(saved["rooms"][0]["name"], "ship");
        assert_eq!(saved["rooms"][0]["mode"], "plan");
        assert_eq!(saved["rooms"][0]["project"], "/p");
        assert_eq!(saved["rooms"][0]["agents"], parsed("- writer"));
    }

    #[test]
    fn saving_the_default_public_room_writes_it_and_a_new_room_is_appended() {
        let saved = parsed(&merge_room(None, Some("public"), &edit("public", &["*"]), "").unwrap());
        assert_eq!(saved["rooms"][0]["name"], "public");
        assert!(saved["rooms"][0].get("project").is_none());
        let both = merge_room(
            Some(&serde_norway::to_string(&saved).unwrap()),
            None,
            &edit("ship", &["main"]),
            "",
        )
        .unwrap();
        assert_eq!(parsed(&both)["rooms"][1]["name"], "ship");
        assert!(
            both.contains("- '*'"),
            "the agent list stays block style: {both}"
        );
    }

    #[test]
    fn a_bad_name_a_taken_name_or_no_member_is_refused() {
        let existing = "rooms:\n  - name: release\n    agents:\n      - tester\n";
        assert!(merge_room(None, None, &edit("bad-name", &["main"]), "").is_err());
        assert!(merge_room(None, None, &edit("_x", &["main"]), "").is_err());
        assert!(merge_room(Some(existing), None, &edit("release", &["main"]), "").is_err());
        assert!(merge_room(None, None, &edit("ship", &[]), "").is_err());
        assert!(
            merge_room(
                Some(existing),
                Some("release"),
                &edit("release", &["main"]),
                ""
            )
            .is_ok()
        );
    }

    #[test]
    fn deleting_a_room_keeps_the_rest() {
        let existing = "router:\n  model: a/b\nrooms:\n  - name: release\n  - name: ship\n";
        let left = parsed(&merge_delete(Some(existing), "release").unwrap());
        assert_eq!(left["router"]["model"], "a/b");
        assert_eq!(left["rooms"], parsed("- name: ship"));
    }

    #[test]
    fn the_router_saves_its_keys_and_keeps_the_rest() {
        let router = RouterSettings {
            model: "ollama_cloud/glm-5.3-flash".into(),
            min_confidence: 0.7,
            fallback: "none".into(),
        };
        let saved = parsed(&merge_router(Some("router:\n  max_members: 3\n"), &router).unwrap());
        assert_eq!(saved["router"]["max_members"], 3);
        assert_eq!(router_in(&saved), router);
        assert_eq!(router_in(&Value::Null).model, DEFAULT_ROUTER_MODEL);
        let bad = RouterSettings {
            min_confidence: 1.5,
            ..router.clone()
        };
        assert!(merge_router(None, &bad).is_err());
        let unqualified = RouterSettings {
            model: "clef".into(),
            ..router
        };
        assert!(merge_router(None, &unqualified).is_err());
    }

    #[test]
    fn the_agents_list_names_every_markdown_agent_with_the_built_ins() {
        let listed = r#"{"markdown":[{"name":"reviewer","source":"user"},{"name":"explore","built_in":true}],"local":[]}"#;
        assert_eq!(
            agents_in(listed).unwrap(),
            vec![
                RoomAgent {
                    name: "reviewer".into(),
                    built_in: false
                },
                RoomAgent {
                    name: "explore".into(),
                    built_in: true
                },
            ]
        );
    }
}
