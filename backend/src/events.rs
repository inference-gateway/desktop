pub(crate) const AGENT_EVENT: &str = "agent-event";
pub(crate) const BROWSER_BRIDGE: &str = "browser-bridge";
pub(crate) const GIT_CHANGED: &str = "git-changed";
pub(crate) const PROJECT_CHANGED: &str = "project-changed";

/// The agent event kinds the monitor bar and the overlay act on. Only these
/// go out on [`AGENT_EVENT`] - the chat gets every kind on its own channel.
pub(crate) const BROADCAST_KINDS: [&str; 9] = [
    "AgentError",
    "ApprovalRequest",
    "AssistantMessage",
    "Cancelled",
    "ComputerUseAction",
    "Done",
    "RecordingStarted",
    "RecordingStopped",
    "ToolResult",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The names `frontend/lib/events.ts` lists under `BACKEND_EVENTS`.
    fn frontend_names() -> BTreeSet<String> {
        let source = include_str!("../../frontend/lib/events.ts");
        let (_, after) = source.split_once("BACKEND_EVENTS = {").unwrap();
        let (block, _) = after.split_once('}').unwrap();
        block
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    /// The variants of `AgentEvent`, the kinds the backend sends.
    fn agent_event_kinds() -> BTreeSet<String> {
        let source = include_str!("cli/events.rs");
        let (_, after) = source.split_once("pub(crate) enum AgentEvent {").unwrap();
        let (block, _) = after.split_once("\n}").unwrap();
        block
            .lines()
            .filter_map(|line| line.strip_prefix("    "))
            .filter(|line| line.starts_with(|c: char| c.is_ascii_uppercase()))
            .map(|line| {
                line.split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    /// The quoted names a window's source compares an event kind with.
    fn kinds_compared_in(source: &str) -> BTreeSet<String> {
        ["kind === \"", "kind !== \"", "case \""]
            .iter()
            .flat_map(|marker| source.split(marker).skip(1))
            .filter_map(|rest| rest.split_once('"'))
            .map(|(name, _)| name.to_owned())
            .collect()
    }

    #[test]
    fn the_other_windows_get_exactly_the_kinds_they_read() {
        let sources = [
            include_str!("../../frontend/monitor.tsx"),
            include_str!("../../frontend/lib/monitor-state.ts"),
            include_str!("../../frontend/overlay.tsx"),
        ];
        let sent = agent_event_kinds();
        let read: BTreeSet<String> = sources
            .iter()
            .flat_map(|source| kinds_compared_in(source))
            .filter(|kind| sent.contains(kind))
            .collect();
        let broadcast: BTreeSet<String> = BROADCAST_KINDS.map(str::to_owned).into();
        assert_eq!(read, broadcast);
    }

    #[test]
    fn the_frontend_listens_for_exactly_the_events_the_backend_emits() {
        let emitted: BTreeSet<String> = [AGENT_EVENT, BROWSER_BRIDGE, GIT_CHANGED, PROJECT_CHANGED]
            .map(str::to_owned)
            .into();
        assert_eq!(frontend_names(), emitted);
    }
}
