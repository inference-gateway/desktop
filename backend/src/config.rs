use crate::env::{config_path, home_dir};
use std::path::PathBuf;

/// App-owned provider key store at ~/.infer/auth.json (Codex-style).
/// infer does not read this file; the desktop injects its values as env vars
/// when spawning `infer`.
pub(crate) fn auth_path() -> PathBuf {
    home_dir().join(".infer").join("auth.json")
}

pub(crate) fn read_auth() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(auth_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Non-empty (KEY, value) pairs to inject as environment variables.
pub(crate) fn auth_env() -> Vec<(String, String)> {
    read_auth()
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[tauri::command]
pub(crate) async fn get_auth() -> Result<serde_json::Value, String> {
    Ok(serde_json::Value::Object(read_auth()))
}

#[tauri::command]
pub(crate) async fn set_auth(
    keys: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let path = auth_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let map: serde_json::Map<String, serde_json::Value> = keys
        .into_iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let json =
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Desktop-facing config fields read from ~/.infer/config.yaml.
/// Only the fields the Settings UI manages are exposed; the rest of the
/// config (gateway.run, gateway.standalone_binary, etc.) passes through.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct DesktopConfig {
    pub(crate) storage_backend: String,
    pub(crate) storage_directory: String,
    pub(crate) gateway_url: String,
    pub(crate) default_model: String,
    pub(crate) sqlite_path: String,
    pub(crate) postgres_host: String,
    pub(crate) postgres_port: String,
    pub(crate) postgres_database: String,
    pub(crate) postgres_username: String,
    pub(crate) postgres_password: String,
    pub(crate) postgres_ssl_mode: String,
    pub(crate) redis_host: String,
    pub(crate) redis_port: String,
    pub(crate) redis_password: String,
    pub(crate) redis_db: String,
    d1_account_id: String,
    d1_database_id: String,
    d1_api_token: String,
    d1_base_url: String,
    /// Extra instructions appended to the default system prompt on every invocation.
    pub(crate) extra_instructions: String,
    /// Override the entire default system prompt with user-supplied text.
    /// When both are set, override replaces the default and extra_instructions are
    /// appended after it.
    pub(crate) system_prompt: String,
    /// Enable local scheduling via `infer channels-manager`.
    pub(crate) schedule_enabled: bool,
}

pub(crate) fn default_storage_directory(home: &std::path::Path) -> String {
    home.join(".infer")
        .join("conversations")
        .to_string_lossy()
        .to_string()
}

pub(crate) fn default_config() -> DesktopConfig {
    DesktopConfig {
        storage_backend: "jsonl".into(),
        storage_directory: default_storage_directory(&home_dir()),
        gateway_url: "http://localhost:8080".into(),
        default_model: String::new(),
        sqlite_path: ".infer/conversations.db".into(),
        postgres_host: "localhost".into(),
        postgres_port: "5432".into(),
        postgres_database: "infer_conversations".into(),
        postgres_username: String::new(),
        postgres_password: String::new(),
        postgres_ssl_mode: "prefer".into(),
        redis_host: "localhost".into(),
        redis_port: "6379".into(),
        redis_password: String::new(),
        redis_db: "0".into(),
        d1_account_id: String::new(),
        d1_database_id: String::new(),
        d1_api_token: String::new(),
        d1_base_url: "https://api.cloudflare.com/client/v4".into(),
        extra_instructions: String::new(),
        system_prompt: String::new(),
        schedule_enabled: false,
    }
}

/// Pure core of `read_config`, split out so field extraction is testable
/// without touching ~/.infer.
pub(crate) fn config_from_value(
    val: &serde_norway::Value,
    home: &std::path::Path,
) -> DesktopConfig {
    let str_at = |path: &[&str]| -> Option<String> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_str().filter(|s| !s.is_empty()).map(str::to_string)
    };
    let int_at = |path: &[&str]| -> Option<i64> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_i64()
    };
    let d = default_config();
    DesktopConfig {
        storage_backend: str_at(&["storage", "type"]).unwrap_or_else(|| "jsonl".into()),
        storage_directory: str_at(&["storage", "jsonl", "path"])
            .map(|s| match s.strip_prefix('~') {
                Some(rest) => format!("{}{}", home.display(), rest),
                None => s,
            })
            .unwrap_or_else(|| default_storage_directory(home)),
        gateway_url: str_at(&["gateway", "url"]).unwrap_or_else(|| "http://localhost:8080".into()),
        default_model: str_at(&["default_model"]).unwrap_or_default(),
        sqlite_path: str_at(&["storage", "sqlite", "path"]).unwrap_or(d.sqlite_path),
        postgres_host: str_at(&["storage", "postgres", "host"]).unwrap_or(d.postgres_host),
        postgres_port: int_at(&["storage", "postgres", "port"])
            .map(|p| p.to_string())
            .unwrap_or(d.postgres_port),
        postgres_database: str_at(&["storage", "postgres", "database"])
            .unwrap_or(d.postgres_database),
        postgres_username: str_at(&["storage", "postgres", "username"])
            .unwrap_or(d.postgres_username),
        postgres_password: str_at(&["storage", "postgres", "password"])
            .unwrap_or(d.postgres_password),
        postgres_ssl_mode: str_at(&["storage", "postgres", "ssl_mode"])
            .unwrap_or(d.postgres_ssl_mode),
        redis_host: str_at(&["storage", "redis", "host"]).unwrap_or(d.redis_host),
        redis_port: int_at(&["storage", "redis", "port"])
            .map(|p| p.to_string())
            .unwrap_or(d.redis_port),
        redis_password: str_at(&["storage", "redis", "password"]).unwrap_or(d.redis_password),
        redis_db: int_at(&["storage", "redis", "db"])
            .map(|p| p.to_string())
            .unwrap_or(d.redis_db),
        d1_account_id: str_at(&["storage", "d1", "account_id"]).unwrap_or(d.d1_account_id),
        d1_database_id: str_at(&["storage", "d1", "database_id"]).unwrap_or(d.d1_database_id),
        d1_api_token: str_at(&["storage", "d1", "api_token"]).unwrap_or(d.d1_api_token),
        d1_base_url: str_at(&["storage", "d1", "base_url"]).unwrap_or(d.d1_base_url),
        extra_instructions: str_at(&["extra_instructions"]).unwrap_or_default(),
        system_prompt: str_at(&["system_prompt"]).unwrap_or_default(),
        schedule_enabled: val
                .get("tools")
                .and_then(|t| t.get("schedule"))
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
    }
}

