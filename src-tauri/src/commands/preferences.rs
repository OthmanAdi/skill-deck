// @agent-context: User preference commands (star/unstar, custom icons, config).
// Preferences are persisted as JSON in the OS app-data directory.

use crate::models::AppConfig;
use log::warn;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

fn normalize_hotkey(input: &str) -> String {
    let raw = input.trim().replace(' ', "");
    if raw.is_empty() {
        return "CommandOrControl+Shift+K".to_string();
    }

    raw.split('+')
        .filter_map(|token| {
            if token.is_empty() {
                return None;
            }
            let upper = token.to_uppercase();
            let normalized = match upper.as_str() {
                "CTRL" | "CONTROL" => "Control".to_string(),
                "SHIFT" => "Shift".to_string(),
                "ALT" | "OPTION" => "Alt".to_string(),
                "CMD" | "COMMAND" | "SUPER" | "META" => "Command".to_string(),
                "CMDORCTRL" | "CMDORCONTROL" | "COMMANDORCTRL" | "COMMANDORCONTROL" => {
                    "CommandOrControl".to_string()
                }
                _ => {
                    if upper.len() == 1 {
                        upper
                    } else if let Some(rest) = upper.strip_prefix("KEY") {
                        rest.to_string()
                    } else {
                        token.to_string()
                    }
                }
            };
            Some(normalized)
        })
        .collect::<Vec<String>>()
        .join("+")
}

fn is_modifier_token(token: &str) -> bool {
    matches!(
        token.to_uppercase().as_str(),
        "COMMAND" | "CONTROL" | "ALT" | "SHIFT" | "COMMANDORCONTROL"
    )
}

fn is_hotkey_shape_valid(hotkey: &str) -> bool {
    let parts = hotkey
        .split('+')
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>();

    if !(2..=3).contains(&parts.len()) {
        return false;
    }

    let has_modifier = parts.iter().any(|p| is_modifier_token(p));
    let has_non_modifier = parts.iter().any(|p| !is_modifier_token(p));
    has_modifier && has_non_modifier
}

fn normalize_skill_sort_mode(input: &str) -> String {
    match input.trim() {
        "installed-newest"
        | "installed-oldest"
        | "archived-most"
        | "archived-recent"
        | "updated-newest"
        | "default" => input.trim().to_string(),
        _ => "default".to_string(),
    }
}

fn hotkey_candidates(preferred: &str, include_fallbacks: bool) -> Vec<String> {
    let preferred = normalize_hotkey(preferred);
    let mut candidates = vec![preferred];

    if !include_fallbacks {
        return candidates;
    }

    for fallback in [
        "CommandOrControl+Shift+K",
        "Ctrl+Shift+K",
        "Control+Shift+K",
        "CmdOrControl+Shift+K",
        "CommandOrControl+Alt+K",
    ] {
        let normalized = normalize_hotkey(fallback);
        if !candidates
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&normalized))
        {
            candidates.push(normalized);
        }
    }

    candidates
}

pub fn register_overlay_hotkey<R: Runtime>(
    app: &AppHandle<R>,
    preferred: &str,
    previous: Option<&str>,
    include_fallbacks: bool,
) -> Result<String, String> {
    let candidates = hotkey_candidates(preferred, include_fallbacks);

    let mut unregister_list = candidates.clone();
    if let Some(prev) = previous {
        let normalized_prev = normalize_hotkey(prev);
        if !unregister_list
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&normalized_prev))
        {
            unregister_list.push(normalized_prev);
        }
    }

    for shortcut in &unregister_list {
        let _ = app.global_shortcut().unregister(shortcut.as_str());
    }

    for shortcut in candidates {
        let register_result = app.global_shortcut().on_shortcut(
            shortcut.as_str(),
            move |app_handle, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    if let Some(window) = app_handle.get_webview_window("overlay") {
                        let _ = window.emit("overlay-hotkey-pressed", ());
                    }
                }
            },
        );

        if register_result.is_ok() {
            return Ok(shortcut);
        }
    }

    Err("Failed to register any global hotkey candidate".to_string())
}

