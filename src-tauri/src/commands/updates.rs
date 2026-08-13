// @agent-context: Update checking, repository management, and local version history.

use tauri::State;

use crate::commands::preferences::ConfigState;
use crate::commands::skills::{
    read_skill_file_in_scope, read_skill_file_with_binding_in_scope, scan_for_lookup,
    scan_for_mutation, write_skill_file_in_scope, CatalogLookup, SkillCatalogState,
};
use crate::detection::marketplaces::{
    self, MarketplaceSearchResponse, ProviderId, ProviderSearchOutcome,
};
use crate::detection::{skill_history, update_checker};
use crate::models::{AppConfig, SkillVersionEntry, UpdateErrorKind};

static RESTORE_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn scan_for_commands(
    state: &State<'_, ConfigState>,
    catalog: &State<'_, SkillCatalogState>,
) -> Result<CatalogLookup, String> {
    scan_for_lookup(state, catalog)
}

fn resolve_canonical_skill_id(scan: &CatalogLookup, skill_id: &str) -> String {
    scan.canonical_id(skill_id)
}

/// The only mutable roots that can extend the destructive file scope. A
/// restore authorizes its scan, binding, and write against one exact snapshot
/// of these lists, and refuses the operation if that scope changes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RestoreScope {
    project_roots: Vec<String>,
    custom_scan_paths: Vec<String>,
}

impl From<&AppConfig> for RestoreScope {
    fn from(config: &AppConfig) -> Self {
        Self {
            project_roots: config.project_roots.clone(),
            custom_scan_paths: config.custom_scan_paths.clone(),
        }
    }
}

fn ensure_restore_scope_is_current(
    expected: &RestoreScope,
    config: &AppConfig,
) -> Result<(), String> {
    if expected.project_roots != config.project_roots
        || expected.custom_scan_paths != config.custom_scan_paths
    {
        return Err(
            "Restore scope changed during the operation; retry from the current catalog"
                .to_string(),
        );
    }
    Ok(())
}

