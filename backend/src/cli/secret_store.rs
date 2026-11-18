use crate::cli::config;
use crate::env::{home_dir, infer_dir_in};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SECRETS_FILE: &str = "secrets.yaml";

/// A secret as secrets.yaml keeps it. Its value lives in auth.yaml, and the
/// workspaces it is assigned to list its Kubernetes Secret in daemon.yaml.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct StoredSecret {
    pub(crate) label: String,
    pub(crate) variable: String,
    pub(crate) computer: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct SecretsFile {
    secrets: Vec<StoredSecret>,
}

pub(crate) fn secrets_path(home: &Path) -> PathBuf {
    infer_dir_in(home).join(SECRETS_FILE)
}

fn parse_stored(text: Option<&str>) -> Vec<StoredSecret> {
    text.and_then(|text| serde_norway::from_str::<SecretsFile>(text).ok())
        .map(|file| file.secrets)
        .unwrap_or_default()
}

pub(crate) fn read_stored_in(home: &Path) -> Vec<StoredSecret> {
    parse_stored(std::fs::read_to_string(secrets_path(home)).ok().as_deref())
}

pub(crate) fn write_stored_in(home: &Path, secrets: Vec<StoredSecret>) -> Result<(), String> {
    let text = serde_norway::to_string(&SecretsFile { secrets }).map_err(|e| e.to_string())?;
    config::update_file(&secrets_path(home), |_| Ok(text))
}

/// The secrets assigned to this computer, as (variable, value) pairs for every
/// infer process the desktop starts. One without a value is left out.
pub(crate) fn computer_env() -> Vec<(String, String)> {
    computer_env_in(&home_dir())
}

fn computer_env_in(home: &Path) -> Vec<(String, String)> {
    read_stored_in(home)
        .into_iter()
        .filter(|secret| secret.computer)
        .map(|secret| {
            let value = config::secret_value_in(home, &secret.label);
            (secret.variable, value)
        })
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn stored(label: &str, variable: &str, computer: bool) -> StoredSecret {
        StoredSecret {
            label: label.into(),
            variable: variable.into(),
            computer,
        }
    }

    #[test]
    fn only_secrets_assigned_to_this_computer_reach_its_processes() {
        let home = TempDir::new("secrets-env");
        let secrets = vec![
            stored("github", "GITHUB_TOKEN", true),
            stored("reviewer-inbox", "AGENTMAIL_API_KEY", false),
            stored("blank", "BLANK", true),
        ];
        write_stored_in(&home, secrets.clone()).unwrap();
        assert_eq!(read_stored_in(&home), secrets);
        config::set_secret_value_in(&home, "github", "ghp_token").unwrap();
        config::set_secret_value_in(&home, "reviewer-inbox", "am_inbox").unwrap();
        assert_eq!(
            computer_env_in(&home),
            vec![("GITHUB_TOKEN".to_string(), "ghp_token".to_string())]
        );
        let file = std::fs::read_to_string(secrets_path(&home)).unwrap();
        assert!(!file.contains("ghp_token") && !file.contains("am_inbox"));
    }
}
