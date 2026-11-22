use crate::shard::Shard;
use crate::spec::Platform;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The system a run's summary names, the one the runner was built for.
pub const THIS_SYSTEM: &str = if cfg!(target_os = "linux") {
    "Linux"
} else {
    "macOS"
};

/// How one test ended. A failure keeps the step that failed and why.
pub enum Outcome {
    Passed,
    Failed(String),
    Skipped(Platform),
}

/// One test of a run, named as its YAML file names it.
pub struct TestResult {
    pub name: String,
    pub file: PathBuf,
    pub outcome: Outcome,
}

/// How many tests of a run passed, failed and were skipped.
pub struct Tally {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

pub fn tally(results: &[TestResult]) -> Tally {
    let count =
        |wanted: fn(&Outcome) -> bool| results.iter().filter(|r| wanted(&r.outcome)).count();
    Tally {
        passed: count(|o| matches!(o, Outcome::Passed)),
        failed: count(|o| matches!(o, Outcome::Failed(_))),
        skipped: count(|o| matches!(o, Outcome::Skipped(_))),
    }
}

/// Whether the run is a GitHub Actions job, whose log reads workflow commands.
pub fn on_github() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true")
}

/// The workflow command that marks a failed test's file, so the failure shows
/// on the run page and in a pull request's checks without opening the log.
pub fn annotation(file: &Path, name: &str, reason: &str) -> String {
    format!(
        "::error file={},title={}::{}",
        property(&file.display().to_string()),
        property(&format!("E2E failed - {name}")),
        message(reason)
    )
}

/// `file` relative to the repository root, the form an annotation needs, or
/// as given when it lies outside the repository.
pub fn repo_relative(file: &Path, repo_root: &Path) -> PathBuf {
    std::path::absolute(file)
        .ok()
        .and_then(|path| path.strip_prefix(repo_root).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| file.to_path_buf())
}

fn message(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn property(text: &str) -> String {
    message(text).replace(':', "%3A").replace(',', "%2C")
}

/// The run as markdown for the job summary: a headline with the counts, the
/// failing tests with the step that failed and why, and the skipped tests.
pub fn summary(
    results: &[TestResult],
    repo_root: &Path,
    shard: Option<Shard>,
    took: &str,
) -> String {
    let Tally {
        passed,
        failed,
        skipped,
    } = tally(results);
    let mark = if failed > 0 { "❌" } else { "✅" };
    let scope = shard.map_or(String::new(), |s| format!(" shard {s}"));
    let mut counts = Vec::new();
    if failed > 0 {
        counts.push(format!("{failed} failed"));
    }
    counts.push(format!("{passed} passed"));
    if skipped > 0 {
        counts.push(format!("{skipped} skipped"));
    }
    let mut out = format!(
        "### {mark} E2E {THIS_SYSTEM}{scope}: {} ({took})\n\n",
        counts.join(" · ")
    );
    if failed > 0 {
        out.push_str("| Test | File | Failure |\n| --- | --- | --- |\n");
        for result in results {
            if let Outcome::Failed(reason) = &result.outcome {
                out.push_str(&format!(
                    "| {} | `{}` | {} |\n",
                    cell(&result.name),
                    repo_relative(&result.file, repo_root).display(),
                    cell(reason)
                ));
            }
        }
        out.push_str(
            "\nEach failing test's screenshot and app log are in this job's artifact.\n\n",
        );
    }
    let skips: Vec<String> = results
        .iter()
        .filter_map(|r| match r.outcome {
            Outcome::Skipped(platform) => Some(format!("{} ({} only)", r.name, platform.label())),
            _ => None,
        })
        .collect();
    if !skips.is_empty() {
        out.push_str(&format!("Skipped: {}\n\n", skips.join(", ")));
    }
    out
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// Appends `markdown` to the job summary when the run is a GitHub Actions job.
/// A failed write only warns, so the summary never changes the run's result.
pub fn publish(markdown: &str) {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return;
    };
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(markdown.as_bytes()));
    if let Err(e) = written {
        println!("warning: could not write the job summary: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str, outcome: Outcome) -> TestResult {
        TestResult {
            name: name.into(),
            file: PathBuf::from(format!("/repo/e2e/tests/{name}.yaml")),
            outcome,
        }
    }

    #[test]
    fn an_annotation_escapes_what_would_end_its_properties_or_message() {
        let line = annotation(
            Path::new("e2e/tests/a.yaml"),
            "voice: input, live",
            "step 2: 100% done\nthen not",
        );
        assert_eq!(
            line,
            "::error file=e2e/tests/a.yaml,title=E2E failed - voice%3A input%2C live::step 2: 100%25 done%0Athen not"
        );
    }

    #[test]
    fn a_test_file_reads_relative_to_the_repository_root() {
        let root = Path::new("/repo");
        assert_eq!(
            repo_relative(Path::new("/repo/e2e/tests/a.yaml"), root),
            Path::new("e2e/tests/a.yaml")
        );
        assert_eq!(
            repo_relative(Path::new("/elsewhere/a.yaml"), root),
            Path::new("/elsewhere/a.yaml")
        );
    }

    #[test]
    fn a_green_run_summarises_as_one_line() {
        let results = [result("a", Outcome::Passed), result("b", Outcome::Passed)];
        assert_eq!(
            summary(&results, Path::new("/repo"), None, "4.2s"),
            format!("### ✅ E2E {THIS_SYSTEM}: 2 passed (4.2s)\n\n")
        );
    }

    #[test]
    fn a_failure_lists_its_file_and_the_step_that_failed() {
        let results = [
            result("a", Outcome::Passed),
            result(
                "b",
                Outcome::Failed("step 5 (wait_for text \"x | y\"): timed out".into()),
            ),
            result("c", Outcome::Skipped(Platform::Macos)),
        ];
        let text = summary(&results, Path::new("/repo"), "2/3".parse().ok(), "1m02s");
        assert!(text.starts_with(&format!(
            "### ❌ E2E {THIS_SYSTEM} shard 2/3: 1 failed · 1 passed · 1 skipped (1m02s)\n"
        )));
        assert!(text.contains(
            "| b | `e2e/tests/b.yaml` | step 5 (wait_for text \"x \\| y\"): timed out |\n"
        ));
        assert!(text.contains("Skipped: c (macOS only)\n"));
    }
}
