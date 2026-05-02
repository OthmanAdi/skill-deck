// @agent-context: Skill scanning commands exposed to the frontend.
// These are the primary data-fetching commands the UI calls on startup and refresh.

use crate::agents::{detect_installed_agents, scan_all_skills, scan_custom_paths};
use crate::commands::preferences::ConfigState;
use crate::detection::repo_detector::{apply_overrides, DetectedSource};
use crate::models::{AgentInfo, AppConfig, ScanResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::State;

const PROJECT_ROOT_MARKERS: &[&str] = &[
    ".git",
    ".jj",
    ".hg",
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".agents",
    ".claude",
    ".codex",
    ".cursor",
    ".github",
    "package.json",
    "pnpm-workspace.yaml",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
];

/// Resolve a terminal CWD to the nearest project root.
/// If no marker is found, the existing directory itself is still a valid scan root.
pub(crate) fn resolve_project_root(project_path: Option<&str>) -> Option<PathBuf> {
    let raw = project_path?.trim();
    if raw.is_empty() {
        return None;
    }

    let input = Path::new(raw);
    let canonical = std::fs::canonicalize(input).ok()?;
    let start = if canonical.is_dir() {
        canonical
    } else {
        canonical.parent()?.to_path_buf()
    };

    let home = dirs::home_dir().and_then(|p| std::fs::canonicalize(p).ok());
    let mut cursor = Some(start.as_path());
    while let Some(dir) = cursor {
        if home.as_deref() == Some(dir) && dir != start.as_path() {
            break;
        }
        if PROJECT_ROOT_MARKERS
            .iter()
            .any(|marker| dir.join(marker).exists())
        {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }

    Some(start)
}

/// Shared scan pipeline used by commands that need the exact same skill IDs as the UI.
pub(crate) fn scan_with_config(config: &AppConfig, project_root: Option<&Path>) -> ScanResult {
    let mut result = scan_all_skills(project_root);

    let (custom_skills, custom_errors) = scan_custom_paths(&config.custom_scan_paths);
    result.skills.extend(custom_skills);
    result.errors.extend(custom_errors);

    let mut seen_ids: HashSet<String> = HashSet::new();
    for skill in &mut result.skills {
        if !seen_ids.insert(skill.id.clone()) {
            let mut counter = 2;
            loop {
                let candidate = format!("{}:{}", skill.id, counter);
                if seen_ids.insert(candidate.clone()) {
                    skill.id = candidate;
                    break;
                }
                counter += 1;
            }
        }

        let mut detected = DetectedSource {
            repository_url: skill.metadata.repository_url.clone(),
            install_command: skill.metadata.install_command.clone(),
        };

        apply_overrides(
            &mut detected,
            &skill.id,
            &config.skill_repo_overrides,
            &config.skill_install_overrides,
        );

        skill.metadata.repository_url = detected.repository_url;
        skill.metadata.install_command = detected.install_command;

        if let Some(icon) = config.skill_icons.get(&skill.id) {
            skill.icon = Some(icon.clone());
        }
        skill.starred = config.starred_skills.contains(&skill.id);

        if let Some(cached) = config.update_check_cache.get(&skill.id) {
            skill.update_available = cached.update_available;
        }
    }

    result
}

/// Scan all installed agents for skills.
/// Called on app startup and when the user refreshes.
///
/// `project_path`: Optional current project directory (from CWD detection).
/// If provided, includes project-scoped skills for that directory.
#[tauri::command]
pub fn scan_skills(state: State<'_, ConfigState>, project_path: Option<String>) -> ScanResult {
    let config = match state.0.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    let project_root = resolve_project_root(project_path.as_deref());
    scan_with_config(&config, project_root.as_deref())
}

/// List all known agents and whether they're installed.
/// Used to populate the agent filter tabs in the UI.
#[tauri::command]
pub fn list_agents(project_path: Option<String>) -> Vec<AgentInfo> {
    let project_root = resolve_project_root(project_path.as_deref());
    detect_installed_agents(project_root.as_deref())
}

/// Read the raw content of a skill file.
/// Used when the user clicks a card to see the full skill body.
#[tauri::command]
pub fn read_skill_content(
    state: State<'_, ConfigState>,
    skill_id: String,
    project_path: Option<String>,
) -> Result<String, String> {
    let config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?
        .clone();
    let project_root = resolve_project_root(project_path.as_deref());
    let scan = scan_with_config(&config, project_root.as_deref());

    let skill = scan
        .skills
        .into_iter()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;

    let canonical = std::fs::canonicalize(&skill.file_path)
        .map_err(|e| format!("Failed to resolve skill path {}: {}", skill.file_path, e))?;

    std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read {}: {}", canonical.to_string_lossy(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_project_root_walks_up_from_nested_cwd() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        let nested = root.join("src").join("lib");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("AGENTS.md"), "rules").unwrap();

        let resolved = resolve_project_root(Some(nested.to_str().unwrap())).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(root).unwrap());
    }

    #[test]
    fn resolve_project_root_keeps_existing_directory_without_markers() {
        let temp = tempfile::tempdir().unwrap();
        let resolved = resolve_project_root(Some(temp.path().to_str().unwrap())).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(temp.path()).unwrap());
    }

    #[test]
    fn resolve_project_root_rejects_missing_path() {
        assert!(resolve_project_root(Some("C:/definitely/not/here")).is_none());
    }
}
