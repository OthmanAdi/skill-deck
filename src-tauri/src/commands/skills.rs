// @agent-context: Skill scanning commands exposed to the frontend.
// These are the primary data-fetching commands the UI calls on startup and refresh.

use crate::agents::{detect_installed_agents, scan_all_skills};
use crate::models::{AgentInfo, ScanResult};
use std::path::Path;

/// Scan all installed agents for skills.
/// Called on app startup and when the user refreshes.
///
/// `project_path`: Optional current project directory (from CWD detection).
/// If provided, includes project-scoped skills for that directory.
#[tauri::command]
pub fn scan_skills(project_path: Option<String>) -> ScanResult {
    let path = project_path.as_deref().map(Path::new);
    scan_all_skills(path)
}

/// List all known agents and whether they're installed.
/// Used to populate the agent filter tabs in the UI.
#[tauri::command]
pub fn list_agents() -> Vec<AgentInfo> {
    detect_installed_agents()
}

/// Read the raw content of a skill file.
/// Used when the user clicks a card to see the full skill body.
#[tauri::command]
pub fn read_skill_content(file_path: String) -> Result<String, String> {
    std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path, e))
}
