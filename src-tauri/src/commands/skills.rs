// @agent-context: Skill scanning commands exposed to the frontend.
// These are the primary data-fetching commands the UI calls on startup and refresh.

use crate::agents::{detect_installed_agents, scan_all_skills, scan_custom_paths};
use crate::commands::preferences::ConfigState;
use crate::detection::skill_identity::{
    dedupe_skills_by_source, remap_config_ids_for_skills, ConfigIdCollections,
};
use crate::detection::repo_detector::{apply_overrides, DetectedSource};
use crate::detection::skill_discovery::enrich_skill_discovery;
use crate::models::{AgentInfo, AppConfig, ScanResult, Skill, SkillInstallEntry};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn normalized_skill_path(skill: &Skill) -> String {
    skill.file_path.replace('\\', "/").to_lowercase()
}

fn skill_install_fingerprint(skill: &Skill) -> String {
    format!(
        "{}:{}:{}:{}",
        serde_json::to_string(&skill.agent_id)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"'),
        normalized_skill_path(skill),
        skill.name.trim().to_lowercase(),
        skill.metadata.slash_command.as_deref().unwrap_or("").trim().to_lowercase()
    )
}

fn file_timestamp(path: &str, current_time: u64) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;

    let mut best: Option<u64> = None;
    for stamp in [metadata.created().ok(), metadata.modified().ok(), metadata.accessed().ok()] {
        let Some(stamp) = stamp else {
            continue;
        };

        let Some(secs) = stamp.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()) else {
            continue;
        };
        if secs == 0 || secs > current_time + 3600 {
            continue;
        }

        best = Some(match best {
            Some(existing) => existing.min(secs),
            None => secs,
        });
    }

    best
}

fn infer_install_timestamp(skill: &Skill, current_time: u64) -> u64 {
    file_timestamp(&skill.file_path, current_time).unwrap_or(current_time)
}

/// Shared scan pipeline used by commands that need the exact same skill IDs as the UI.
pub(crate) fn scan_with_config(config: &mut AppConfig) -> (ScanResult, bool) {
    let mut result = scan_all_skills();
    let mut config_changed = false;

    let (custom_skills, custom_errors) = scan_custom_paths(&config.custom_scan_paths);
    result.skills.extend(custom_skills);
    result.errors.extend(custom_errors);
    dedupe_skills_by_source(&mut result.skills);

    config_changed |= remap_config_ids_for_skills(
        &result.skills,
        ConfigIdCollections {
            starred_skills: &mut config.starred_skills,
            skill_icons: &mut config.skill_icons,
            skill_repo_overrides: &mut config.skill_repo_overrides,
            skill_install_overrides: &mut config.skill_install_overrides,
            update_check_cache: &mut config.update_check_cache,
            skill_version_history: &mut config.skill_version_history,
            collapsed_tree_nodes: &mut config.collapsed_tree_nodes,
        },
    );

    let current_time = now_unix_secs();

    // True only on the very first scan after a fresh install (or after the
    // user wiped their config). When false, an unknown fingerprint means a
    // genuinely new skill the user just installed, so prefer "now" over the
    // source file's mtime — that mtime might be from years ago if the skill
    // was authored long before the user installed it.
    let is_bootstrap_scan = config.skill_install_index.is_empty();

    for skill in &mut result.skills {
        let mut detected = DetectedSource {
            repository_url: skill.metadata.repository_url.clone(),
            install_command: skill.metadata.install_command.clone(),
        };

        for legacy in &skill.legacy_ids {
            if config.starred_skills.contains(legacy) {
                skill.starred = true;
            }

            if skill.icon.is_none() {
                if let Some(icon) = config.skill_icons.get(legacy) {
                    skill.icon = Some(icon.clone());
                }
            }
        }

        apply_overrides(
            &mut detected,
            &skill.id,
            &config.skill_repo_overrides,
            &config.skill_install_overrides,
        );

        if detected.repository_url.is_none() {
            if let Some(override_url) = skill
                .legacy_ids
                .iter()
                .find_map(|legacy| config.skill_repo_overrides.get(legacy).cloned())
            {
                detected.repository_url = Some(override_url);
            }
        }

        if detected.install_command.is_none() {
            if let Some(override_install) = skill
                .legacy_ids
                .iter()
                .find_map(|legacy| config.skill_install_overrides.get(legacy).cloned())
            {
                detected.install_command = Some(override_install);
            }
        }

        skill.metadata.repository_url = detected.repository_url;
        skill.metadata.install_command = detected.install_command;

        if let Some(icon) = config.skill_icons.get(&skill.id) {
            skill.icon = Some(icon.clone());
        }
        if config.starred_skills.contains(&skill.id) {
            skill.starred = true;
        }

        if let Some(cached) = config
            .update_check_cache
            .get(&skill.id)
            .or_else(|| {
                skill
                    .legacy_ids
                    .iter()
                    .find_map(|legacy| config.update_check_cache.get(legacy))
            })
        {
            skill.update_available = cached.update_available;
        }

        let install_key = skill_install_fingerprint(skill);
        let installed_at = if let Some(existing) = config.skill_install_index.get(&install_key) {
            existing.installed_at
        } else {
            // On a bootstrap scan, fall back to file mtime so existing skills
            // don't all collapse to one timestamp. Otherwise this is a newly
            // installed skill — use current_time so "Recently installed" puts
            // it at the top instead of inheriting the source file's old mtime.
            let inferred = if is_bootstrap_scan {
                infer_install_timestamp(skill, current_time)
            } else {
                current_time
            };
            config
                .skill_install_index
                .insert(install_key.clone(), SkillInstallEntry { installed_at: inferred });
            config_changed = true;
            inferred
        };

        skill.installed_at = Some(installed_at);
    }

    enrich_skill_discovery(&mut result.skills);

    (result, config_changed)
}

/// Scan all installed agents for skills.
/// Called on app startup and when the user refreshes.
///
#[tauri::command]
pub fn scan_skills(state: State<'_, ConfigState>) -> ScanResult {
    let (result, config_changed, config_snapshot) = {
        let mut guard = match state.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let (result, changed) = scan_with_config(&mut guard);
        (result, changed, guard.clone())
    };

    if config_changed {
        let _ = crate::commands::preferences::save_config(&config_snapshot);
    }

    result
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
pub fn read_skill_content(
    state: State<'_, ConfigState>,
    skill_id: String,
) -> Result<String, String> {
    let (scan, config_changed, config_snapshot) = {
        let mut config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        let (scan, changed) = scan_with_config(&mut config);
        (scan, changed, config.clone())
    };

    if config_changed {
        let _ = crate::commands::preferences::save_config(&config_snapshot);
    }

    let skill = scan
        .skills
        .into_iter()
        .find(|s| s.id == skill_id || s.legacy_ids.iter().any(|legacy| legacy == &skill_id))
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;

    let canonical = std::fs::canonicalize(&skill.file_path)
        .map_err(|e| format!("Failed to resolve skill path {}: {}", skill.file_path, e))?;

    std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read {}: {}", canonical.to_string_lossy(), e))
}
