// @agent-context: User preference commands (star/unstar, custom icons, config).
// Preferences are persisted as JSON in the OS app-data directory.

use crate::models::AppConfig;
use log::warn;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

/// Thread-safe wrapper for the app config
pub struct ConfigState(pub Mutex<AppConfig>);

/// Get the config file path (OS-specific app data directory)
fn config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("skill-deck");
    if let Err(e) = std::fs::create_dir_all(&path) {
        warn!("Failed to create config directory {}: {}", path.to_string_lossy(), e);
    }
    path.push("config.json");
    path
}

/// Load config from disk, or create default
pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        let mut config: AppConfig = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        config.theme = match config.theme.as_str() {
            "system" | "dark" | "light" => config.theme,
            "obsidian" => "dark".to_string(),
            "obsidian-light" => "light".to_string(),
            _ => "system".to_string(),
        };

        config
    } else {
        let config = AppConfig::default();
        if let Err(e) = save_config(&config) {
            warn!("Failed to save initial config {}: {}", path.to_string_lossy(), e);
        }
        config
    }
}

/// Persist config to disk (public for use by other command modules)
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write config {}: {}", path.to_string_lossy(), e))
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
    if let Err(e) = save_config(&config) {
        warn!("Failed to persist starred state: {}", e);
    }
    is_starred
}

/// Set a custom icon for a skill
#[tauri::command]
pub fn set_skill_icon(state: State<ConfigState>, skill_id: String, icon: String) {
    let mut config = state.0.lock().unwrap();
    config.skill_icons.insert(skill_id, icon);
    if let Err(e) = save_config(&config) {
        warn!("Failed to persist skill icon: {}", e);
    }
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
    if let Err(e) = save_config(&config) {
        warn!("Failed to persist hotkey: {}", e);
    }
}

/// Get all starred skill IDs
#[tauri::command]
pub fn get_starred_skills(state: State<ConfigState>) -> Vec<String> {
    let config = state.0.lock().unwrap();
    config.starred_skills.iter().cloned().collect()
}

/// Set the active theme mode by name ("system", "dark", or "light")
#[tauri::command]
pub fn set_theme(state: State<ConfigState>, theme: String) {
    let mut config = state.0.lock().unwrap();
    config.theme = match theme.as_str() {
        "system" | "dark" | "light" => theme,
        "obsidian" => "dark".to_string(),
        "obsidian-light" => "light".to_string(),
        _ => "system".to_string(),
    };
    if let Err(e) = save_config(&config) {
        warn!("Failed to persist theme: {}", e);
    }
}
