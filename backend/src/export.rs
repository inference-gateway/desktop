// One-click export/import of the complete desktop state (issue #166).
//
// A single portable `DesktopExport` moves everything the desktop app owns:
// the Settings fields from ~/.infer/config.yaml, projects.json, A2A agents
// (via the `infer` CLI), scheduled jobs, snippets and the skills registry
// URL, plus installed skill names for automatic reinstall. The file format
// is user-selectable JSON/YAML/TOML with content-sniffing import. Secrets
// (auth.json keys, DB passwords, tokens) never enter the export.
use crate::agent::{A2aAgent, add_a2a_agent, list_a2a_agents, set_a2a_agent_model};
use crate::config::{config_from_value, merge_config, merge_default_model, DesktopConfig};
use crate::env::home_dir;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Bumped whenever the export shape changes; import rejects other versions.
const EXPORT_VERSION: u32 = 1;

/// Default file name of an export (the extension follows the chosen format).
pub(crate) const EXPORT_BASENAME: &str = "infer-desktop-export";

/// One snippet as persisted in ~/.infer/desktop.json and the export file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct SnippetEntry {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) prompt: String,
}

/// Desktop-only UI state under ~/.infer/desktop.json: the localStorage
/// items (snippets, skills registry URL) that must survive machine moves.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopData {
    #[serde(default)]
    pub(crate) snippets: Vec<SnippetEntry>,
    #[serde(default)]
    pub(crate) skills_registry_url: String,
}

/// The complete portable desktop state. One shape, three serializations.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopExport {
    version: u32,
    /// All Settings-managed fields; machine paths are `~/`-relative.
    config: DesktopConfig,
    /// Raw ~/.infer/projects.json value.
    #[serde(default)]
    projects: serde_json::Value,
    /// A2A agents, re-created through the `infer` CLI on import.
    #[serde(default)]
    agents: Vec<A2aAgent>,
    /// Scheduled job files: filename -> raw YAML value, round-tripped verbatim.
    #[serde(default)]
    schedules: BTreeMap<String, serde_norway::Value>,
    #[serde(default)]
    snippets: Vec<SnippetEntry>,
    #[serde(default)]
    skills_registry_url: String,
    /// Installed skill names; reinstalled through the CLI on import.
    #[serde(default)]
    skills: Vec<String>,
}

/// What an import changed, shown to the user in Settings.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ImportReport {
    imported: Vec<String>,
    warnings: Vec<String>,
}

/// What an export produced.
#[derive(serde::Serialize)]
pub(crate) struct ExportResult {
    location: String,
    warnings: Vec<String>,
}

// --- Tilde-relative machine paths -------------------------------------------

/// Absolute path under `home` becomes `~/...`; anything else is untouched.
fn tilde_path(raw: &str, home: &Path) -> String {
    Path::new(raw)
        .strip_prefix(home)
        .ok()
        .map(|rest| {
            if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.to_string_lossy())
            }
        })
        .unwrap_or_else(|_| raw.to_string())
}

/// Inverse of `tilde_path`: `~/rest` resolves against this machine's home.
fn expand_tilde(raw: &str, home: &Path) -> String {
    match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None if raw == "~" => home.to_string_lossy().into_owned(),
        None => raw.to_string(),
    }
}

// --- Reading the current state ----------------------------------------------

/// `read_config` rooted at an arbitrary home, so export/import is testable
/// against a temp dir. Reads <home>/.infer/config.yaml the same way
/// `read_config` does.
fn read_config_in(home: &Path) -> DesktopConfig {
    std::fs::read_to_string(home.join(".infer").join("config.yaml"))
        .ok()
        .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
        .map(|val| config_from_value(&val, home))
        .unwrap_or_else(|| crate::config::default_config())
}

/// The portable state the frontend keeps in localStorage: snippets and the
/// skills registry URL, mirrored under ~/.infer/desktop.json.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopData {
    #[serde(default)]
    pub(crate) snippets: Vec<SnippetEntry>,
    #[serde(default)]
    pub(crate) skills_registry_url: String,
}

fn desktop_data_path(home: &Path) -> PathBuf {
    home.join(".infer").join("desktop.json")
}

fn read_desktop_data_in(home: &Path) -> DesktopData {
    std::fs::read_to_string(desktop_data_path(home))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_desktop_data(data: &DesktopData, home: &Path) -> Result<(), String> {
    let path = desktop_data_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Failed to write {path:?}: {e}"))
}

fn desktop_data_path() -> PathBuf {
    home_dir().join(".infer").join("desktop.json")
}

/// Tilde-relative form of `path` when it lives under `home`; e.g. the export
/// carries `~/...` so it re-expands on any machine.
fn tilde_path(raw: &str, home: &Path) -> String {
    Path::new(raw)
        .strip_prefix(home)
        .ok()
        .map(|rest| {
            if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.to_string_lossy())
            }
        })
        .unwrap_or_else(|| raw.to_string())
}

/// Inverse of `tilde_path` against the (new) machine's home.
fn expand_tilde(raw: &str, home: &Path) -> String {
    match raw.strip_prefix("~/") {
        Some(rest) if rest.is_empty() => home.to_string_lossy().into_owned(),
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None => raw.to_string(),
    }
}

fn expand_paths_in(v: &mut serde_json::Value, home: &Path, tilde: bool) {
    let Some(obj) = v.get_mut("paths").and_then(|p| p.as_object_mut()) else {
        return;
    };
    for val in obj.values_mut() {
        if let Some(s) = val_string_mut(v).or(None) {
            let _ = s;
        }
        let _ = &mut obj;
        let _ = v;
        let _ = home;
        let _ = tilde;
        if let Some(s) = v.as_str() {
            let _ = s;
        }
id: unused_marker()
    }
}

// --- Collecting the export ---------------------------------------------------

/// Secrets must not appear in exported files: backend passwords and tokens are
/// blanked (`auth.json` provider keys are never read for the export at all).
fn scrub_credentials(cfg: &mut DesktopConfig) {
    cfg.postgres_password.clear();
    cfg.redis_password.clear();
    cfg.d1_api_token.clear();
}

/// Config as seen by the Settings UI, rooted at `home` (mirrors `read_config`).
fn read_desktop_config(home: &Path) -> DesktopConfig {
    std::fs::read_to_string(home.join(".infer").join("config.yaml"))
        .ok()
        .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
        .map(|val| config_from_value(&val, home))
        .unwrap_or_else(crate::config::default_config)
}

/// Scheduled job files verbatim (filename -> parsed YAML) so CLI-written
/// fields the desktop does not model still survive the trip.
fn read_schedules_in(home: &Path) -> BTreeMap<String, serde_norway::Value> {
    let dir = home.join(".infer").join("schedules");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|e| e == "yaml"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let val = serde_norway::from_str::<serde_norway::Value>(&text).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), val))
        })
        .collect()
}

/// projects.json content with machine-absolute `paths` overrides stored
/// tilde-relative, like the config's own path fields.
fn read_projects_in(home: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(home.join(".infer").join("projects.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_else(|| json!({}));
    tilde_paths_in(&raw, Path::new(&home.display()), tilde_path)
}