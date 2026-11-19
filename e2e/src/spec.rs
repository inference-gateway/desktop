use serde::Deserialize;
use std::path::PathBuf;

/// A YAML test file (tests/*.yaml): a `name`, optional cleanup paths, record
/// and narration flags, the agents, projects, voice samples and models seeded
/// into the fresh HOME before launch (the AX driver cannot drive the native
/// `<select>` pickers themselves), and the steps.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub name: String,
    #[serde(default)]
    pub cleanup: Vec<PathBuf>,
    #[serde(default)]
    pub record: bool,
    #[serde(default)]
    pub narration: Vec<String>,
    #[serde(default)]
    pub content_project: Option<String>,
    #[serde(default)]
    pub workspace_agent: Option<String>,
    #[serde(default)]
    pub model_agent: Option<String>,
    #[serde(default)]
    pub project_agent: Option<String>,
    #[serde(default)]
    pub voice_sample: Option<String>,
    #[serde(default)]
    pub speech_model: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub hub_model: Option<String>,
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
    WaitGone {
        wait_gone: GoneTarget,
    },
    Click {
        click: ClickTarget,
    },
    Drag {
        drag: DragTarget,
    },
    AssertOrder {
        assert_order: OrderTarget,
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

/// What `wait_for` waits to appear: any of `button`, `text` or `file`, with
/// `contains` also matching the file's contents, within `timeout`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitTarget {
    pub button: Option<String>,
    pub text: Option<String>,
    pub file: Option<PathBuf>,
    pub contains: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

/// What `wait_gone` waits to disappear. `never` is a button that must not
/// show up while `button` is still there, nor once it has gone.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoneTarget {
    pub button: String,
    pub never: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

/// What `drag` moves with the mouse: the element named `from` onto the element
/// named `to`, each matched by its accessible name or description. `from_dx`
/// shifts the press sideways from `from`'s centre, onto a handle with no name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DragTarget {
    pub from: String,
    #[serde(default)]
    pub from_dx: f64,
    pub to: String,
}

/// What `assert_order` checks: the element named `above` sits higher on screen
/// than the element named `below`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderTarget {
    pub above: String,
    pub below: String,
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
    /// Seconds to wait for the text to show up. Without it the check is immediate.
    #[serde(default)]
    pub timeout: u64,
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
                for line in std::iter::once(&test.name).chain(&test.narration) {
                    assert!(
                        !line.contains(';'),
                        "{}: split the sentence or use a dash instead of a semicolon: {line}",
                        test.name
                    );
                }
                names.push(test.name);
            }
        }
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "duplicate test names: {names:?}");
    }
}
