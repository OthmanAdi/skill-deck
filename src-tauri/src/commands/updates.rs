// @agent-context: Update checking, repository management, and local version history.

use tauri::State;

use crate::commands::preferences::ConfigState;
use crate::detection::{skill_history, skills_registry, update_checker};
use crate::models::{SkillVersionEntry, UpdateErrorKind};

fn scan_for_commands(state: &State<'_, ConfigState>) -> Result<crate::models::ScanResult, String> {
    let (scan, config_changed, config_snapshot) = {
        let mut config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        let (scan, changed) = crate::commands::skills::scan_with_config(&mut config);
        (scan, changed, config.clone())
    };

    if config_changed {
        let _ = crate::commands::preferences::save_config(&config_snapshot);
    }

    Ok(scan)
}

fn resolve_canonical_skill_id(scan: &crate::models::ScanResult, skill_id: &str) -> String {
    scan.skills
        .iter()
        .find(|skill| skill.id == skill_id || skill.legacy_ids.iter().any(|legacy| legacy == skill_id))
        .map(|skill| skill.id.clone())
        .unwrap_or_else(|| skill_id.to_string())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResponse {
    pub checked: bool,
    pub update_available: bool,
    pub canonical_repo_url: Option<String>,
    pub remote_ref: Option<String>,
    pub source: String,
    pub error: Option<String>,
    pub error_kind: Option<UpdateErrorKind>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillHistoryResponse {
    pub skill_id: String,
    pub entries: Vec<SkillVersionEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSkillVersionResult {
    pub restored: bool,
    pub version_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySkillSearchResponse {
    pub query: String,
    pub count: usize,
    pub duration_ms: u64,
    pub skills: Vec<skills_registry::RegistrySkillSummary>,
}

fn scan_skill_file_path(state: &State<'_, ConfigState>, skill_id: &str) -> Result<String, String> {
    let scan = scan_for_commands(state)?;
    let skill = scan
        .skills
        .into_iter()
        .find(|s| s.id == skill_id || s.legacy_ids.iter().any(|legacy| legacy == skill_id))
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;
    Ok(skill.file_path)
}

fn read_repo_and_remote_ref_for_skill(
    state: &State<'_, ConfigState>,
    skill_id: &str,
) -> Result<(Option<String>, Option<String>), String> {
    let scan = scan_for_commands(state)?;
    let skill = scan
        .skills
        .into_iter()
        .find(|s| s.id == skill_id || s.legacy_ids.iter().any(|legacy| legacy == skill_id))
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;

    let version = skill.metadata.version.clone();
    let remote_ref = if version.is_some() {
        version
    } else {
        skill
            .metadata
            .extra
            .as_ref()
            .and_then(|value| value.get("sha"))
            .and_then(|value| value.as_str())
            .map(|s| s.to_string())
    };
    Ok((skill.metadata.repository_url, remote_ref))
}

fn write_skill_file(path: &str, content: &str) -> Result<(), String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| format!("Failed to resolve skill path {}: {}", path, e))?;
    std::fs::write(&canonical, content)
        .map_err(|e| format!("Failed to write {}: {}", canonical.to_string_lossy(), e))
}

#[tauri::command]
pub async fn check_skill_update(
    state: State<'_, ConfigState>,
    skill_id: String,
    repo_url: String,
    force: Option<bool>,
) -> Result<UpdateCheckResponse, String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    {
        let config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        if !config.check_updates {
            return Err("Update checks are disabled in configuration".to_string());
        }
    }

    let parsed_repo_ref = update_checker::repo_ref_from_repo_url(&repo_url);
    let canonical_repo_url = update_checker::canonicalize_github_repo_url(&repo_url);
    if parsed_repo_ref.is_none() {
        return Ok(UpdateCheckResponse {
            checked: false,
            update_available: false,
            canonical_repo_url: None,
            remote_ref: None,
            source: "error".to_string(),
            error: Some(
                "Only HTTPS github.com repositories are supported for update checks currently"
                    .to_string(),
            ),
            error_kind: Some(UpdateErrorKind::InvalidRepoUrl),
        });
    }

    let local_ref: Option<String> = {
        let config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        let cache_entry = config.update_check_cache.get(&canonical_skill_id);
        if !force.unwrap_or(false) && !update_checker::should_check(cache_entry) {
            if let Some(entry) = cache_entry {
                return Ok(UpdateCheckResponse {
                    checked: false,
                    update_available: entry.update_available,
                    canonical_repo_url: canonical_repo_url.clone(),
                    remote_ref: entry.remote_ref.clone(),
                    source: "cache".to_string(),
                    error: entry.last_error.clone(),
                    error_kind: entry.last_error_kind.clone(),
                });
            }
        }

        config.update_check_cache.get(&canonical_skill_id).and_then(|e| {
            if parsed_repo_ref.is_some() && e.repo_ref.as_ref() == parsed_repo_ref.as_ref() {
                e.remote_ref.clone()
            } else {
                None
            }
        })
    };

    let result = update_checker::check_github_update(&repo_url, local_ref.as_deref()).await;

    {
        let mut config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        let mut entry = update_checker::make_cache_entry(&result);
        entry.repo_ref = parsed_repo_ref;
        config.update_check_cache.insert(canonical_skill_id, entry);
        crate::commands::preferences::save_config(&config)?;
    }

    let checked_ok = result.error.is_none();
    Ok(UpdateCheckResponse {
        checked: checked_ok,
        update_available: result.update_available,
        canonical_repo_url: result.canonical_repo_url,
        remote_ref: result.remote_ref,
        source: "remote".to_string(),
        error: result.error,
        error_kind: result.error_kind,
    })
}

/// Set or override a skill's repository URL.
#[tauri::command]
pub fn set_skill_repo(
    state: State<ConfigState>,
    skill_id: String,
    repo_url: String,
) -> Result<Option<String>, String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    if repo_url.trim().is_empty() {
        config.skill_repo_overrides.remove(&canonical_skill_id);
        crate::commands::preferences::save_config(&config)?;
        Ok(None)
    } else {
        let canonical =
            update_checker::canonicalize_github_repo_url(&repo_url).ok_or_else(|| {
                "Repository URL must be owner/repo or https://github.com/owner/repo".to_string()
            })?;
        config
            .skill_repo_overrides
            .insert(canonical_skill_id, canonical.clone());
        crate::commands::preferences::save_config(&config)?;
        Ok(Some(canonical))
    }
}

