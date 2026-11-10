pub(crate) const AGENT_EVENT: &str = "agent-event";
pub(crate) const BROWSER_BRIDGE: &str = "browser-bridge";
pub(crate) const GIT_CHANGED: &str = "git-changed";
pub(crate) const PROJECT_CHANGED: &str = "project-changed";

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

    #[test]
    fn the_frontend_listens_for_exactly_the_events_the_backend_emits() {
        let emitted: BTreeSet<String> = [AGENT_EVENT, BROWSER_BRIDGE, GIT_CHANGED, PROJECT_CHANGED]
            .map(str::to_owned)
            .into();
        assert_eq!(frontend_names(), emitted);
    }
}
