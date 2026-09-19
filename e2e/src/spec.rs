//! YAML test specification.
//!
//! A test file has a `name`, an optional `cleanup` list (paths relative to
//! the repo root, the infer child's cwd, deleted before and after the run), an
//! optional `record` flag (screen-record the run into artifacts/<slug>.mov), an
//! optional `narration` list (one line per step index, printed by the runner so
//! the file reads top to bottom as a walkthrough of the feature it demos), and
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
    /// Wrap the run in `screencapture -v` and write artifacts/<slug>.mov.
    #[serde(default)]
    pub record: bool,
    /// One walkthrough line per step; the runner prints it in place of the
    /// mechanical step label. Missing entries fall back to the label.
    #[serde(default)]
    pub narration: Vec<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Step {
    Bare(BareStep),
    Send {
        send: String,
    },
    Keypress {
        keypress: String,
    },
    Type {
        #[serde(rename = "type")]
        text: String,
    },
    WaitFor {
        wait_for: WaitTarget,
    },
    Click {
        click: ClickTarget,
    },
    AssertAbsent {
        assert_absent: FileTarget,
    },
    AssertAboveComposer {
        assert_above_composer: ClickTarget,
    },
    AssertModel {
        assert_model: String,
    },
    AssertComposer {
        assert_composer: TextTarget,
    },
    Screenshot {
        screenshot: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BareStep {
    NewChat,
    AssertOverlayBounds,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitTarget {
    pub button: Option<String>,
    pub text: Option<String>,
    pub file: Option<PathBuf>,
    /// With `file`: also wait until the file's contents include this string.
    pub contains: Option<String>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextTarget {
    pub text: String,
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
        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "yaml") {
                let test = super::load(&path).unwrap();
                assert!(!test.steps.is_empty(), "{} has no steps", test.name);
                assert!(
                    test.narration.len() <= test.steps.len(),
                    "{} narrates {} steps but has {}",
                    test.name,
                    test.narration.len(),
                    test.steps.len()
                );
                names.push(test.name);
            }
        }
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "duplicate test names: {names:?}");
    }
}
