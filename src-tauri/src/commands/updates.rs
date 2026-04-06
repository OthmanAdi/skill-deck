// @agent-context: Update checking and repo management commands.
// These commands let the frontend:
// 1. Check a single skill for updates (async, calls GitHub API)
// 2. Set/override a skill's repository URL
// 3. Set/override a skill's install command

use std::path::Path;
use tauri::State;

use crate::commands::preferences::ConfigState;
use crate::detection::update_checker;

/// Check a single skill for updates.
/// Returns whether an update is available.
#[tauri::command]
pub async fn check_skill_update(
    state: State<'_, ConfigState>,
    skill_id: String,
    repo_url: String,
    file_path: String,
) -> Result<bool, String> {
    // Check cooldown
    {
        let config = state.0.lock().unwrap();
        let cache_entry = config.update_check_cache.get(&skill_id);
        if !update_checker::should_check(cache_entry) {
            // Return cached result
            return Ok(cache_entry.map(|e| e.update_available).unwrap_or(false));
        }
    }

    // Compute local file hash for comparison
    let local_hash = update_checker::file_content_hash(Path::new(&file_path));

    // Check GitHub for updates
    let result = update_checker::check_github_update(
        &repo_url,
        local_hash.as_deref(),
    ).await;

    if let Some(err) = &result.error {
        return Err(err.clone());
    }

    // Cache the result
    {
        let mut config = state.0.lock().unwrap();
        let entry = update_checker::make_cache_entry(&result);
        config.update_check_cache.insert(skill_id, entry);
        // Persist to disk
        crate::commands::preferences::save_config(&config);
    }

    Ok(result.update_available)
}

/// Set or override a skill's repository URL.
#[tauri::command]
pub fn set_skill_repo(
    state: State<ConfigState>,
    skill_id: String,
    repo_url: String,
) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    if repo_url.is_empty() {
        config.skill_repo_overrides.remove(&skill_id);
    } else {
        config.skill_repo_overrides.insert(skill_id, repo_url);
    }
    crate::commands::preferences::save_config(&config);
    Ok(())
}

/// Set or override a skill's install command.
#[tauri::command]
pub fn set_skill_install_command(
    state: State<ConfigState>,
    skill_id: String,
    install_command: String,
) -> Result<(), String> {
    let mut config = state.0.lock().unwrap();
    if install_command.is_empty() {
        config.skill_install_overrides.remove(&skill_id);
    } else {
        config.skill_install_overrides.insert(skill_id, install_command);
    }
    crate::commands::preferences::save_config(&config);
    Ok(())
}