/// Set or override a skill's install command.
#[tauri::command]
pub fn set_skill_install_command(
    state: State<ConfigState>,
    skill_id: String,
    install_command: String,
) -> Result<(), String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;
    if install_command.is_empty() {
        config.skill_install_overrides.remove(&canonical_skill_id);
    } else {
        config
            .skill_install_overrides
            .insert(canonical_skill_id, install_command);
    }
    crate::commands::preferences::save_config(&config)?;
    Ok(())
}

/// Snapshot the current content of a skill file before applying an external update.
#[tauri::command]
pub fn snapshot_skill_before_update(
    state: State<ConfigState>,
    skill_id: String,
    _project_path: Option<String>,
    source_repo_url: Option<String>,
    remote_ref: Option<String>,
    reason: Option<String>,
) -> Result<SkillVersionEntry, String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);
    let file_path = scan_skill_file_path(&state, &canonical_skill_id)?;
    let current = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;
    let (detected_repo, detected_remote_ref) =
        read_repo_and_remote_ref_for_skill(&state, &canonical_skill_id)?;

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    let reason_text = reason.unwrap_or_else(|| "before-update".to_string());
    let repo_ref = source_repo_url.or(detected_repo);
    let remote_ref = remote_ref.or(detected_remote_ref);
    let entry = skill_history::create_snapshot(
        &mut config,
        &canonical_skill_id,
        &current,
        &reason_text,
        repo_ref.as_deref(),
        remote_ref.as_deref(),
    )?;

    crate::commands::preferences::save_config(&config)?;
    Ok(entry)
}

#[tauri::command]
pub fn list_skill_versions(
    state: State<ConfigState>,
    skill_id: String,
) -> Result<SkillHistoryResponse, String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;
    Ok(SkillHistoryResponse {
        skill_id: canonical_skill_id.clone(),
        entries: skill_history::get_history(&config, &canonical_skill_id),
    })
}

#[tauri::command]
pub fn restore_skill_version(
    state: State<ConfigState>,
    skill_id: String,
    version_id: String,
    _project_path: Option<String>,
) -> Result<RestoreSkillVersionResult, String> {
    let scan = scan_for_commands(&state)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);
    let file_path = scan_skill_file_path(&state, &canonical_skill_id)?;

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    let entries = config
        .skill_version_history
        .get(&canonical_skill_id)
        .cloned()
        .unwrap_or_default();
    let target = entries
        .iter()
        .find(|e| e.version_id == version_id)
        .cloned()
        .ok_or_else(|| "Requested version not found".to_string())?;

    let current = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;

    let _ = skill_history::create_snapshot(
        &mut config,
        &canonical_skill_id,
        &current,
        "before-restore",
        None,
        None,
    )?;

    let snapshot = skill_history::load_snapshot(&target)?;
    write_skill_file(&file_path, &snapshot.content)?;

    crate::commands::preferences::save_config(&config)?;
    Ok(RestoreSkillVersionResult {
        restored: true,
        version_id,
    })
}

#[tauri::command]
pub async fn search_skills_registry(
    query: String,
    limit: Option<u32>,
) -> Result<RegistrySkillSearchResponse, String> {
    let normalized_limit = skills_registry::normalize_limit(limit.unwrap_or(20) as usize);

    let result = skills_registry::search_registry(&query, normalized_limit)
        .await
        .map_err(|e| e.to_string())?;

    Ok(RegistrySkillSearchResponse {
        query: result.query,
        count: result.count,
        duration_ms: result.duration_ms,
        skills: result.skills,
    })
}
