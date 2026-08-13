// Skill install/list: thin wrappers over `infer skills`, which owns download,
// validation, and the ~/.infer/skills layout.
use crate::agent::run_infer;
use crate::env::home_dir;

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
pub(crate) fn list_installed_skills() -> Vec<String> {
    let dir = home_dir().join(".infer").join("skills");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
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
