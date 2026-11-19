use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DAEMON_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const DAEMON_EXIT_POLL: Duration = Duration::from_millis(100);

pub(crate) const PROCESS_MATCH: &str = "inference-gateway-desktop";

/// What a test wants on disk in the fresh HOME before the app starts.
#[derive(Clone, Copy, Default)]
pub struct Seeds<'a> {
    pub content_project: Option<&'a str>,
    pub workspace_agent: Option<&'a str>,
    pub model_agent: Option<&'a str>,
    pub project_agent: Option<&'a str>,
    pub voice_sample: Option<&'a str>,
    pub speech_model: Option<&'a str>,
    pub avatar: Option<&'a str>,
    pub hub_model: Option<&'a str>,
}

/// Wipe a mock run's HOME and write what the test wants on disk into it
/// before the app starts.
pub fn prepare(home: &Path, repo_root: &Path, seeds: Seeds, infer: Option<&Path>) -> Result<()> {
    let _ = std::fs::remove_dir_all(home);
    std::fs::create_dir_all(home)?;
    if let Some(name) = seeds.content_project {
        seed_content_project(home, name, repo_root)?;
    }
    if let Some(name) = seeds.workspace_agent {
        seed_workspace_agent(home, name)?;
    }
    if let Some(name) = seeds.model_agent {
        seed_model_agent(home, name)?;
    }
    if let Some(name) = seeds.project_agent {
        let project = seeds
            .content_project
            .context("project_agent needs a content_project to default to")?;
        seed_project_agent(home, name, project)?;
    }
    if let Some(name) = seeds.avatar {
        seed_avatar(home, name, repo_root)?;
    }
    if let Some(id) = seeds.hub_model {
        seed_hub_model(home, id)?;
    }
    if let Some(name) = seeds.voice_sample {
        let infer = infer.context("voice_sample needs an infer binary to speak it")?;
        seed_voice_sample(home, name, seeds.speech_model, infer)?;
    }
    Ok(())
}

