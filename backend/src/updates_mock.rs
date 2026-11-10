use crate::download::ProgressEvent;
use crate::updates::UpdateInfo;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const INSTALLED: &str = "0.1.0";
const RELEASED: &str = "0.2.0";
const DOWNLOAD_STEPS: u64 = 4;
const DOWNLOAD_STEP: Duration = Duration::from_millis(400);

static APPLIED: AtomicBool = AtomicBool::new(false);

/// What a check finds in mock mode: a CLI one release behind, until `apply`
/// has installed that release.
pub(crate) fn check() -> Vec<UpdateInfo> {
    let current = if APPLIED.load(Ordering::SeqCst) {
        RELEASED
    } else {
        INSTALLED
    };
    vec![UpdateInfo::new(
        "CLI",
        current.to_owned(),
        vec![format!("v{RELEASED}"), format!("v{INSTALLED}")],
    )]
}

/// Stands in for installing the release: nothing is downloaded, but the
/// progress a download reports is, slowly enough for the window to show it.
pub(crate) fn apply(report: impl Fn(ProgressEvent)) {
    apply_pausing(DOWNLOAD_STEP, report);
}

fn apply_pausing(step: Duration, report: impl Fn(ProgressEvent)) {
    for received in 0..=DOWNLOAD_STEPS {
        report(ProgressEvent::Downloading {
            received,
            total: DOWNLOAD_STEPS,
        });
        std::thread::sleep(step);
    }
    report(ProgressEvent::Installing);
    APPLIED.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn the_cli_is_outdated_until_the_update_is_applied() {
        let before = serde_json::to_value(check()).unwrap();
        assert_eq!(before[0]["name"], "CLI");
        assert_eq!(before[0]["current"], INSTALLED);
        assert_eq!(before[0]["latest"], RELEASED);
        assert_eq!(before[0]["outdated"], true);

        let reported = Mutex::new(Vec::new());
        apply_pausing(Duration::ZERO, |event| {
            reported
                .lock()
                .unwrap()
                .push(serde_json::to_value(event).unwrap());
        });

        let after = serde_json::to_value(check()).unwrap();
        assert_eq!(after[0]["current"], RELEASED);
        assert_eq!(after[0]["outdated"], false);
        let reported = reported.into_inner().unwrap();
        assert_eq!(reported.len() as u64, DOWNLOAD_STEPS + 2);
        assert_eq!(reported.last().unwrap()["kind"], "Installing");
    }
}
