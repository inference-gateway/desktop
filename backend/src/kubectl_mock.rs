use crate::workspace_settings::{MACHINES, MANAGED_LABEL, ORCHESTRATORS, REQUIREMENTS};
use std::sync::atomic::{AtomicBool, Ordering};

const MACHINE: &str = "infer-mock-broken";
static MACHINE_DELETED: AtomicBool = AtomicBool::new(false);

/// What kubectl would print for `args` against a cluster that has everything
/// sessions need and holds one broken machine, until that machine is deleted.
/// Anything else prints nothing, as a command that changed nothing does.
pub(crate) fn output(args: &[&str]) -> String {
    match args {
        ["get", "crd", ..] => REQUIREMENTS
            .iter()
            .flat_map(|requirement| requirement.crds)
            .map(|crd| format!("customresourcedefinition.apiextensions.k8s.io/{crd}\n"))
            .collect(),
        ["get", "namespace", ..] => format!(r#"{{"{MANAGED_LABEL}":"true"}}"#),
        ["get", ORCHESTRATORS, ..] if args.contains(&MACHINES) => {
            if MACHINE_DELETED.load(Ordering::Relaxed) {
                String::new()
            } else {
                format!("{MACHINE}\tFalse\tSandboxNotReady\n")
            }
        }
        ["delete", ORCHESTRATORS, MACHINE, ..] => {
            MACHINE_DELETED.store(true, Ordering::Relaxed);
            String::new()
        }
        _ => String::new(),
    }
}
