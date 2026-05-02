// @agent-context: Skill scanning commands exposed to the frontend.
// These are the primary data-fetching commands the UI calls on startup and refresh.

use crate::agents::{detect_installed_agents, scan_all_skills, scan_custom_paths};
use crate::commands::preferences::ConfigState;
use crate::detection::repo_detector::{apply_overrides, DetectedSource};
use crate::models::{AgentInfo, ScanResult};
use std::collections::HashSet;
use std::path::Path;
use tauri::State;

/// Scan all installed agents for skills.
/// Called on app startup and when the user refreshes.
///
/// `project_path`: Optional current project directory (from CWD detection).
/// If provided, includes project-scoped skills for that directory.
#[tauri::command]
pub fn scan_skills(state: State<'_, ConfigState>, project_path: Option<String>) -> ScanResult {
    let path = project_path.as_deref().map(Path::new);
    let mut result = scan_all_skills(path);

    let config = match state.0.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

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

/// List all known agents and whether they're installed.
/// Used to populate the agent filter tabs in the UI.
#[tauri::command]
pub fn list_agents(project_path: Option<String>) -> Vec<AgentInfo> {
    let path = project_path.as_deref().map(Path::new);
    detect_installed_agents(path)
}

/// Read the raw content of a skill file.
/// Used when the user clicks a card to see the full skill body.
#[tauri::command]
pub fn read_skill_content(
    skill_id: String,
    project_path: Option<String>,
) -> Result<String, String> {
    let path = project_path.as_deref().map(Path::new);
    let scan = scan_all_skills(path);

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
