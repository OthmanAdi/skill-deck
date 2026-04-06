// @agent-context: User preference commands (star/unstar, custom icons, config).
// Preferences are persisted as JSON in the OS app-data directory.

use crate::models::AppConfig;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

/// Thread-safe wrapper for the app config
pub struct ConfigState(pub Mutex<AppConfig>);

/// Get the config file path (OS-specific app data directory)
fn config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("skill-deck");
    std::fs::create_dir_all(&path).ok();
    path.push("config.json");
    path
}

/// Load config from disk, or create default
pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        let config = AppConfig::default();
        save_config(&config);
        config
    }
}

/// Persist config to disk (public for use by other command modules)
pub fn save_config(config: &AppConfig) {
    let path = config_path();
    if let Ok(json) = serde_json::to_string_pretty(config) {
        std::fs::write(path, json).ok();
    }
}

/// Toggle star status for a skill
#[tauri::command]
pub fn toggle_star(state: State<ConfigState>, skill_id: String) -> bool {
    let mut config = state.0.lock().unwrap();
    let is_starred = if config.starred_skills.contains(&skill_id) {
        config.starred_skills.remove(&skill_id);
        false
    } else {
        config.starred_skills.insert(skill_id);
        true
    };
    save_config(&config);
    is_starred
}

/// Set a custom icon for a skill
#[tauri::command]
pub fn set_skill_icon(state: State<ConfigState>, skill_id: String, icon: String) {
    let mut config = state.0.lock().unwrap();
    config.skill_icons.insert(skill_id, icon);
    save_config(&config);
}

/// Get the current app configuration
#[tauri::command]
pub fn get_config(state: State<ConfigState>) -> AppConfig {
    state.0.lock().unwrap().clone()
}

/// Update the global hotkey
#[tauri::command]
pub fn set_hotkey(state: State<ConfigState>, hotkey: String) {
    let mut config = state.0.lock().unwrap();
    config.hotkey = hotkey;
    save_config(&config);
}

/// Get all starred skill IDs
#[tauri::command]
pub fn get_starred_skills(state: State<ConfigState>) -> Vec<String> {
    let config = state.0.lock().unwrap();
    config.starred_skills.iter().cloned().collect()
}

/// Set the active theme by name (e.g. "obsidian", "obsidian-light")
#[tauri::command]
pub fn set_theme(state: State<ConfigState>, theme: String) {
    let mut config = state.0.lock().unwrap();
    config.theme = theme;
    save_config(&config);
}
