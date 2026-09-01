// One-click export/import of the complete desktop state (issue #166).
//
// A single portable `DesktopExport` moves everything the desktop app owns:
// the Settings fields from ~/.infer/config.yaml, projects.json, A2A agents
// (via the `infer` CLI), scheduled jobs, snippets and the skills registry
// URL, plus installed skill names for automatic reinstall. The file format
// is user-selectable JSON/YAML/TOML with content-sniffing import. Secrets
// (auth.json keys, DB passwords, tokens) never enter the export.
//
// The testable core is rooted at a passed `home` (no global env):
// `build_export` / `apply_export_files` / `parse_export`; the tauri commands
// below are thin wrappers. Round-trip tests live at the bottom.
use crate::agent::{A2aAgent, add_a2a_agent, list_a2a_agents, set_a2a_agent_model};
use crate::config::{DesktopConfig, merge_config, merge_default_model};
use crate::env::home_dir;
use base64::Engine as _;
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use tauri_plugin_dialog::DialogExt;

/// Bumped whenever the export shape changes; import rejects other versions.
const EXPORT_VERSION: u32 = 1;

/// Default file name of an export (the extension follows the chosen format).
pub(crate) const EXPORT_BASENAME: &str = "infer-desktop-export";

/// One snippet as persisted in ~/.infer/desktop.json and the export file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct SnippetEntry {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) prompt: String,
}

/// Desktop-only UI state under ~/.infer/desktop.json: the localStorage items
/// (snippets, skills registry URL) that must survive machine moves.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopData {
    #[serde(default)]
    pub(crate) snippets: Vec<SnippetEntry>,
    #[serde(default)]
    pub(crate) skills_registry_url: String,
}

/// The complete portable desktop state. One shape, three serializations
/// (JSON/YAML/TOML, see `serialize_export` / `parse_export`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopExport {
    version: u32,
    /// All Settings-managed fields; machine paths are `~/`-relative.
    config: DesktopConfig,
    /// Raw ~/.infer/projects.json value (`paths` overrides tilde-relative).
    #[serde(default)]
    projects: serde_json::Value,
    /// Git-repo projects as their GitHub `owner/name`: re-cloned on import so the
    /// checkout (and its AGENTS.md) need not travel in the file. See
    /// `strip_git_project_remotes`.
    #[serde(default)]
    project_remotes: BTreeMap<String, String>,
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
    /// Installed skill names; reinstalled through the `infer` CLI on import.
    #[serde(default)]
    skills: Vec<String>,
}

/// What an import changed, shown to the user in Settings.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ImportReport {
    imported: Vec<String>,
    warnings: Vec<String>,
}

/// What an export produced. A `null` location means the user cancelled.
#[derive(serde::Serialize)]
pub(crate) struct ExportResult {
    location: Option<String>,
    warnings: Vec<String>,
}

/// The user-selectable export serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Json,
    Yaml,
    Toml,
}

impl ExportFormat {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "json" => Ok(Self::Json),
            "yaml" | "yml" => Ok(Self::Yaml),
            "toml" => Ok(Self::Toml),
            other => Err(format!("unsupported format: {other} (json, yaml or toml)")),
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
        }
    }
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
        .unwrap_or_else(|| raw.to_string())
}

/// Inverse of `tilde_path`: `~/rest` resolves against this machine's home.
fn expand_tilde(raw: &str, home: &Path) -> String {
    match raw.strip_prefix("~/") {
        Some("") => home.to_string_lossy().into_owned(),
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None if raw == "~" => home.to_string_lossy().into_owned(),
        None => raw.to_string(),
    }
}

// --- Reading the current state ----------------------------------------------

