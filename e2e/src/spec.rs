//! YAML test specification.
//!
//! A test file has a `name`, an optional `cleanup` list (paths relative to
//! src-tauri/, the infer child's cwd, deleted before and after the run), and
//! a list of steps. Steps are either a bare verb (`- new_chat`) or a
//! single-key map (`- send: "..."`).

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub name: String,
    #[serde(default)]
    pub cleanup: Vec<PathBuf>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Step {
    Bare(BareStep),
    Send { send: String },
    WaitFor { wait_for: WaitTarget },
    Click { click: ClickTarget },
    AssertAbsent { assert_absent: FileTarget },
    AssertModel { assert_model: String },
    Screenshot { screenshot: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BareStep {
    NewChat,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitTarget {
    pub button: Option<String>,
    pub text: Option<String>,
    pub file: Option<PathBuf>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClickTarget {
    pub button: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileTarget {
    pub file: PathBuf,
}

fn default_timeout() -> u64 {
    15
}

pub fn load(path: &std::path::Path) -> anyhow::Result<Test> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading {}: {}", path.display(), e))?;
    serde_norway::from_str(&text).map_err(|e| anyhow::anyhow!("parsing {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    #[test]
    fn shipped_tests_parse() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut n = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "yaml") {
                let test = super::load(&path).unwrap();
                assert!(!test.steps.is_empty(), "{} has no steps", test.name);
                n += 1;
            }
        }
        assert!(
            n >= 2,
            "expected the two shipped example tests, found {}",
            n
        );
    }
}