/// Thread-safe wrapper for the app config
pub struct ConfigState(pub Mutex<AppConfig>);

/// Tracks the currently active registered global hotkey.
pub struct HotkeyState(pub Mutex<Option<String>>);

/// Get the config file path (OS-specific app data directory)
fn config_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("skill-deck");
    if let Err(e) = std::fs::create_dir_all(&path) {
        warn!(
            "Failed to create config directory {}: {}",
            path.to_string_lossy(),
            e
        );
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

        config.hotkey = normalize_hotkey(&config.hotkey);
        config.overlay_mode = match config.overlay_mode.as_str() {
            "auto-hide" => "auto-hide".to_string(),
            _ => "pinned".to_string(),
        };
        config.skill_sort_mode = normalize_skill_sort_mode(&config.skill_sort_mode);

        config
    } else {
        let config = AppConfig::default();
        if let Err(e) = save_config(&config) {
            warn!(
                "Failed to save initial config {}: {}",
                path.to_string_lossy(),
                e
            );
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

    let canonical_skill_id = {
        let mut temp = config.clone();
        let (scan, _) = crate::commands::skills::scan_with_config(&mut temp);
        scan.skills
            .iter()
            .find(|skill| {
                skill.id == skill_id || skill.legacy_ids.iter().any(|legacy| legacy == &skill_id)
            })
            .map(|skill| skill.id.clone())
            .unwrap_or(skill_id.clone())
    };

    let legacy_ids = {
        let mut temp = config.clone();
        let (scan, _) = crate::commands::skills::scan_with_config(&mut temp);
        scan.skills
            .iter()
            .find(|skill| skill.id == canonical_skill_id)
            .map(|skill| skill.legacy_ids.clone())
            .unwrap_or_default()
    };

    let is_starred = if config.starred_skills.contains(&canonical_skill_id)
        || config.starred_skills.contains(&skill_id)
    {
        config.starred_skills.remove(&canonical_skill_id);
        config.starred_skills.remove(&skill_id);
        for legacy in &legacy_ids {
            config.starred_skills.remove(legacy);
        }
        false
    } else {
        config.starred_skills.insert(canonical_skill_id);
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

    let canonical_skill_id = {
        let mut temp = config.clone();
        let (scan, _) = crate::commands::skills::scan_with_config(&mut temp);
        scan.skills
            .iter()
            .find(|skill| {
                skill.id == skill_id || skill.legacy_ids.iter().any(|legacy| legacy == &skill_id)
            })
            .map(|skill| skill.id.clone())
            .unwrap_or(skill_id.clone())
    };

    let legacy_ids = {
        let mut temp = config.clone();
        let (scan, _) = crate::commands::skills::scan_with_config(&mut temp);
        scan.skills
            .iter()
            .find(|skill| skill.id == canonical_skill_id)
            .map(|skill| skill.legacy_ids.clone())
            .unwrap_or_default()
    };

    let normalized = icon.trim();
    if normalized.is_empty() {
        config.skill_icons.remove(&canonical_skill_id);
        config.skill_icons.remove(&skill_id);
        for legacy in &legacy_ids {
            config.skill_icons.remove(legacy);
        }
    } else {
        config
            .skill_icons
            .insert(canonical_skill_id, normalized.to_string());
    }
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
pub fn set_hotkey<R: Runtime>(
    app: AppHandle<R>,
    state: State<ConfigState>,
    hotkey_state: State<HotkeyState>,
    hotkey: String,
) -> Result<String, String> {
    let requested = normalize_hotkey(&hotkey);

    if !is_hotkey_shape_valid(&requested) {
        return Err("Shortcut must use 2 or 3 keys with at least one modifier".to_string());
    }

    let previous = hotkey_state
        .0
        .lock()
        .map_err(|_| "Failed to lock hotkey state".to_string())?
        .clone();

    let active = match register_overlay_hotkey(&app, &requested, previous.as_deref(), false) {
        Ok(active) => active,
        Err(err) => {
            if let Some(prev) = previous.as_deref() {
                let _ = register_overlay_hotkey(&app, prev, None, false);
            }
            return Err(err);
        }
    };

    {
        let mut current = hotkey_state
            .0
            .lock()
            .map_err(|_| "Failed to lock hotkey state".to_string())?;
        *current = Some(active.clone());
    }

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;
    config.hotkey = active.clone();
    save_config(&config)?;

    Ok(active)
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

/// Set overlay interaction mode ("pinned" or "auto-hide")
#[tauri::command]
pub fn set_overlay_mode<R: Runtime>(
    app: AppHandle<R>,
    state: State<ConfigState>,
    mode: String,
) -> Result<String, String> {
    let normalized = match mode.as_str() {
        "pinned" => "pinned".to_string(),
        "auto-hide" => "auto-hide".to_string(),
        _ => "pinned".to_string(),
    };

    {
        let mut config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        config.overlay_mode = normalized.clone();
        save_config(&config)?;
    }

    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.set_always_on_top(normalized != "auto-hide");
        let _ = window.emit("overlay-mode-changed", normalized.clone());
    }

    Ok(normalized)
}

/// Persist overlay size.
#[tauri::command]
pub fn set_overlay_size(state: State<ConfigState>, width: u32, height: u32) -> Result<(), String> {
    const MIN_W: u32 = 380;
    const MAX_W: u32 = 1600;
    const MIN_H: u32 = 560;
    const MAX_H: u32 = 1100;

    let clamped_w = width.clamp(MIN_W, MAX_W);
    let clamped_h = height.clamp(MIN_H, MAX_H);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.overlay_width = clamped_w;
    config.overlay_height = clamped_h;
    save_config(&config)?;

    Ok(())
}

/// Persist global UI font scale (CSS `zoom` applied to the document root).
/// Clamped to [1.0, 2.0] so the overlay always stays usable.
#[tauri::command]
pub fn set_font_scale(state: State<ConfigState>, scale: f64) -> Result<f64, String> {
    const MIN_SCALE: f64 = 1.0;
    const MAX_SCALE: f64 = 2.0;

    let clamped = if scale.is_finite() {
        scale.clamp(MIN_SCALE, MAX_SCALE)
    } else {
        MIN_SCALE
    };

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.font_scale = clamped;
    save_config(&config)?;
    Ok(clamped)
}

/// Persist finder panel visibility state.
#[tauri::command]
pub fn set_finder_open(state: State<ConfigState>, open: bool) -> Result<(), String> {
    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.finder_open = open;
    save_config(&config)?;
    Ok(())
}

/// Persist skill sort mode for overlay lists.
#[tauri::command]
pub fn set_skill_sort_mode(state: State<ConfigState>, mode: String) -> Result<String, String> {
    let normalized = normalize_skill_sort_mode(&mode);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.skill_sort_mode = normalized.clone();
    save_config(&config)?;

    Ok(normalized)
}

/// Persist collapsed agent groups for grouped list view.
#[tauri::command]
pub fn set_collapsed_agents(
    state: State<ConfigState>,
    collapsed_agents: Vec<String>,
) -> Result<(), String> {
    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.collapsed_agents = collapsed_agents
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();

    save_config(&config)?;
    Ok(())
}

/// Persist collapsed tree node ids for tree view.
#[tauri::command]
pub fn set_collapsed_tree_nodes(
    state: State<ConfigState>,
    collapsed_tree_nodes: Vec<String>,
) -> Result<(), String> {
    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    config.collapsed_tree_nodes = collapsed_tree_nodes
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();

    save_config(&config)?;
    Ok(())
}