/// Offline fallback: the infer binary from the dev environment's PATH (the
/// flox-pinned version), so e2e never silently tests a stale ~/.infer/bin
/// install. The primary path is the tested release the runner downloads.
pub fn which_infer() -> Option<PathBuf> {
    let out = Command::new("which").arg("infer").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Write a content project into a fresh HOME before the app reads it. Only
/// `types:` needs seeding (a code project is creatable through the UI) and
/// `paths:` pins the directory. A real image goes into the project's media
/// folder with it: the AX driver cannot drag a file out of Finder, so an image
/// asset has to start on disk the way `import_project_file` lands one.
fn seed_content_project(home: &Path, name: &str, repo_root: &Path) -> Result<()> {
    let dir = home.join("projects").join(name);
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(home.join(".infer"))?;
    let media = dir.join("media");
    std::fs::create_dir_all(&media)?;
    std::fs::copy(
        repo_root.join("frontend/public/logo.png"),
        media.join("logo.png"),
    )
    .context("seeding the project's media folder")?;
    std::fs::write(
        home.join(".infer").join("projects.yaml"),
        format!(
            "names:\n  - \"{name}\"\ntypes:\n  \"{name}\": content\npaths:\n  \"{name}\": \"{dir}\"\n",
            dir = dir.display(),
        ),
    )?;
    Ok(())
}

/// An agent whose chats run on the workspace mock mode lists.
fn seed_workspace_agent(home: &Path, name: &str) -> Result<()> {
    let agents = home.join(".infer").join("agents");
    std::fs::create_dir_all(&agents)?;
    std::fs::write(
        agents.join(format!("{name}.md")),
        format!(
            "---\nname: {name}\ndescription: Works on a computer of its own\nworkspace: mock-computer\n---\n\nYou work on your own computer.\n"
        ),
    )?;
    Ok(())
}

/// The model the seeded model agent's front matter fixes - a mock model the
/// default (`openai/gpt-4o`) is not, so a scenario can tell the two apart.
const AGENT_MODEL: &str = "anthropic/claude-sonnet-4-5";

fn seed_model_agent(home: &Path, name: &str) -> Result<()> {
    let agents = home.join(".infer").join("agents");
    std::fs::create_dir_all(&agents)?;
    std::fs::write(
        agents.join(format!("{name}.md")),
        format!(
            "---\nname: {name}\ndescription: Answers on a model of its own\nmodel: {AGENT_MODEL}\n---\n\nYou answer on your own model.\n"
        ),
    )?;
    Ok(())
}

/// An agent whose new chats open in `project`.
fn seed_project_agent(home: &Path, name: &str, project: &str) -> Result<()> {
    let agents = home.join(".infer").join("agents");
    std::fs::create_dir_all(&agents)?;
    std::fs::write(
        agents.join(format!("{name}.md")),
        format!(
            "---\nname: {name}\ndescription: Works in a project of its own\nproject: {project}\n---\n\nYou work in your project.\n"
        ),
    )?;
    Ok(())
}

/// The CLI config of a HOME that has text to speech on, on the speech model
/// a test names or on the CLI's own default.
fn speech_config(model: Option<&str>) -> String {
    let model = model.map(|model| format!("  model: {model}\n"));
    format!(
        "text_to_speech:\n  enabled: true\n{}",
        model.unwrap_or_default()
    )
}

/// A voice sample in the library of a HOME that has text to speech on. The
/// mock gateway speaks it, so no audio is made up here.
fn seed_voice_sample(home: &Path, name: &str, model: Option<&str>, infer: &Path) -> Result<()> {
    let infer_home = home.join(".infer");
    let samples = infer_home.join("voice").join("samples");
    std::fs::create_dir_all(&samples)?;
    std::fs::write(infer_home.join("config.yaml"), speech_config(model))?;
    let sample = format!("{name}.wav");
    Command::new(infer)
        .current_dir(home)
        .env("HOME", home)
        .env("INFER_GATEWAY_MOCK", "true")
        .env("INFER_TEXT_TO_SPEECH_OUTPUT_DIR", &samples)
        .args(["tools", "execute", "TextToSpeech"])
        .arg(format!(
            r#"{{"text":"A voice sample.","output_path":"{sample}"}}"#
        ))
        .arg("--approved")
        .output()
        .context("speaking the voice sample through the mock gateway")?;
    if !samples.join(&sample).exists() {
        bail!("the mock gateway did not speak the voice sample {sample}");
    }
    Ok(())
}

/// An avatar in the library of a HOME that can make it talk: text to speech
/// and text to video are both on, and the portrait is the app's own logo.
fn seed_avatar(home: &Path, name: &str, repo_root: &Path) -> Result<()> {
    let infer_home = home.join(".infer");
    let avatar = infer_home.join("avatars").join(name);
    std::fs::create_dir_all(&avatar)?;
    std::fs::copy(
        repo_root.join("frontend/public/logo.png"),
        avatar.join("01-front.png"),
    )
    .context("seeding the avatar's portrait")?;
    std::fs::write(
        infer_home.join("config.yaml"),
        "text_to_speech:\n  enabled: true\ntext_to_video:\n  enabled: true\n",
    )?;
    Ok(())
}

/// A model in the library of downloaded Hugging Face models, one small file of it.
fn seed_hub_model(home: &Path, id: &str) -> Result<()> {
    let folder = home
        .join(".infer")
        .join("models")
        .join("huggingface")
        .join(id);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(folder.join("config.json"), "{}\n")?;
    Ok(())
}

/// Stops the `infer daemon` the app started under `home`, by the pid file the
/// daemon keeps, so the next run talks to a daemon started with its own HOME.
pub fn stop_daemon(home: &Path) {
    let pid_file = home.join(".infer").join("run").join("daemon.pid");
    let Some(pid) = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    else {
        return;
    };
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status();
    let deadline = Instant::now() + DAEMON_EXIT_TIMEOUT;
    while Instant::now() < deadline {
        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !alive {
            break;
        }
        std::thread::sleep(DAEMON_EXIT_POLL);
    }
    let _ = std::fs::remove_file(pid_file);
}
