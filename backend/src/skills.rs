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
    use super::valid_name;

    #[test]
    fn name_validation() {
        assert!(valid_name("skill-creator"));
        assert!(valid_name("cpp_17"));
        assert!(!valid_name(""));
        assert!(!valid_name("../evil"));
        assert!(!valid_name("a b"));
        assert!(!valid_name("--user"));
    }
}