/// Scheduled job files verbatim (filename -> parsed YAML) so CLI-written
/// fields the desktop does not model still survive the trip.
fn read_schedules_in(home: &Path) -> BTreeMap<String, serde_norway::Value> {
    let Ok(entries) = std::fs::read_dir(home.join(".infer").join("schedules")) else {
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

/// projects.json content, with machine-absolute `paths` overrides stored
/// tilde-relative (projects.rs re-expands `~` when reading them).
fn read_projects_in(home: &Path) -> serde_json::Value {
    let mut raw: serde_json::Value =
        std::fs::read_to_string(home.join(".infer").join("projects.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| json!({}));
    if let Some(paths) = raw.get_mut("paths").and_then(|p| p.as_object_mut()) {
        for v in paths.values_mut() {
            if let Some(s) = v.as_str() {
                *v = tilde_path(s, home).into();
            }
        }
    }
    raw
}

// --- Collecting the export ---------------------------------------------------

/// Secrets must not appear in exported files: backend passwords, tokens, and
/// the GitHub App secret references are blanked (`auth.json` provider keys are
/// never read for the export at all). This is the allowlist the "no secrets"
/// guarantee rests on - add any new secret-bearing field here.
fn scrub_credentials(cfg: &mut DesktopConfig) {
    cfg.postgres_password.clear();
    cfg.redis_password.clear();
    cfg.d1_api_token.clear();
    cfg.scheduler_github_app_client_id_secret.clear();
    cfg.scheduler_github_app_private_key_secret.clear();
}

/// Replace each git-repo project's embedded copy with its GitHub `owner/name`:
/// for every `paths` entry whose checkout has a GitHub remote, record the remote
/// and drop the machine-specific `paths` entry plus the `contexts` entry - the
/// context is derived from the repo's AGENTS.md and re-read from the fresh
/// clone on import, so embedding it is pure bloat (and goes stale). Only
/// non-GitHub/local repos keep their context embedded.
fn strip_git_project_remotes(
    projects: &mut serde_json::Value,
    home: &Path,
) -> BTreeMap<String, String> {
    let mut remotes = BTreeMap::new();
    let entries: Vec<(String, String)> = projects
        .get("paths")
        .and_then(|p| p.as_object())
        .map(|paths| {
            paths
                .iter()
                .filter_map(|(name, v)| Some((name.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    for (name, tilde) in entries {
        let dir = PathBuf::from(expand_tilde(&tilde, home));
        let Some(repo) = crate::projects::git_remote_repo(&dir) else {
            continue;
        };
        remotes.insert(name.clone(), repo);
        if let Some(paths) = projects.get_mut("paths").and_then(|p| p.as_object_mut()) {
            paths.remove(&name);
        }
        if let Some(ctx) = projects.get_mut("contexts").and_then(|c| c.as_object_mut()) {
            ctx.remove(&name);
        }
    }
    remotes
}

/// The portable state rooted at `home`. `agents` comes from the `infer` CLI
/// by the caller (it needs an async CLI round-trip); tests pass a fixed list.
fn build_export(home: &Path, agents: Vec<A2aAgent>) -> DesktopExport {
    let mut config = crate::config::read_config_in(home);
    for path in [
        &mut config.storage_directory,
        &mut config.sqlite_path,
        &mut config.projects_root,
    ] {
        let s = std::mem::take(path);
        *path = tilde_path(&s, home);
    }
    scrub_credentials(&mut config);
    let mut data = read_desktop_data_in(home);
    let mut projects = read_projects_in(home);
    let project_remotes = strip_git_project_remotes(&mut projects, home);
    DesktopExport {
        version: EXPORT_VERSION,
        config,
        projects,
        project_remotes,
        agents,
        schedules: read_schedules_in(home),
        snippets: std::mem::take(&mut data.snippets),
        skills_registry_url: data.skills_registry_url,
        skills: crate::skills::installed_skills_in(home),
    }
}

// --- Serialization ------------------------------------------------------------

/// `YAML first` (JSON is valid YAML so it parses there too), `TOML` second;
/// anything that does not deserialize as `DesktopExport` is rejected with a
/// clear error, and a wrong `version` is rejected afterwards.
pub(crate) fn parse_export(text: &str) -> Result<DesktopExport, String> {
    let export = serde_norway::from_str::<DesktopExport>(text).or_else(|yaml_err| {
        toml::from_str::<DesktopExport>(text).map_err(|toml_err| {
            format!("not a desktop export file: yaml: {yaml_err} / toml: {toml_err}")
        })
    })?;
    if export.version != EXPORT_VERSION {
        return Err(format!(
            "unsupported export version {} (expected {EXPORT_VERSION})",
            export.version
        ));
    }
    Ok(export)
}

fn serialize_export(export: &DesktopExport, format: ExportFormat) -> Result<String, String> {
    match format {
        ExportFormat::Json => serde_json::to_string_pretty(export)
            .map_err(|e| format!("failed to serialize export: {e}")),
        ExportFormat::Yaml => {
            serde_norway::to_string(export).map_err(|e| format!("failed to serialize export: {e}"))
        }
        ExportFormat::Toml => {
            toml::to_string_pretty(export).map_err(|e| format!("failed to serialize export: {e}"))
        }
    }
}

// --- ~/.infer/desktop.json ----------------------------------------------------

fn read_desktop_data_in(home: &Path) -> DesktopData {
    std::fs::read_to_string(home.join(".infer").join("desktop.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_desktop_data(data: &DesktopData, home: &Path) -> Result<(), String> {
    let path = home.join(".infer").join("desktop.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

// --- Applying an import -------------------------------------------------------

/// Deep-merge `patch` into `base`: mappings merge key-wise, arrays union
/// (missing entries appended, order of `base` kept), anything else replaces.
fn merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) => merge_json(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (serde_json::Value::Array(old), serde_json::Value::Array(new)) => {
            for v in new {
                if !old.contains(v) {
                    old.push(v.clone());
                }
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// Only plain `*.yaml` file names: blocks schedule-file traversal on import.
fn valid_schedule_file(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && name.ends_with(".yaml")
}

/// Write the file-backed state from `export` into `home`: config.yaml (merged
/// with `merge_config` semantics so CLI-managed keys survive), projects.json
/// (merged), schedules (verbatim, traversal-guarded) and desktop.json.
/// Agents and skills need the `infer` CLI and are handled separately.
fn apply_export_files(export: &DesktopExport, home: &Path) -> Result<ImportReport, String> {
    let mut imported = Vec::new();
    let mut warnings = Vec::new();

    // Config: expand the source machine's `~/` paths against this machine's
    // home, keep non-empty local secrets when the export carried blanks.
    let mut cfg = export.config.clone();
    for path in [
        &mut cfg.storage_directory,
        &mut cfg.sqlite_path,
        &mut cfg.projects_root,
    ] {
        *path = expand_tilde(path, home);
    }
    let local = crate::config::read_config_in(home);
    if cfg.postgres_password.is_empty() {
        cfg.postgres_password = local.postgres_password.clone();
    }
    if cfg.redis_password.is_empty() {
        cfg.redis_password = local.redis_password.clone();
    }
    if cfg.d1_api_token.is_empty() {
        cfg.d1_api_token = local.d1_api_token.clone();
    }
    if cfg.scheduler_github_app_client_id_secret.is_empty() {
        cfg.scheduler_github_app_client_id_secret =
            local.scheduler_github_app_client_id_secret.clone();
    }
    if cfg.scheduler_github_app_private_key_secret.is_empty() {
        cfg.scheduler_github_app_private_key_secret =
            local.scheduler_github_app_private_key_secret.clone();
    }
    let config_path = home.join(".infer").join("config.yaml");
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(&config_path).ok();
    let text = merge_config(existing.as_deref(), &cfg)?;
    let text = merge_default_model(Some(&text), &cfg.default_model)?;
    std::fs::write(&config_path, text).map_err(|e| e.to_string())?;
    imported.push("Settings (config.yaml)".into());

    let projects_path = home.join(".infer").join("projects.json");
    let mut projects: serde_json::Value = std::fs::read_to_string(&projects_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    merge_json(&mut projects, &export.projects);
    if let Some(parent) = projects_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &projects_path,
        serde_json::to_string_pretty(&projects).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    imported.push("Projects (projects.json)".into());

    if !export.schedules.is_empty() {
        let dir = home.join(".infer").join("schedules");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut n = 0;
        for (name, val) in &export.schedules {
            if !valid_schedule_file(name) {
                warnings.push(format!("skipped schedule file with unsafe name: {name}"));
                continue;
            }
            let body = serde_norway::to_string(val)
                .map_err(|e| format!("failed to re-serialize schedule {name}: {e}"))?;
            std::fs::write(dir.join(name), body).map_err(|e| e.to_string())?;
            n += 1;
        }
        imported.push(format!("{n} scheduled job(s)"));
    }

    let data = DesktopData {
        snippets: export.snippets.clone(),
        skills_registry_url: export.skills_registry_url.clone(),
    };
    write_desktop_data(&data, home)?;
    imported.push("Snippets + skills registry URL".into());

    Ok(ImportReport { imported, warnings })
}

/// Re-create A2A agents missing on this machine via the `infer` CLI
/// (desktop and CLI stay in sync; the CLI's agents.yaml is never parsed).
async fn apply_export_agents(agents: &[A2aAgent], warnings: &mut Vec<String>) -> usize {
    let existing = match list_a2a_agents().await {
        Ok(v) => v,
        Err(e) => {
            warnings.push(format!("A2A agents not imported: {e}"));
            return 0;
        }
    };
    let names: HashSet<&str> = existing.iter().map(|a| a.name.as_str()).collect();
    let mut added = 0;
    for agent in agents {
        if names.contains(agent.name.as_str()) {
            continue;
        }
        match add_a2a_agent(agent.name.clone(), agent.url.clone()).await {
            Ok(()) => {
                added += 1;
                if !agent.model.is_empty()
                    && let Err(e) =
                        set_a2a_agent_model(agent.name.clone(), agent.model.clone()).await
                {
                    warnings.push(format!("agent {} added but model not set: {e}", agent.name));
                }
            }
            Err(e) => warnings.push(format!("agent {} not added: {e}", agent.name)),
        }
    }
    added
}

/// Reinstall skills missing on this machine; failures are warnings, not fatal.
async fn apply_export_skills(skills: &[String], warnings: &mut Vec<String>) -> usize {
    let installed = crate::skills::installed_skills_in(&home_dir());
    let mut n = 0;
    for name in skills {
        if !crate::skills::valid_name(name) {
            warnings.push(format!("skipped invalid skill name: {name}"));
            continue;
        }
        if installed.iter().any(|i| i == name) {
            continue;
        }
        match crate::agent::run_infer(&["skills", "install", name, "--user"]).await {
            Ok(_) => n += 1,
            Err(e) => warnings.push(format!("skill {name} not installed: {e}")),
        }
    }
    n
}

/// Write each cloned checkout's path (and its AGENTS.md context, unless the export
/// already carried one) onto its project in projects.json.
fn patch_projects_json(
    home: &Path,
    cloned: &BTreeMap<String, crate::projects::GitRepo>,
) -> Result<(), String> {
    if cloned.is_empty() {
        return Ok(());
    }
    let path = home.join(".infer").join("projects.json");
    let mut projects: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    let obj = projects
        .as_object_mut()
        .ok_or("projects.json is not an object")?;
    {
        let paths = obj.entry("paths").or_insert_with(|| json!({}));
        if let Some(paths) = paths.as_object_mut() {
            for (name, repo) in cloned {
                paths.insert(name.clone(), repo.path.clone().into());
            }
        }
    }
    {
        let contexts = obj.entry("contexts").or_insert_with(|| json!({}));
        if let Some(contexts) = contexts.as_object_mut() {
            for (name, repo) in cloned {
                if let Some(ctx) = &repo.context {
                    contexts
                        .entry(name.clone())
                        .or_insert_with(|| ctx.clone().into());
                }
            }
        }
    }
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&projects).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Re-clone the git-repo projects recorded in `project_remotes` under the imported
/// projects root, then record their checkout path and AGENTS.md context in
/// projects.json. Clone failures are warnings, not fatal (as with agents/skills).
async fn apply_export_project_repos(
    remotes: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> usize {
    if remotes.is_empty() {
        return 0;
    }
    let home = home_dir();
    let root = PathBuf::from(expand_tilde(
        &crate::config::read_config_in(&home).projects_root,
        &home,
    ));
    let mut cloned = BTreeMap::new();
    for (name, repo) in remotes {
        let (root, repo) = (root.clone(), repo.clone());
        match tokio::task::spawn_blocking(move || crate::projects::clone_repo_under(&root, &repo))
            .await
        {
            Ok(Ok(gitrepo)) => {
                cloned.insert(name.clone(), gitrepo);
            }
            Ok(Err(e)) => warnings.push(format!("project {name} not cloned: {e}")),
            Err(e) => warnings.push(format!("project {name} clone task failed: {e}")),
        }
    }
    if let Err(e) = patch_projects_json(&home, &cloned) {
        warnings.push(format!("cloned projects not recorded: {e}"));
    }
    cloned.len()
}

/// Full import of a parsed export: files first, then the CLI-mediated pieces.
async fn apply_export(export: &DesktopExport) -> Result<ImportReport, String> {
    let home = home_dir();
    let mut report = apply_export_files(export, &home)?;
    let repos = apply_export_project_repos(&export.project_remotes, &mut report.warnings).await;
    if !export.project_remotes.is_empty() {
        report
            .imported
            .push(format!("{repos} project repo(s) cloned"));
    }
    let agents = apply_export_agents(&export.agents, &mut report.warnings).await;
    if !export.agents.is_empty() {
        report.imported.push(format!("{agents} A2A agent(s)"));
    }
    let skills = apply_export_skills(&export.skills, &mut report.warnings).await;
    if !export.skills.is_empty() {
        report.imported.push(format!("{skills} skill(s)"));
    }
    Ok(report)
}

// --- GitHub private-repo target (via `gh`) ------------------------------------

/// `gh` with an override token; without one the CLI's stored auth is used.
fn gh_call(args: &[&str], token: Option<&str>) -> Result<String, String> {
    let token = token.map(str::trim).filter(|t| !t.is_empty());
    let Some(token) = token else {
        return crate::scheduler::gh_output(args);
    };
    let out = std::process::Command::new(crate::download::gh_bin())
        .args(args)
        .env("GH_TOKEN", token)
        .output()
        .map_err(|e| format!("gh failed to start: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Ensure the repo is private (a missing repo is created private; a public
/// repo is refused - the export must never land anywhere readable), then PUT
/// the file at the repo root (Contents API, overwrite via the existing sha).
fn github_push(repo: &str, filename: &str, text: &str, token: Option<&str>) -> Result<(), String> {
    let repo_url = format!("repos/{repo}");
    let private_error = |visibility: &str| {
        format!(
            "repository {repo} is not private (visibility={visibility}) - refusing to write the export"
        )
    };
    match gh_call(&["api", &repo_url, "--jq", ".private"], token) {
        Ok(v) if v.trim() == "true" => {}
        Ok(v) => return Err(private_error(v.trim())),
        Err(_) => {
            gh_call(&["repo", "create", repo, "--private"], token)
                .map_err(|e| format!("could not create private repository {repo}: {e}"))?;
            let v = gh_call(&["api", &repo_url, "--jq", ".private"], token)
                .map_err(|e| format!("private check failed for {repo}: {e}"))?;
            if v.trim() != "true" {
                return Err(private_error(v.trim()));
            }
        }
    }

    let content_url = format!("repos/{repo}/contents/{filename}");
    let sha = gh_call(&["api", &content_url, "--jq", ".sha"], token).unwrap_or_default();
    let sha = sha.trim().to_string();
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut args = vec![
        "api".to_string(),
        "--method".into(),
        "PUT".into(),
        content_url,
        "-f".into(),
        format!("message=add {filename}"),
        "-f".into(),
        format!("content={encoded}"),
    ];
    if !sha.is_empty() {
        args.push("-f".into());
        args.push(format!("sha={sha}"));
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    gh_call(&refs, token).map(|_| ())
}

/// Fetch the flattened `infer-desktop-export.<ext>` from a private repo,
/// trying all three supported extensions.
fn github_fetch(repo: &str, token: Option<&str>) -> Result<String, String> {
    for ext in ["yaml", "toml", "json"] {
        let url = format!("repos/{repo}/contents/{EXPORT_BASENAME}.{ext}");
        if let Ok(encoded) = gh_call(&["api", &url, "--jq", ".content"], token) {
            let clean: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(clean.as_bytes())
                .map_err(|e| format!("invalid base64 in {url}: {e}"))?;
            return String::from_utf8(bytes).map_err(|e| format!("export file is not UTF-8: {e}"));
        }
    }
    Err(format!(
        "no {EXPORT_BASENAME}.{{yaml,toml,json}} found in {repo} (is the export committed at the repo root?)"
    ))
}

// --- Tauri commands ------------------------------------------------------------

/// The export plus any CLI-mediated warnings (agents need the infer binary).
async fn collect_export(home: &Path) -> (DesktopExport, Vec<String>) {
    let (agents, warnings) = match list_a2a_agents().await {
        Ok(agents) => (agents, Vec::new()),
        Err(e) => (Vec::new(), vec![format!("A2A agents not included: {e}")]),
    };
    (build_export(home, agents), warnings)
}

/// Native dialogs via tauri-plugin-dialog's Rust-side blocking API, run off
/// the main thread; no capability entries needed (the JS side never calls it).
fn file_path_buf(fp: tauri_plugin_dialog::FilePath) -> Option<PathBuf> {
    match fp {
        tauri_plugin_dialog::FilePath::Path(p) => Some(p),
        _ => None,
    }
}

async fn native_save_path(
    app: tauri::AppHandle,
    format: ExportFormat,
) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Desktop export", &["json", "yaml", "yml", "toml"])
            .set_file_name(format!("{EXPORT_BASENAME}.{}", format.ext()))
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("save dialog failed: {e}"))
    .map(|fp| fp.and_then(file_path_buf))
}

async fn native_open_path(app: tauri::AppHandle) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Desktop export", &["json", "yaml", "yml", "toml"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("open dialog failed: {e}"))
    .map(|fp| fp.and_then(file_path_buf))
}

/// Write the export to the filesystem. Without `path`, the native save dialog
/// requests the target (a cancelled dialog reports `location: null`).
#[tauri::command]
pub(crate) async fn export_desktop_file(
    app: tauri::AppHandle,
    format: String,
    path: Option<String>,
) -> Result<ExportResult, String> {
    let format = ExportFormat::parse(&format)?;
    let home = home_dir();
    let (export, warnings) = collect_export(&home).await;
    let text = serialize_export(&export, format)?;
    let dest = match path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        Some(p) => Some(PathBuf::from(p)),
        None => native_save_path(app, format).await?,
    };
    let Some(mut target) = dest else {
        return Ok(ExportResult {
            location: None,
            warnings,
        });
    };
    if target.extension().is_none() {
        target.set_extension(format.ext());
    }
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&target, text)
        .map_err(|e| format!("failed to write {}: {e}", target.display()))?;
    Ok(ExportResult {
        location: Some(target.display().to_string()),
        warnings,
    })
}

/// Write the export into a private GitHub repo (created on demand). Trunk
/// based, flattened: the file lands at the root with its default name.
#[tauri::command]
pub(crate) async fn export_desktop_github(
    repo: String,
    format: String,
    token: Option<String>,
) -> Result<ExportResult, String> {
    if !crate::scheduler::valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    let format = ExportFormat::parse(&format)?;
    let (export, warnings) = collect_export(&home_dir()).await;
    let text = serialize_export(&export, format)?;
    let filename = format!("{EXPORT_BASENAME}.{}", format.ext());
    let repo_target = repo.clone();
    tokio::task::spawn_blocking(move || {
        github_push(&repo_target, &filename, &text, token.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(ExportResult {
        location: Some(repo),
        warnings,
    })
}

/// Import from a local file; without `path`, the native open dialog picks it.
#[tauri::command]
pub(crate) async fn import_desktop_file(
    app: tauri::AppHandle,
    path: Option<String>,
) -> Result<ImportReport, String> {
    let target = match path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        Some(p) => Some(PathBuf::from(p)),
        None => native_open_path(app).await?,
    };
    let Some(target) = target else {
        return Ok(ImportReport {
            imported: Vec::new(),
            warnings: Vec::new(),
        });
    };
    let text = std::fs::read_to_string(&target)
        .map_err(|e| format!("could not read {}: {e}", target.display()))?;
    apply_export(&parse_export(&text)?).await
}

/// Import the export committed to a private GitHub repo.
#[tauri::command]
pub(crate) async fn import_desktop_github(
    repo: String,
    token: Option<String>,
) -> Result<ImportReport, String> {
    if !crate::scheduler::valid_repo(&repo) {
        return Err(format!("invalid repository: {repo}"));
    }
    let text = tokio::task::spawn_blocking(move || github_fetch(&repo, token.as_deref()))
        .await
        .map_err(|e| e.to_string())??;
    apply_export(&parse_export(&text)?).await
}

/// The localStorage-backed desktop state, persisted under ~/.infer/desktop.json.
#[tauri::command]
pub(crate) fn read_desktop_data() -> DesktopData {
    read_desktop_data_in(&home_dir())
}

#[tauri::command]
pub(crate) fn save_desktop_snippets(snippets: Vec<SnippetEntry>) -> Result<(), String> {
    let home = home_dir();
    let mut data = read_desktop_data_in(&home);
    data.snippets = snippets;
    write_desktop_data(&data, &home)
}

#[tauri::command]
pub(crate) fn save_skills_registry_url(url: String) -> Result<(), String> {
    let home = home_dir();
    let mut data = read_desktop_data_in(&home);
    data.skills_registry_url = url;
    write_desktop_data(&data, &home)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "desktop-export-{}-{}-{tag}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn agent() -> A2aAgent {
        A2aAgent {
            name: "docs-agent".into(),
            url: "http://localhost:8090".into(),
            run: true,
            model: "gpt-test".into(),
        }
    }

    /// A home with every state kind the export moves, rooted at `home` so the
    /// tilde round-trip is exercised (paths written absolute).
    fn seed_home(home: &Path) {
        let infer = home.join(".infer");
        std::fs::create_dir_all(infer.join("schedules")).unwrap();
        std::fs::create_dir_all(infer.join("skills").join("code-review")).unwrap();
        std::fs::write(
            infer.join("skills").join("code-review").join("SKILL.md"),
            "n",
        )
        .unwrap();
        let h = home.display();
        std::fs::write(
            infer.join("config.yaml"),
            format!(
                "storage:\n  type: jsonl\n  jsonl:\n    path: {h}/conv\n  sqlite:\n    path: {h}/conv.db\n  postgres:\n    host: db\n    password: hunter2\n  redis:\n    password: rpw2\n  d1:\n    api_token: d1-secret\ngateway:\n  url: http://gw:9999\ndefault_model: m-x\nextra_instructions: be brief\nsystem_prompt: sys\ntools:\n  schedule:\n    enabled: true\nscheduler:\n  backend: github\n  github:\n    repository: alice/.routines\n    app_client_id_secret: gh-client-secret\n    app_private_key_secret: gh-key-secret\nprojects:\n  root: {h}/code\n  backend: local\n"
            ),
        )
        .unwrap();
        std::fs::write(
            infer.join("projects.json"),
            format!(
                "{{\"assignments\":{{\"a\":\"p1\"}},\"names\":[\"p1\"],\"contexts\":{{\"p1\":\"ctx\"}},\"groups\":{{\"p1\":\"g1\"}},\"paths\":{{\"p1\":\"{h}/src\"}}}}"
            ),
        )
        .unwrap();
        std::fs::write(
            infer.join("schedules").join("nightly.yaml"),
            "id: n1\nname: nightly\nprompt: check\nrun_once: false\ncron_expression: \"0 9 * * *\"\n",
        )
        .unwrap();
        std::fs::write(
            infer.join("desktop.json"),
            "{\"snippets\":[{\"id\":\"s1\",\"label\":\"Say hi\",\"prompt\":\"hi\"}],\"skills_registry_url\":\"https://reg.example/c.json\"}",
        )
        .unwrap();
    }

    #[test]
    fn round_trip_reproduces_equivalent_state_in_all_formats() {
        let home_a = temp_home("a");
        seed_home(&home_a);

        let export = build_export(&home_a, vec![agent()]);
        for format in [ExportFormat::Json, ExportFormat::Yaml, ExportFormat::Toml] {
            let home = temp_home(&format!("b-{}", format.ext()));
            let text = serialize_export(&export, format).unwrap();
            let parsed = parse_export(&text).unwrap();
            let report = apply_export_files(&parsed, &home).unwrap();
            assert!(
                report.warnings.is_empty(),
                "warnings: {:?}",
                report.warnings
            );
            assert_eq!(report.imported.len(), 4);
            // Skills are (re)installed via the CLI on a real import; a unit
            // test without the CLI fakes the install outcome instead.
            std::fs::create_dir_all(home.join(".infer").join("skills").join("code-review"))
                .unwrap();
            std::fs::write(
                home.join(".infer")
                    .join("skills")
                    .join("code-review")
                    .join("SKILL.md"),
                "n",
            )
            .unwrap();
            assert_eq!(build_export(&home, vec![agent()]), parsed);
            let _ = std::fs::remove_dir_all(&home);
        }
        let _ = std::fs::remove_dir_all(&home_a);
    }

    #[test]
    fn config_import_preserves_keys_and_defaults() {
        let home_source = temp_home("src");
        seed_home(&home_source);
        let export = build_export(&home_source, vec![]);
        let home = temp_home("tgt");
        std::fs::create_dir_all(home.join(".infer")).unwrap();
        std::fs::write(
            home.join(".infer").join("config.yaml"),
            "gateway:\n  run: true\n  standalone_binary: /x\nother_top: keep\nstorage:\n  type: jsonl\n  postgres:\n    password: keepme\n  redis:\n    password: keepredis\n  d1:\n    api_token: keepd1\n",
        )
        .unwrap();
        apply_export_files(&export, &home).unwrap();
        let text = std::fs::read_to_string(home.join(".infer").join("config.yaml")).unwrap();
        assert!(text.contains("other_top: keep"));
        assert!(text.contains("run: true"));
        assert!(text.contains("standalone_binary: /x"));
        // Blanked secrets do not overwrite non-empty local values.
        assert!(text.contains("keepme"));
        assert!(text.contains("keepredis"));
        assert!(text.contains("keepd1"));
        for tag in [&home_source, &home] {
            let _ = std::fs::remove_dir_all(tag);
        }
    }

    #[test]
    fn exported_file_carries_no_secrets() {
        let home = temp_home("secrets");
        seed_home(&home);
        let text = serialize_export(&build_export(&home, vec![]), ExportFormat::Json).unwrap();
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("rpw2"));
        assert!(!text.contains("d1-secret"));
        assert!(!text.contains("gh-client-secret"));
        assert!(!text.contains("gh-key-secret"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn git_projects_export_as_remotes_without_contexts() {
        let home = temp_home("gitproj");
        let infer = home.join(".infer");
        std::fs::create_dir_all(&infer).unwrap();
        std::fs::write(
            infer.join("config.yaml"),
            format!("projects:\n  root: {}/code\n", home.display()),
        )
        .unwrap();

        // GitHub checkout: the repo is the source of truth, so even a stored
        // context that drifted from the live AGENTS.md is dropped.
        let repo = home.join("code").join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = https://github.com/owner/repo.git\n",
        )
        .unwrap();
        std::fs::write(repo.join("AGENTS.md"), "live rules").unwrap();

        // Plain local project: no remote, context stays embedded.
        std::fs::write(
            infer.join("projects.json"),
            format!(
                "{{\"names\":[\"repo\",\"local\"],\"contexts\":{{\"repo\":\"stale snapshot\",\"local\":\"local notes\"}},\"paths\":{{\"repo\":\"{h}/code/repo\",\"local\":\"{h}/code/local\"}}}}",
                h = home.display()
            ),
        )
        .unwrap();

        let export = build_export(&home, vec![]);
        assert_eq!(
            export.project_remotes.get("repo").map(String::as_str),
            Some("owner/repo")
        );
        assert!(export.projects["contexts"].get("repo").is_none());
        assert!(export.projects["paths"].get("repo").is_none());
        assert_eq!(export.projects["contexts"]["local"], json!("local notes"));

        let text = serialize_export(&export, ExportFormat::Json).unwrap();
        assert!(!text.contains("stale snapshot"), "no AGENTS.md embedded");
        assert!(text.contains("local notes"), "local context kept");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn parse_rejects_garbage_and_wrong_version() {
        assert!(parse_export("hello, world").is_err());
        let home = temp_home("ver");
        seed_home(&home);
        let export = build_export(&home, vec![]);
        let text = serialize_export(&export, ExportFormat::Json).unwrap();
        let mut raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        raw["version"] = 99.into();
        let err = parse_export(&serde_json::to_string(&raw).unwrap()).unwrap_err();
        assert!(err.contains("unsupported export version 99"), "{err}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn schedule_import_rejects_traversal_names() {
        let home = temp_home("trav");
        let mut export = DesktopExport {
            version: EXPORT_VERSION,
            config: crate::config::default_config(),
            projects: json!({}),
            project_remotes: BTreeMap::new(),
            agents: Vec::new(),
            schedules: BTreeMap::from([
                ("../evil.yaml".to_string(), serde_norway::Value::Null),
                ("sub/x.yaml".to_string(), serde_norway::Value::Null),
                ("note.txt".to_string(), serde_norway::Value::Null),
                (
                    "ok.yaml".to_string(),
                    serde_norway::from_str("id: n1\nname: ok\n").unwrap(),
                ),
            ]),
            snippets: Vec::new(),
            skills_registry_url: String::new(),
            skills: Vec::new(),
        };
        scrub_credentials(&mut export.config);
        let report = apply_export_files(&export, &home).unwrap();
        assert!(home.join(".infer/schedules/ok.yaml").exists());
        assert!(!home.join("evil.yaml").exists());
        assert!(!home.join(".infer/schedules/sub").exists());
        assert_eq!(
            report
                .warnings
                .iter()
                .filter(|w| w.contains("unsafe name"))
                .count(),
            3
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tilde_paths_round_trip_and_leave_strangers_untouched() {
        let home = Path::new("/home/x");
        assert_eq!(tilde_path("/home/x/conv", home), "~/conv");
        assert_eq!(tilde_path(home.to_str().unwrap(), home), "~");
        assert_eq!(tilde_path("/etc/passwd", home), "/etc/passwd");
        assert_eq!(expand_tilde("~/conv", home), "/home/x/conv");
        assert_eq!(expand_tilde("~", home), "/home/x");
        assert_eq!(expand_tilde("/keep/me", home), "/keep/me");
    }
}
