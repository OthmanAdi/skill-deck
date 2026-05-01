// @agent-context: Update checking and repo management commands.
// These commands let the frontend:
// 1. Check a single skill for updates (async, calls GitHub API)
// 2. Set/override a skill's repository URL
// 3. Set/override a skill's install command

use tauri::State;

use crate::commands::preferences::ConfigState;
use crate::detection::update_checker;

fn is_supported_update_repo(repo_url: &str) -> bool {
    repo_url.contains("github.com")
}

fn parse_repo_ref(repo_url: &str) -> Option<String> {
    update_checker::parse_github_owner_repo(repo_url)
        .map(|(owner, repo)| format!("github:{}/{}", owner, repo))
}

/// Check a single skill for updates.
/// Returns whether an update is available.
#[tauri::command]
pub async fn check_skill_update(
    state: State<'_, ConfigState>,
    skill_id: String,
    repo_url: String,
) -> Result<bool, String> {
    {
        let config = state.0.lock().unwrap();
        if !config.check_updates {
            return Err("Update checks are disabled in configuration".to_string());
        }
    }

    if !is_supported_update_repo(&repo_url) {
        return Err("Only GitHub repositories are supported for update checks currently".to_string());
    }

    // Check cooldown
    let local_ref: Option<String>;
    {
        let config = state.0.lock().unwrap();
        let cache_entry = config.update_check_cache.get(&skill_id);
        let repo_ref = parse_repo_ref(&repo_url);
        if !update_checker::should_check(cache_entry) {
            // Return cached result
            return Ok(cache_entry.map(|e| e.update_available).unwrap_or(false));
        }

        local_ref = config
            .update_check_cache
            .get(&skill_id)
            .and_then(|e| {
                if repo_ref.is_some() && e.repo_ref.as_ref() == repo_ref.as_ref() {
                    e.remote_ref.clone()
                } else {
                    None
                }
            });
    }

    // Check GitHub for updates
    let result = update_checker::check_github_update(&repo_url, local_ref.as_deref()).await;

    if let Some(err) = &result.error {
        return Err(err.clone());
    }

    // Cache the result
    {
        let mut config = state.0.lock().unwrap();
        let mut entry = update_checker::make_cache_entry(&result);
        entry.repo_ref = parse_repo_ref(&repo_url);
        config.update_check_cache.insert(skill_id, entry);
        // Persist to disk
        crate::commands::preferences::save_config(&config)?;
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
    crate::commands::preferences::save_config(&config)?;
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
    crate::commands::preferences::save_config(&config)?;
    Ok(())
}
