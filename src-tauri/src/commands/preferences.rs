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
        "installed-newest" | "installed-oldest" | "archived-most" | "archived-recent"
        | "updated-newest" | "default" => input.trim().to_string(),
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

/// Load config from disk, or create default.
///
/// Corruption tolerance: a config.json that no longer parses is renamed to
/// `config.json.corrupt` (data preserved for inspection, never deleted) and
/// the app starts with defaults. The old behavior parsed-or-defaulted
/// silently, which meant the next save WIPED the user's unreadable-but-real
/// config with defaults.
pub fn load_config() -> AppConfig {
    let path = config_path();
    if path.exists() {
        let raw = std::fs::read_to_string(&path).ok();
        let parsed: Option<AppConfig> =
            raw.as_deref().and_then(|s| match serde_json::from_str(s) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    warn!(
                        "config.json is corrupt ({}); quarantining as .corrupt and starting fresh",
                        e
                    );
                    let corrupt = path.with_extension("json.corrupt");
                    let _ = std::fs::remove_file(&corrupt);
                    if let Err(re) = std::fs::rename(&path, &corrupt) {
                        warn!("failed to quarantine corrupt config: {}", re);
                    }
                    None
                }
            });
        let mut config: AppConfig = parsed.unwrap_or_default();

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

/// Persist config to disk (public for use by other command modules).
/// Atomic: temp file + rename, so a crash mid-save can never leave a
/// half-written config.json (which would then be quarantined on next start).
pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    crate::harness::session::atomic_write(&path, &json)
        .map_err(|e| format!("Failed to write config {}: {:#}", path.to_string_lossy(), e))
}

/// Apply one explicit starred state while normalising legacy aliases.
///
/// Keeping this mutation separate from the IPC command makes its ordering
/// contract testable: callers that know the desired final state never depend
/// on when two toggle requests happen to acquire the config mutex.
fn apply_starred_state(
    config: &mut AppConfig,
    canonical_skill_id: &str,
    requested_skill_id: &str,
    legacy_ids: &[String],
    desired_starred: bool,
) -> bool {
    if desired_starred {
        // A canonical ID is the sole persisted representation. Remove stale
        // aliases first so an old scan cannot leave duplicate preferences.
        config.starred_skills.remove(requested_skill_id);
        for legacy in legacy_ids {
            config.starred_skills.remove(legacy);
        }
        config.starred_skills.insert(canonical_skill_id.to_string());
        true
    } else {
        config.starred_skills.remove(canonical_skill_id);
        config.starred_skills.remove(requested_skill_id);
        for legacy in legacy_ids {
            config.starred_skills.remove(legacy);
        }
        false
    }
}

/// Set star status for a skill.
///
/// `desired_starred` is optional for compatibility with older webviews. New
/// clients always send it, which turns rapid clicks into ordered set-state
/// writes instead of relying on backend scheduling of two inverse toggles.
#[tauri::command]
pub fn toggle_star(
    state: State<ConfigState>,
    catalog: State<crate::commands::skills::SkillCatalogState>,
    skill_id: String,
    desired_starred: Option<bool>,
) -> Result<bool, String> {
    let lookup = crate::commands::skills::scan_for_lookup(&state, &catalog).ok();
    let canonical_skill_id = lookup
        .as_ref()
        .map(|lookup| lookup.canonical_id(&skill_id))
        .unwrap_or_else(|| skill_id.clone());
    let legacy_ids = lookup
        .as_ref()
        .and_then(|lookup| lookup.skill(&canonical_skill_id))
        .map(|skill| skill.legacy_ids.clone())
        .unwrap_or_default();

    let mut config = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let currently_starred = config.starred_skills.contains(&canonical_skill_id)
        || config.starred_skills.contains(&skill_id)
        || legacy_ids
            .iter()
            .any(|legacy| config.starred_skills.contains(legacy));
    let desired_starred = desired_starred.unwrap_or(!currently_starred);
    let previous_starred_skills = config.starred_skills.clone();
    let is_starred = apply_starred_state(
        &mut config,
        &canonical_skill_id,
        &skill_id,
        &legacy_ids,
        desired_starred,
    );
    if let Err(e) = save_config(&config) {
        // Do not leave process memory claiming a preference that could not be
        // persisted. The next scan and next launch therefore agree.
        config.starred_skills = previous_starred_skills;
        return Err(format!("Failed to persist starred state: {e}"));
    }
    drop(config);
    catalog.invalidate();
    Ok(is_starred)
}

/// Set a custom icon for a skill
#[tauri::command]
pub fn set_skill_icon(
    state: State<ConfigState>,
    catalog: State<crate::commands::skills::SkillCatalogState>,
    skill_id: String,
    icon: String,
) {
    let lookup = crate::commands::skills::scan_for_lookup(&state, &catalog).ok();
    let canonical_skill_id = lookup
        .as_ref()
        .map(|lookup| lookup.canonical_id(&skill_id))
        .unwrap_or_else(|| skill_id.clone());
    let legacy_ids = lookup
        .as_ref()
        .and_then(|lookup| lookup.skill(&canonical_skill_id))
        .map(|skill| skill.legacy_ids.clone())
        .unwrap_or_default();

    let mut config = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

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
    drop(config);
    catalog.invalidate();
}