fn rollback_restore_transaction(
    config: &mut AppConfig,
    original_config: &AppConfig,
    staged_entry: &SkillVersionEntry,
) -> Result<(), String> {
    crate::commands::preferences::save_config(original_config)?;
    skill_history::rollback_staged_snapshot(config, original_config, staged_entry);
    Ok(())
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
pub struct AggregatedMarketplaceResponse {
    pub query: String,
    pub providers: Vec<ProviderSearchOutcome>,
}

fn scan_skill_file_path(scan: &CatalogLookup, skill_id: &str) -> Result<String, String> {
    let skill = scan
        .skill(skill_id)
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;
    Ok(skill.file_path.clone())
}

fn read_repo_and_remote_ref_for_skill(
    scan: &CatalogLookup,
    skill_id: &str,
) -> Result<(Option<String>, Option<String>), String> {
    let skill = scan
        .skill(skill_id)
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
    Ok((skill.metadata.repository_url.clone(), remote_ref))
}

#[tauri::command]
pub async fn check_skill_update(
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
    skill_id: String,
    repo_url: String,
    force: Option<bool>,
) -> Result<UpdateCheckResponse, String> {
    let scan = scan_for_commands(&state, &catalog)?;
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

        config
            .update_check_cache
            .get(&canonical_skill_id)
            .and_then(|e| {
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
    catalog.invalidate();

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
    catalog: State<SkillCatalogState>,
    skill_id: String,
    repo_url: String,
) -> Result<Option<String>, String> {
    let scan = scan_for_commands(&state, &catalog)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    if repo_url.trim().is_empty() {
        config.skill_repo_overrides.remove(&canonical_skill_id);
        crate::commands::preferences::save_config(&config)?;
        drop(config);
        catalog.invalidate();
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
        drop(config);
        catalog.invalidate();
        Ok(Some(canonical))
    }
}

/// Set or override a skill's install command.
#[tauri::command]
pub fn set_skill_install_command(
    state: State<ConfigState>,
    catalog: State<SkillCatalogState>,
    skill_id: String,
    install_command: String,
) -> Result<(), String> {
    let scan = scan_for_commands(&state, &catalog)?;
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
    drop(config);
    catalog.invalidate();
    Ok(())
}

/// Snapshot the current content of a skill file before applying an external update.
#[tauri::command]
pub fn snapshot_skill_before_update(
    state: State<ConfigState>,
    catalog: State<SkillCatalogState>,
    skill_id: String,
    _project_path: Option<String>,
    source_repo_url: Option<String>,
    remote_ref: Option<String>,
    reason: Option<String>,
) -> Result<SkillVersionEntry, String> {
    let scan = scan_for_commands(&state, &catalog)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);
    let file_path = scan_skill_file_path(&scan, &canonical_skill_id)?;
    let config_snapshot = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?
        .clone();
    let home_dir = dirs::home_dir();
    let current = read_skill_file_in_scope(
        std::path::Path::new(&file_path),
        home_dir.as_deref(),
        &config_snapshot.project_roots,
        &config_snapshot.custom_scan_paths,
    )?;
    let (detected_repo, detected_remote_ref) =
        read_repo_and_remote_ref_for_skill(&scan, &canonical_skill_id)?;

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    let reason_text = reason.unwrap_or_else(|| "before-update".to_string());
    let repo_ref = source_repo_url.or(detected_repo);
    let remote_ref = remote_ref.or(detected_remote_ref);
    let staged = skill_history::stage_snapshot(
        &config,
        &canonical_skill_id,
        &current,
        &reason_text,
        repo_ref.as_deref(),
        remote_ref.as_deref(),
    )?;
    let entry = staged.entry.clone();
    let original_config = config.clone();
    let mut committed_config = config.clone();
    let pruned_entries = skill_history::commit_staged_snapshot_for_skill(
        &mut committed_config,
        &canonical_skill_id,
        staged,
    );

    if let Err(error) = crate::commands::preferences::save_config(&committed_config) {
        *config = original_config;
        skill_history::remove_snapshot_files(std::slice::from_ref(&entry));
        return Err(error);
    }
    *config = committed_config;
    skill_history::remove_snapshot_files(&pruned_entries);
    drop(config);
    catalog.invalidate();
    Ok(entry)
}

#[tauri::command]
pub fn list_skill_versions(
    state: State<ConfigState>,
    catalog: State<SkillCatalogState>,
    skill_id: String,
) -> Result<SkillHistoryResponse, String> {
    let scan = scan_for_commands(&state, &catalog)?;
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSnapshotContentResponse {
    pub version_id: String,
    pub skill_id: String,
    pub created_at: u64,
    pub reason: String,
    pub content: String,
    pub source_repo_url: Option<String>,
    pub remote_ref: Option<String>,
}

/// Read the raw text content of one archived snapshot — used by the diff /
/// view dialog in the UI. Performs the integrity check baked into
/// `skill_history::load_snapshot`.
#[tauri::command]
pub fn read_skill_snapshot(
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
    skill_id: String,
    version_id: String,
) -> Result<SkillSnapshotContentResponse, String> {
    let scan = scan_for_commands(&state, &catalog)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    let entry = config
        .skill_version_history
        .get(&canonical_skill_id)
        .and_then(|entries| entries.iter().find(|e| e.version_id == version_id))
        .cloned()
        .ok_or_else(|| "Requested version not found".to_string())?;

    let snapshot = skill_history::load_snapshot(&entry)?;

    Ok(SkillSnapshotContentResponse {
        version_id: snapshot.version_id,
        skill_id: snapshot.skill_id,
        created_at: snapshot.created_at,
        reason: snapshot.reason,
        content: snapshot.content,
        source_repo_url: snapshot.source_repo_url,
        remote_ref: snapshot.remote_ref,
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSkillVersionResult {
    pub deleted: bool,
    pub version_id: String,
    pub remaining_entries: Vec<SkillVersionEntry>,
    pub archive_cleanup_pending: bool,
}

/// Permanently remove a single archived version. Returns the new history list
/// so the UI can re-render without an extra round trip.
#[tauri::command]
pub fn delete_skill_version(
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
    skill_id: String,
    version_id: String,
) -> Result<DeleteSkillVersionResult, String> {
    let scan = scan_for_commands(&state, &catalog)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    let original_config = config.clone();
    let outcome = skill_history::delete_version(&mut config, &canonical_skill_id, &version_id)?;
    if let Err(error) = crate::commands::preferences::save_config(&config) {
        *config = original_config;
        return Err(error);
    }
    let archive_cleanup_pending =
        skill_history::remove_snapshot_file(&outcome.removed_entry).is_err();
    drop(config);
    catalog.invalidate();

    Ok(DeleteSkillVersionResult {
        deleted: true,
        version_id,
        remaining_entries: outcome.remaining_entries,
        archive_cleanup_pending,
    })
}

#[tauri::command]
pub fn restore_skill_version(
    state: State<ConfigState>,
    catalog: State<SkillCatalogState>,
    skill_id: String,
    version_id: String,
    _project_path: Option<String>,
) -> Result<RestoreSkillVersionResult, String> {
    // Direct IPC callers can bypass the frontend's single-flight behaviour.
    // Serialize restores so history and live-file ordering stays deterministic.
    let _restore_guard = RESTORE_MUTATION_LOCK
        .lock()
        .map_err(|_| "Failed to lock restore mutation state".to_string())?;

    // A restore is destructive: do not authorize it from the short-lived
    // browsing snapshot. Resolve the current file, then bind its identity and
    // content through the write below.
    let restore_scope = {
        let config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        RestoreScope::from(&*config)
    };
    let scan = scan_for_mutation(&state, &catalog)?;
    let canonical_skill_id = resolve_canonical_skill_id(&scan, &skill_id);
    let file_path = scan_skill_file_path(&scan, &canonical_skill_id)?;
    let home_dir = dirs::home_dir();

    let mut config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?;

    // Materialize and validate the selected archive before taking the rollback
    // snapshot. At the history cap, the rollback snapshot may evict the oldest
    // archive, including the selected target. The returned content is now a
    // verified in-memory value, rather than a path that eviction can remove.
    let snapshot = skill_history::load_restore_target(&config, &canonical_skill_id, &version_id)?;

    // Check immediately before reading: a root removed after the fresh scan
    // must not remain authorized by an older configuration snapshot.
    ensure_restore_scope_is_current(&restore_scope, &config)?;
    let (current, current_binding) = read_skill_file_with_binding_in_scope(
        std::path::Path::new(&file_path),
        home_dir.as_deref(),
        &restore_scope.project_roots,
        &restore_scope.custom_scan_paths,
    )?;

    // Stage the rollback snapshot, then make that history entry durable before
    // changing the live file. This keeps the pre-restore content recoverable
    // even if the following atomic replacement fails.
    let staged_rollback = skill_history::stage_snapshot(
        &config,
        &canonical_skill_id,
        &current,
        "before-restore",
        None,
        None,
    )?;
    let staged_entry = staged_rollback.entry.clone();
    let original_config = config.clone();
    // Persist cap-plus-one before the live write. A crash or write failure in
    // this interval retains both the selected target and rollback archive,
    // rather than durably pruning an archive for a restore that never landed.
    let mut staged_config = config.clone();
    skill_history::append_staged_snapshot_for_skill(
        &mut staged_config,
        &canonical_skill_id,
        staged_rollback,
    );

    if let Err(error) = crate::commands::preferences::save_config(&staged_config) {
        skill_history::remove_snapshot_files(std::slice::from_ref(&staged_entry));
        return Err(error);
    }
    *config = staged_config;

    // Keep the final authorization explicit next to the destructive write.
    // The config mutex prevents a concurrent preference mutation after the
    // previous check, while the file binding below rejects file replacement or
    // content drift in the interval after the read.
    if let Err(error) = ensure_restore_scope_is_current(&restore_scope, &config) {
        if let Err(rollback_error) =
            rollback_restore_transaction(&mut config, &original_config, &staged_entry)
        {
            return Err(format!(
                "Restore stopped before the live write: {error}; history rollback also failed: {rollback_error}"
            ));
        }
        return Err(error);
    }
    if let Err(error) = write_skill_file_in_scope(
        std::path::Path::new(&file_path),
        &snapshot.content,
        &current_binding,
        home_dir.as_deref(),
        &restore_scope.project_roots,
        &restore_scope.custom_scan_paths,
    ) {
        // Once the bound-handle write begins, an I/O failure can leave the
        // live bytes partially changed. Keep the already-durable rollback
        // archive and cap-plus-one index in that case instead of deleting the
        // only guaranteed recovery path.
        if error.starts_with("Restore write may have modified the bound file") {
            drop(config);
            catalog.invalidate();
            return Err(format!(
                "Restore may have partially changed the live skill; the durable rollback archive is retained: {error}"
            ));
        }
        if let Err(rollback_error) =
            rollback_restore_transaction(&mut config, &original_config, &staged_entry)
        {
            return Err(format!(
                "Restore did not change the live skill: {error}; history rollback also failed: {rollback_error}"
            ));
        }
        return Err(format!(
            "Restore did not change the live skill; the history transaction was rolled back: {error}"
        ));
    }

    // Only a successful live restore may trim history. If this final save
    // fails, leave the safe cap-plus-one state intact rather than losing an
    // archive after the live file changed.
    let mut final_config = config.clone();
    let pruned_entries =
        skill_history::prune_history_to_cap(&mut final_config, &canonical_skill_id);
    if let Err(error) = crate::commands::preferences::save_config(&final_config) {
        drop(config);
        catalog.invalidate();
        return Err(format!(
            "Restore changed the live skill, but history retention is pending: {error}"
        ));
    }
    *config = final_config;
    skill_history::remove_snapshot_files(&pruned_entries);
    drop(config);
    catalog.invalidate();
    Ok(RestoreSkillVersionResult {
        restored: true,
        version_id,
    })
}

/// Search a single marketplace provider (skills.sh, ClawHub, ...).
#[tauri::command]
pub async fn search_marketplace(
    provider: ProviderId,
    query: String,
    limit: Option<u32>,
) -> Result<MarketplaceSearchResponse, String> {
    let normalized_limit = marketplaces::normalize_limit(limit.unwrap_or(20) as usize);
    marketplaces::search(provider, &query, normalized_limit)
        .await
        .map_err(|e| e.to_string())
}

/// Fan-out search across every known marketplace in parallel.
#[tauri::command]
pub async fn search_marketplaces_aggregated(
    query: String,
    limit: Option<u32>,
) -> Result<AggregatedMarketplaceResponse, String> {
    let normalized_limit = marketplaces::normalize_limit(limit.unwrap_or(20) as usize);
    let providers = marketplaces::search_all(&query, normalized_limit).await;
    Ok(AggregatedMarketplaceResponse { query, providers })
}

#[cfg(test)]
mod restore_scope_tests {
    use super::*;

    #[test]
    fn restore_rejects_project_root_removed_after_scope_snapshot() {
        let expected = RestoreScope {
            project_roots: vec!["C:/synthetic/project".to_string()],
            custom_scan_paths: vec!["C:/synthetic/custom/SKILL.md".to_string()],
        };
        let current = AppConfig::default();

        let error = ensure_restore_scope_is_current(&expected, &current)
            .expect_err("a removed configured root must revoke restore authorization");
        assert!(error.contains("scope changed"));
    }
}
