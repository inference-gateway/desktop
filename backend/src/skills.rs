use crate::agent::run_infer;
use crate::env::home_dir;

/// True when `name` is safe to pass to `infer skills install/uninstall`.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Names of the skills installed under `<home>/.infer/skills` (an entry owns a
/// SKILL.md), rooted at an arbitrary home so tests can use a temp dir.
pub(crate) fn installed_skills_in(home: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(home.join(".infer").join("skills")) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Skills shipped inside the app binary, written to ~/.infer/skills at startup
/// so the agent always finds them, whether or not the catalog copy is installed.
const BUNDLED_SKILLS: [(&str, &str); 2] = [
    (
        "video-editing",
        include_str!("../../.agents/skills/video-editing/SKILL.md"),
    ),
    (
        "desktop-projects",
        include_str!("../../.agents/skills/desktop-projects/SKILL.md"),
    ),
];

/// Write `<home>/.infer/skills/<name>/SKILL.md`, rewriting only when the
/// content differs from the bundled copy.
fn install_bundled_in(home: &std::path::Path, name: &str, contents: &str) -> Result<(), String> {
    let dir = home.join(".infer").join("skills").join(name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("SKILL.md");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(contents) {
        std::fs::write(&path, contents).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Install every bundled skill, logging rather than failing: a missing skill
/// degrades the agent, it should not block app startup.
pub(crate) fn install_bundled_skills() {
    let home = home_dir();
    for (name, contents) in BUNDLED_SKILLS {
        if let Err(e) = install_bundled_in(&home, name, contents) {
            eprintln!("installing bundled skill {name} failed: {e}");
        }
    }
}

#[tauri::command]
pub(crate) fn list_installed_skills() -> Vec<String> {
    installed_skills_in(&home_dir())
}

#[tauri::command]
pub(crate) async fn install_skill(name: String) -> Result<(), String> {
    if !valid_name(&name) {
        return Err(format!("invalid skill name: {name}"));
    }
    run_infer(&["skills", "install", &name, "--user"]).await?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn uninstall_skill(name: String) -> Result<(), String> {
    if !valid_name(&name) {
        return Err(format!("invalid skill name: {name}"));
    }
    run_infer(&["skills", "uninstall", &name, "--user"]).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BUNDLED_SKILLS, install_bundled_in, installed_skills_in, valid_name};

    #[test]
    fn name_validation() {
        assert!(valid_name("skill-creator"));
        assert!(valid_name("cpp_17"));
        assert!(!valid_name(""));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("--user"));
    }

    #[test]
    fn bundled_skills_install_and_refresh() {
        let home = std::env::temp_dir().join(format!("bundled-skills-{}", std::process::id()));
        for (name, contents) in BUNDLED_SKILLS {
            install_bundled_in(&home, name, contents).unwrap();
        }
        let mut names = installed_skills_in(&home);
        names.sort();
        assert_eq!(names, ["desktop-projects", "video-editing"]);
        let path = home.join(".infer/skills/desktop-projects/SKILL.md");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .starts_with("---\nname: desktop-projects\n")
        );
        std::fs::write(&path, "stale").unwrap();
        install_bundled_in(&home, "desktop-projects", "fresh").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh");
        std::fs::remove_dir_all(&home).unwrap();
    }
}