/// Get the current app configuration.
/// Secrets hygiene: provider API keys and the web-search key are BLANKED in
/// the returned copy — `get_config` serializes into the webview where DevTools
/// can read it, and no frontend path consumes keys from here (provider setup
/// goes through `harness_list_providers`, which masks). Persisted config on
/// disk keeps the real values.
#[tauri::command]
pub fn get_config(state: State<ConfigState>) -> AppConfig {
    let mut config = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    for provider in config.harness_providers.iter_mut() {
        if provider.api_key.is_some() {
            provider.api_key = Some("••••".into());
        }
    }
    if config.harness_settings.web_search_api_key.is_some() {
        config.harness_settings.web_search_api_key = Some("••••".into());
    }
    config
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
    let config = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    config.starred_skills.iter().cloned().collect()
}

/// Set the active theme mode by name ("system", "dark", or "light")
#[tauri::command]
pub fn set_theme(state: State<ConfigState>, theme: String) {
    let mut config = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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

/// Replace the set of project roots scanned for project-scoped skills.
/// Blank entries are dropped; the list is de-duplicated while preserving order.
#[tauri::command]
pub fn set_project_roots(
    state: State<ConfigState>,
    catalog: State<crate::commands::skills::SkillCatalogState>,
    roots: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut cleaned: Vec<String> = Vec::new();
    for raw in roots {
        let trimmed = raw.trim().replace('\\', "/");
        let trimmed = trimmed.trim_end_matches('/').to_string();
        if !trimmed.is_empty() && !cleaned.contains(&trimmed) {
            cleaned.push(trimmed);
        }
    }

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;
    config.project_roots = cleaned.clone();
    save_config(&config)?;
    drop(config);
    catalog.invalidate();
    Ok(cleaned)
}

/// Replace custom files and directories included in the configured scan domain.
#[tauri::command]
pub fn set_custom_scan_paths(
    state: State<ConfigState>,
    catalog: State<crate::commands::skills::SkillCatalogState>,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let mut cleaned = Vec::new();
    for raw in paths {
        let trimmed = raw.trim().replace('\\', "/");
        if !trimmed.is_empty() && !cleaned.contains(&trimmed) {
            cleaned.push(trimmed);
        }
    }

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;
    config.custom_scan_paths = cleaned.clone();
    save_config(&config)?;
    drop(config);
    catalog.invalidate();
    Ok(cleaned)
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

/// Persist overlay size. The bounds account for font-scale resizing —
/// at scale 2.0 the default 460x640 layout becomes 920x1280, which must
/// still fit within MAX_W / MAX_H.
#[tauri::command]
pub fn set_overlay_size(state: State<ConfigState>, width: u32, height: u32) -> Result<(), String> {
    const MIN_W: u32 = 380;
    const MAX_W: u32 = 2400;
    const MIN_H: u32 = 560;
    const MAX_H: u32 = 1800;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Barrier, Mutex};
    use std::thread;

    #[test]
    fn explicit_star_state_normalizes_legacy_ids() {
        let mut config = AppConfig::default();
        config.starred_skills.insert("old-skill-id".to_string());

        assert!(apply_starred_state(
            &mut config,
            "canonical-skill-id",
            "requested-skill-id",
            &["old-skill-id".to_string()],
            true,
        ));
        assert_eq!(config.starred_skills.len(), 1);
        assert!(config.starred_skills.contains("canonical-skill-id"));

        assert!(!apply_starred_state(
            &mut config,
            "canonical-skill-id",
            "requested-skill-id",
            &["old-skill-id".to_string()],
            false,
        ));
        assert!(config.starred_skills.is_empty());
    }

    #[test]
    fn explicit_star_writes_are_linearizable_when_requests_overlap() {
        let config = Arc::new(Mutex::new(AppConfig::default()));
        let first_applied = Arc::new(Barrier::new(2));
        let release_first = Arc::new(Barrier::new(2));
        let (second_started_tx, second_started_rx) = mpsc::channel();

        let first_config = Arc::clone(&config);
        let first_applied_signal = Arc::clone(&first_applied);
        let release_first_signal = Arc::clone(&release_first);
        let first = thread::spawn(move || {
            let mut config = first_config.lock().expect("test config lock");
            apply_starred_state(&mut config, "skill", "skill", &[], true);
            first_applied_signal.wait();
            release_first_signal.wait();
        });

        // The first request has acquired the shared state mutex and applied
        // its desired value before the second request attempts its write.
        first_applied.wait();
        let second_config = Arc::clone(&config);
        let second = thread::spawn(move || {
            second_started_tx
                .send(())
                .expect("test coordination receiver");
            let mut config = second_config.lock().expect("test config lock");
            apply_starred_state(&mut config, "skill", "skill", &[], false);
        });
        second_started_rx.recv().expect("second request started");
        release_first.wait();

        first.join().expect("first writer");
        second.join().expect("second writer");
        assert!(!config
            .lock()
            .expect("test config lock")
            .starred_skills
            .contains("skill"));
    }
}