pub(crate) fn read_config() -> DesktopConfig {
    match std::fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_norway::from_str::<serde_norway::Value>(&text).ok())
    {
        Some(val) => config_from_value(&val, &home_dir()),
        None => default_config(),
    }
}

/// Parse `text` as a YAML mapping, or start from a fresh one. A malformed or
/// non-mapping existing config is replaced rather than aborting the write.
pub(crate) fn config_mapping(text: Option<&str>) -> serde_norway::Value {
    text.and_then(|t| serde_norway::from_str::<serde_norway::Value>(t).ok())
        .filter(serde_norway::Value::is_mapping)
        .unwrap_or_else(|| serde_norway::Value::Mapping(Default::default()))
}

/// Parse `s` as an integer, falling back to `default` for empty/garbage input so
/// we never write a non-numeric value where infer's schema expects an int.
pub(crate) fn parse_int_or(s: &str, default: i64) -> i64 {
    s.trim().parse().unwrap_or(default)
}

/// Insert `kvs` into `storage.<name>` (creating the sub-mapping if absent),
/// leaving any keys infer manages but the desktop doesn't touch.
pub(crate) fn set_section(
    smap: &mut serde_norway::Mapping,
    name: &str,
    kvs: Vec<(&str, serde_norway::Value)>,
) {
    let section = smap
        .entry(name.into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(m) = section.as_mapping_mut() {
        for (k, v) in kvs {
            m.insert(k.into(), v);
        }
    }
}

/// Merge the Settings-managed storage + gateway fields into the existing config
/// text, returning serialized YAML. `default_model` is owned by
/// `merge_default_model` (written from the composer too), so it is deliberately
/// not touched here - otherwise a stale Settings form would clobber a newer
/// model chosen from the composer.
pub(crate) fn merge_config(existing: Option<&str>, cfg: &DesktopConfig) -> Result<String, String> {
    let mut yaml = config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;

    let storage = map
        .entry("storage".into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(smap) = storage.as_mapping_mut() {
        smap.insert("enabled".into(), true.into());
        smap.insert("type".into(), cfg.storage_backend.clone().into());
        smap.remove("backend");
        smap.remove("directory");

        set_section(
            smap,
            "jsonl",
            vec![("path", cfg.storage_directory.clone().into())],
        );
        set_section(
            smap,
            "sqlite",
            vec![("path", cfg.sqlite_path.clone().into())],
        );
        set_section(
            smap,
            "postgres",
            vec![
                ("host", cfg.postgres_host.clone().into()),
                ("port", parse_int_or(&cfg.postgres_port, 5432).into()),
                ("database", cfg.postgres_database.clone().into()),
                ("username", cfg.postgres_username.clone().into()),
                ("password", cfg.postgres_password.clone().into()),
                ("ssl_mode", cfg.postgres_ssl_mode.clone().into()),
            ],
        );
        set_section(
            smap,
            "redis",
            vec![
                ("host", cfg.redis_host.clone().into()),
                ("port", parse_int_or(&cfg.redis_port, 6379).into()),
                ("password", cfg.redis_password.clone().into()),
                ("db", parse_int_or(&cfg.redis_db, 0).into()),
            ],
        );
        set_section(
            smap,
            "d1",
            vec![
                ("account_id", cfg.d1_account_id.clone().into()),
                ("database_id", cfg.d1_database_id.clone().into()),
                ("api_token", cfg.d1_api_token.clone().into()),
                ("base_url", cfg.d1_base_url.clone().into()),
            ],
        );
    }

    let gateway = map
        .entry("gateway".into())
        .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(gmap) = gateway.as_mapping_mut() {
        gmap.insert("url".into(), cfg.gateway_url.clone().into());
    }

    // Desktop round-trip: write individual fields at top level so the Settings
    // UI can read them back via config_from_value.
    map.insert(
        "extra_instructions".into(),
        cfg.extra_instructions.clone().into(),
    );
    map.insert("system_prompt".into(), cfg.system_prompt.clone().into());

    let tools = map
            .entry("tools".into())
            .or_insert_with(|| serde_norway::Value::Mapping(Default::default()));
    if let Some(tmap) = tools.as_mapping_mut() {
            set_section(
                tmap,
                "schedule",
                vec![
                    ("enabled", cfg.schedule_enabled.into()),
                ],
            );
    }

    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// Merge only the top-level `default_model` key into the existing config text.
pub(crate) fn merge_default_model(existing: Option<&str>, model: &str) -> Result<String, String> {
    let mut yaml = config_mapping(existing);
    let map = yaml.as_mapping_mut().ok_or("yaml root is not a mapping")?;
    map.insert("default_model".into(), model.to_string().into());
    serde_norway::to_string(&yaml).map_err(|e| e.to_string())
}

/// Read ~/.infer/config.yaml, apply `f` to its text, and write the result back,
/// creating the parent dir as needed.
pub(crate) fn update_config_file(
    f: impl FnOnce(Option<&str>) -> Result<String, String>,
) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existing = std::fs::read_to_string(&path).ok();
    let text = f(existing.as_deref())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn get_config() -> Result<DesktopConfig, String> {
    Ok(read_config())
}

#[tauri::command]
pub(crate) async fn set_config(cfg: DesktopConfig) -> Result<(), String> {
    update_config_file(|existing| merge_config(existing, &cfg))
}

#[tauri::command]
pub(crate) async fn set_default_model(model: String) -> Result<(), String> {
    update_config_file(|existing| merge_default_model(existing, &model))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_yaml(text: &str) -> serde_norway::Value {
        serde_norway::from_str(text).unwrap()
    }

    fn str_field<'a>(val: &'a serde_norway::Value, path: &[&str]) -> Option<&'a str> {
        let mut cur = val;
        for key in path {
            cur = cur.get(key)?;
        }
        cur.as_str()
    }

    #[test]
    fn config_from_value_reads_fields_and_expands_tilde() {
        let home = PathBuf::from("/home/tester");
        let val = parse_yaml(
            "storage:\n  type: postgres\n  jsonl:\n    path: ~/conv\n  sqlite:\n    path: /db/conv.db\n  postgres:\n    host: db.example\n    port: 6543\n    database: mydb\n    username: alice\n    password: secret\n    ssl_mode: require\n  redis:\n    host: cache\n    port: 6380\n    password: rpw\n    db: 3\n  d1:\n    account_id: acc\n    database_id: dbid\n    api_token: tok\n    base_url: https://d1.example\ngateway:\n  url: http://gw:9000\ndefault_model: gpt-x\n",
        );
        let cfg = config_from_value(&val, &home);
        assert_eq!(cfg.storage_backend, "postgres");
        assert_eq!(cfg.storage_directory, "/home/tester/conv");
        assert_eq!(cfg.gateway_url, "http://gw:9000");
        assert_eq!(cfg.default_model, "gpt-x");
        assert_eq!(cfg.sqlite_path, "/db/conv.db");
        assert_eq!(cfg.postgres_host, "db.example");
        assert_eq!(cfg.postgres_port, "6543");
        assert_eq!(cfg.postgres_database, "mydb");
        assert_eq!(cfg.postgres_username, "alice");
        assert_eq!(cfg.postgres_password, "secret");
        assert_eq!(cfg.postgres_ssl_mode, "require");
        assert_eq!(cfg.redis_host, "cache");
        assert_eq!(cfg.redis_port, "6380");
        assert_eq!(cfg.redis_password, "rpw");
        assert_eq!(cfg.redis_db, "3");
        assert_eq!(cfg.d1_account_id, "acc");
        assert_eq!(cfg.d1_database_id, "dbid");
        assert_eq!(cfg.d1_api_token, "tok");
        assert_eq!(cfg.d1_base_url, "https://d1.example");
    }

    #[test]
    fn config_from_value_defaults_when_missing() {
        let home = PathBuf::from("/home/tester");
        let cfg = config_from_value(&serde_norway::Value::Mapping(Default::default()), &home);
        assert_eq!(cfg.storage_backend, "jsonl");
        assert_eq!(cfg.storage_directory, "/home/tester/.infer/conversations");
        assert_eq!(cfg.gateway_url, "http://localhost:8080");
        assert_eq!(cfg.default_model, "");
        assert_eq!(cfg.postgres_host, "localhost");
        assert_eq!(cfg.postgres_port, "5432");
        assert_eq!(cfg.postgres_database, "infer_conversations");
        assert_eq!(cfg.postgres_username, "");
        assert_eq!(cfg.postgres_password, "");
        assert_eq!(cfg.postgres_ssl_mode, "prefer");
        assert_eq!(cfg.sqlite_path, ".infer/conversations.db");
        assert_eq!(cfg.redis_host, "localhost");
        assert_eq!(cfg.redis_port, "6379");
        assert_eq!(cfg.redis_db, "0");
        assert_eq!(cfg.d1_base_url, "https://api.cloudflare.com/client/v4");
    }

    #[test]
    fn merge_config_preserves_unrelated_fields_and_skips_default_model() {
        let existing = "gateway:\n  run: true\n  standalone_binary: /x\nother_top: keep\nstorage:\n  backend: jsonl\n  directory: /old\n";
        let mut cfg = default_config();
        cfg.storage_backend = "postgres".into();
        cfg.storage_directory = "/data".into();
        cfg.gateway_url = "http://gw".into();
        cfg.default_model = "ignored".into();
        cfg.postgres_host = "pg.host".into();
        cfg.postgres_port = "6543".into();
        let val = parse_yaml(&merge_config(Some(existing), &cfg).unwrap());
        assert_eq!(str_field(&val, &["other_top"]), Some("keep"));
        assert_eq!(
            val.get("gateway")
                .and_then(|g| g.get("run"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            str_field(&val, &["gateway", "standalone_binary"]),
            Some("/x")
        );
        assert_eq!(str_field(&val, &["storage", "type"]), Some("postgres"));
        assert_eq!(
            val.get("storage")
                .and_then(|s| s.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            str_field(&val, &["storage", "jsonl", "path"]),
            Some("/data")
        );
        assert_eq!(
            str_field(&val, &["storage", "postgres", "host"]),
            Some("pg.host")
        );
        assert_eq!(
            val.get("storage")
                .and_then(|s| s.get("postgres"))
                .and_then(|p| p.get("port"))
                .and_then(|v| v.as_i64()),
            Some(6543)
        );
        assert!(val.get("storage").and_then(|s| s.get("backend")).is_none());
        assert!(
            val.get("storage")
                .and_then(|s| s.get("directory"))
                .is_none()
        );
        assert_eq!(str_field(&val, &["gateway", "url"]), Some("http://gw"));
        assert!(val.get("default_model").is_none());
        assert!(val.get("prompts").is_none());
    }

    #[test]
    fn merge_config_writes_every_backend_section() {
        let mut cfg = default_config();
        cfg.storage_backend = "redis".into();
        cfg.sqlite_path = "/db/c.db".into();
        cfg.redis_host = "cache".into();
        cfg.redis_port = "6380".into();
        cfg.redis_db = "3".into();
        cfg.d1_account_id = "acc".into();
        let val = parse_yaml(&merge_config(None, &cfg).unwrap());
        let int_at = |path: &[&str]| -> Option<i64> {
            let mut cur = &val;
            for key in path {
                cur = cur.get(key)?;
            }
            cur.as_i64()
        };
        assert_eq!(str_field(&val, &["storage", "type"]), Some("redis"));
        assert_eq!(
            str_field(&val, &["storage", "sqlite", "path"]),
            Some("/db/c.db")
        );
        assert_eq!(
            str_field(&val, &["storage", "redis", "host"]),
            Some("cache")
        );
        assert_eq!(int_at(&["storage", "redis", "port"]), Some(6380));
        assert_eq!(int_at(&["storage", "redis", "db"]), Some(3));
        assert_eq!(
            str_field(&val, &["storage", "d1", "account_id"]),
            Some("acc")
        );
        assert_eq!(
            str_field(&val, &["storage", "d1", "base_url"]),
            Some("https://api.cloudflare.com/client/v4")
        );
    }

    #[test]
    fn merge_default_model_sets_only_that_key() {
        let existing = "storage:\n  type: jsonl\n";
        let val = parse_yaml(&merge_default_model(Some(existing), "claude-x").unwrap());
        assert_eq!(str_field(&val, &["default_model"]), Some("claude-x"));
        assert_eq!(str_field(&val, &["storage", "type"]), Some("jsonl"));
    }

    #[test]
    fn merge_config_replaces_non_mapping_root() {
        let cfg = default_config();
        let val = parse_yaml(&merge_config(Some("just a scalar"), &cfg).unwrap());
        assert_eq!(str_field(&val, &["storage", "type"]), Some("jsonl"));
    }
}
