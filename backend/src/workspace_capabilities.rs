/// What a workspace's computer installs when it boots to browse: Firefox with
/// the opentask extension, wired to the pod's browser bridge.
const BROWSER_USER_DATA: &str = include_str!("computer_user_data.sh");

/// Something a workspace's agents may do on its computers. A workspace has it
/// when its environment holds every entry of `env` and its user data holds
/// `user_data`, so `daemon.yaml` stays the one place that records it.
#[derive(Debug, PartialEq, serde::Serialize)]
pub(crate) struct Capability {
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    pub(crate) env: &'static [(&'static str, &'static str)],
    pub(crate) user_data: &'static str,
}

/// A workspace's computer is a pod of its own, not the user's machine, so what
/// it is given runs without asking.
pub(crate) const CAPABILITIES: [Capability; 3] = [
    Capability {
        id: "computer-use",
        label: "Computer use",
        hint: "The agent sees the computer's screen and controls its apps.",
        env: &[
            ("INFER_COMPUTER_USE_ENABLED", "true"),
            ("INFER_COMPUTER_USE_APPROVAL", "never"),
        ],
        user_data: "",
    },
    Capability {
        id: "browser-use",
        label: "Browser use",
        hint: "The computer installs Firefox with the opentask extension when it boots. Needs an Ubuntu or Debian image.",
        env: &[],
        user_data: BROWSER_USER_DATA,
    },
    Capability {
        id: "recording",
        label: "Recording",
        hint: "The agent records the computer's screen with RecordStart and RecordStop, into the chat's artifacts.",
        env: &[
            ("INFER_COMPUTER_USE_RECORDING_ENABLED", "true"),
            ("INFER_COMPUTER_USE_RECORDING_REQUIRE_APPROVAL", "false"),
            (
                "INFER_COMPUTER_USE_RECORDING_OUTPUT_DIR",
                "/home/infer/.infer/artifacts",
            ),
        ],
        user_data: "",
    },
];

/// The capabilities the workspace form offers as switches.
#[tauri::command]
pub(crate) fn workspace_capabilities() -> &'static [Capability] {
    &CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn capabilities_have_distinct_ids_and_never_share_a_variable() {
        let ids: BTreeSet<_> = CAPABILITIES.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), CAPABILITIES.len());
        let names: Vec<_> = CAPABILITIES
            .iter()
            .flat_map(|c| c.env.iter().map(|(name, _)| *name))
            .collect();
        assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), names.len());
    }

    #[test]
    fn recording_registers_the_record_tools_without_an_approver() {
        let recording = CAPABILITIES.iter().find(|c| c.id == "recording").unwrap();
        assert!(
            recording
                .env
                .contains(&("INFER_COMPUTER_USE_RECORDING_ENABLED", "true"))
        );
        assert!(
            recording
                .env
                .contains(&("INFER_COMPUTER_USE_RECORDING_REQUIRE_APPROVAL", "false"))
        );
    }
}
