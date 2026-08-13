// @agent-context: Filesystem backed skill version history for local snapshots.
//
// We keep snapshots as JSON under config_dir/skill-deck/history/{skill_id_sanitized}/
// and store the metadata index inside AppConfig.skill_version_history.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{AppConfig, SkillVersionEntry};

const MAX_REASON_LEN: usize = 48;
const MAX_SOURCE_URL_LEN: usize = 512;
const MAX_REMOTE_REF_LEN: usize = 256;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersionSnapshot {
    pub version_id: String,
    pub skill_id: String,
    pub created_at: u64,
    pub reason: String,
    pub source_repo_url: Option<String>,
    pub remote_ref: Option<String>,
    pub content_hash: String,
    pub content: String,
}

/// A snapshot that is safely written to disk but not yet indexed in config.
/// Restore uses this two-step form so it can persist the rollback entry before
/// touching the live skill file.
#[derive(Debug, Clone)]
pub struct StagedSnapshot {
    pub entry: SkillVersionEntry,
}

#[derive(Debug, Clone)]
pub struct DeleteVersionOutcome {
    pub remaining_entries: Vec<SkillVersionEntry>,
    pub removed_entry: SkillVersionEntry,
}

pub fn sanitize_skill_id(skill_id: &str) -> String {
    let mut out = String::with_capacity(skill_id.len());
    for ch in skill_id.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "skill".to_string()
    } else {
        out
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    to_hex(&hasher.finalize())
}

fn sanitize_reason(reason: &str) -> String {
    let mut cleaned = reason
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ' | '/'))
        .collect::<String>()
        .trim()
        .to_string();

    if cleaned.is_empty() {
        cleaned = "snapshot".to_string();
    }

    if cleaned.len() > MAX_REASON_LEN {
        cleaned.truncate(MAX_REASON_LEN);
    }

    cleaned
}

fn sanitize_opt_value(value: Option<&str>, max_len: usize) -> Option<String> {
    value.map(|v| v.trim()).filter(|v| !v.is_empty()).map(|v| {
        let mut s = v.to_string();
        if s.len() > max_len {
            s.truncate(max_len);
        }
        s
    })
}

pub fn history_root_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("skill-deck");
    path.push("history");
    path
}

fn history_dir_for_skill(skill_id: &str) -> PathBuf {
    history_root_dir().join(sanitize_skill_id(skill_id))
}

#[cfg(test)]
pub fn create_snapshot(
    config: &mut AppConfig,
    skill_id: &str,
    content: &str,
    reason: &str,
    source_repo_url: Option<&str>,
    remote_ref: Option<&str>,
) -> Result<SkillVersionEntry, String> {
    let staged = stage_snapshot(
        config,
        skill_id,
        content,
        reason,
        source_repo_url,
        remote_ref,
    )?;
    let entry = staged.entry.clone();
    let _removed = commit_staged_snapshot_for_skill(config, skill_id, staged);
    Ok(entry)
}

/// Write a collision-safe snapshot file without changing its config index.
pub fn stage_snapshot(
    config: &AppConfig,
    skill_id: &str,
    content: &str,
    reason: &str,
    source_repo_url: Option<&str>,
    remote_ref: Option<&str>,
) -> Result<StagedSnapshot, String> {
    let created_at = now_secs();
    let reason = sanitize_reason(reason);
    let content_hash = hash_content(content);
    let short_hash = &content_hash[..12];
    let base_version_id = format!("{}-{}", created_at, short_hash);
    let skill_dir = history_dir_for_skill(skill_id);
    std::fs::create_dir_all(&skill_dir).map_err(|e| {
        format!(
            "Failed to create history directory {}: {}",
            skill_dir.to_string_lossy(),
            e
        )
    })?;

    let version_id = unique_version_id(config, skill_id, &skill_dir, &base_version_id)?;
    let file_name = format!("{}.json", version_id);
    let snapshot_path = skill_dir.join(&file_name);
    let snapshot = SkillVersionSnapshot {
        version_id: version_id.clone(),
        skill_id: skill_id.to_string(),
        created_at,
        reason,
        source_repo_url: sanitize_opt_value(source_repo_url, MAX_SOURCE_URL_LEN),
        remote_ref: sanitize_opt_value(remote_ref, MAX_REMOTE_REF_LEN),
        content_hash: content_hash.clone(),
        content: content.to_string(),
    };

    let json = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| format!("Failed to serialize skill snapshot: {}", e))?;
    crate::harness::session::atomic_write(&snapshot_path, &json).map_err(|e| {
        format!(
            "Failed to write skill snapshot {}: {}",
            snapshot_path.to_string_lossy(),
            e
        )
    })?;

    let relative_snapshot_path = format!("{}/{}", sanitize_skill_id(skill_id), file_name);

    let entry = SkillVersionEntry {
        version_id,
        created_at,
        reason: snapshot.reason,
        source_repo_url: snapshot.source_repo_url,
        remote_ref: snapshot.remote_ref,
        content_hash,
        content_bytes: content.len() as u64,
        snapshot_path: relative_snapshot_path,
    };

    Ok(StagedSnapshot { entry })
}

/// Commit a staged snapshot to the exact original skill ID. This preserves
/// literal IDs that sanitize to the same directory name.
pub fn commit_staged_snapshot_for_skill(
    config: &mut AppConfig,
    skill_id: &str,
    staged: StagedSnapshot,
) -> Vec<SkillVersionEntry> {
    append_staged_snapshot_for_skill(config, skill_id, staged);
    prune_history_to_cap(config, skill_id)
}

/// Add a staged snapshot without applying retention. Restore persists this
/// cap-plus-one state before the live write so a crash cannot orphan the
/// selected target while the operation is in flight.
pub fn append_staged_snapshot_for_skill(
    config: &mut AppConfig,
    skill_id: &str,
    staged: StagedSnapshot,
) {
    let history = config
        .skill_version_history
        .entry(skill_id.to_string())
        .or_default();
    history.push(staged.entry);
    history.sort_by_key(|v| v.created_at);
    history.reverse();
}

/// Apply retention only after the caller has durably committed a successful
/// live restore. The returned files are deleted only after that config save.
pub fn prune_history_to_cap(config: &mut AppConfig, skill_id: &str) -> Vec<SkillVersionEntry> {
    let Some(history) = config.skill_version_history.get_mut(skill_id) else {
        return Vec::new();
    };

    let max = config.max_skill_history_entries.max(1);
    if history.len() > max {
        history.split_off(max)
    } else {
        Vec::new()
    }
}

pub fn remove_snapshot_files(entries: &[SkillVersionEntry]) {
    for entry in entries {
        let _ = std::fs::remove_file(snapshot_path_from_entry(entry));
    }
}

/// Revert the in-memory half of a staged snapshot transaction after its
/// durable config commit has been rolled back. The caller must persist the
/// original config first; this function only restores the matching memory
/// state and removes the now-unindexed staged file.
pub fn rollback_staged_snapshot(
    config: &mut AppConfig,
    original_config: &AppConfig,
    staged_entry: &SkillVersionEntry,
) {
    *config = original_config.clone();
    remove_snapshot_files(std::slice::from_ref(staged_entry));
}

fn unique_version_id(
    config: &AppConfig,
    skill_id: &str,
    skill_dir: &Path,
    base_version_id: &str,
) -> Result<String, String> {
    for suffix in 0..10_000u32 {
        let candidate = if suffix == 0 {
            base_version_id.to_string()
        } else {
            format!("{base_version_id}-{suffix}")
        };
        let already_indexed = config
            .skill_version_history
            .get(skill_id)
            .is_some_and(|entries| entries.iter().any(|entry| entry.version_id == candidate));
        if !already_indexed && !skill_dir.join(format!("{candidate}.json")).exists() {
            return Ok(candidate);
        }
    }

    Err("Failed to allocate a unique snapshot version ID".to_string())
}

fn snapshot_path_from_entry(entry: &SkillVersionEntry) -> PathBuf {
    history_root_dir().join(&entry.snapshot_path)
}

pub fn load_snapshot(entry: &SkillVersionEntry) -> Result<SkillVersionSnapshot, String> {
    let path = snapshot_path_from_entry(entry);
    let snapshot = load_snapshot_from_path(&path)?;

    // The metadata index is the caller's authorization for this archive. A
    // snapshot that remains internally well-formed but no longer matches its
    // index entry must never be restored.
    if snapshot.version_id != entry.version_id
        || snapshot.created_at != entry.created_at
        || snapshot.content_hash != entry.content_hash
        || snapshot.content.len() as u64 != entry.content_bytes
    {
        return Err("Snapshot no longer matches its history index".to_string());
    }

    Ok(snapshot)
}

/// Load one indexed version for a destructive restore before adding the
/// rollback snapshot. This pins verified bytes in memory so cap eviction
/// cannot remove the restore target between selection and write.
pub fn load_restore_target(
    config: &AppConfig,
    skill_id: &str,
    version_id: &str,
) -> Result<SkillVersionSnapshot, String> {
    let entry = config
        .skill_version_history
        .get(skill_id)
        .and_then(|entries| entries.iter().find(|entry| entry.version_id == version_id))
        .ok_or_else(|| "Requested version not found".to_string())?;
    let snapshot = load_snapshot(entry)?;

    if snapshot.skill_id != skill_id {
        return Err("Snapshot skill identity does not match its history index".to_string());
    }

    Ok(snapshot)
}

pub fn load_snapshot_from_path(path: &Path) -> Result<SkillVersionSnapshot, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read snapshot {}: {}", path.to_string_lossy(), e))?;
    let snapshot: SkillVersionSnapshot = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse snapshot {}: {}", path.to_string_lossy(), e))?;

    let expected_hash = hash_content(&snapshot.content);
    if expected_hash != snapshot.content_hash {
        return Err(format!(
            "Snapshot integrity check failed for {}",
            path.to_string_lossy()
        ));
    }

    Ok(snapshot)
}

pub fn get_history(config: &AppConfig, skill_id: &str) -> Vec<SkillVersionEntry> {
    config
        .skill_version_history
        .get(skill_id)
        .cloned()
        .unwrap_or_default()
}

/// Remove a version from the in-memory history index only. The caller must
/// persist that index before deleting `removed_entry` from disk.
pub fn delete_version(
    config: &mut AppConfig,
    skill_id: &str,
    version_id: &str,
) -> Result<DeleteVersionOutcome, String> {
    let history = config
        .skill_version_history
        .get_mut(skill_id)
        .ok_or_else(|| "No history found for skill".to_string())?;

    let position = history
        .iter()
        .position(|entry| entry.version_id == version_id)
        .ok_or_else(|| "Requested version not found".to_string())?;

    let removed = history.remove(position);

    // Drop the per-skill bucket entirely when it becomes empty so the next
    // scan stops reporting archiveCount > 0.
    if history.is_empty() {
        config.skill_version_history.remove(skill_id);
        return Ok(DeleteVersionOutcome {
            remaining_entries: Vec::new(),
            removed_entry: removed,
        });
    }

    Ok(DeleteVersionOutcome {
        remaining_entries: history.clone(),
        removed_entry: removed,
    })
}

/// Remove a no-longer-indexed snapshot after its history config is durable.
/// A deletion failure leaves only an unreferenced archive, never a persisted
/// index pointing at missing data.
pub fn remove_snapshot_file(entry: &SkillVersionEntry) -> Result<(), String> {
    #[cfg(test)]
    {
        use std::sync::atomic::Ordering;
        if FAIL_NEXT_SNAPSHOT_DELETE.swap(false, Ordering::SeqCst) {
            return Err("Injected snapshot cleanup failure".to_string());
        }
    }

    match std::fs::remove_file(snapshot_path_from_entry(entry)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Snapshot cleanup is pending".to_string()),
    }
}

#[cfg(test)]
static FAIL_NEXT_SNAPSHOT_DELETE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppConfig;

    #[test]
    fn test_sanitize_skill_id() {
        assert_eq!(
            sanitize_skill_id("claude-code:skill/path"),
            "claude-code_skill_path"
        );
    }

    #[test]
    fn test_hash_content_differs() {
        let a = hash_content("one");
        let b = hash_content("two");
        assert_ne!(a, b);
    }

    #[test]
    fn test_create_snapshot_adds_history_entry() {
        let mut config = AppConfig::default();
        let result = create_snapshot(
            &mut config,
            "skill-a",
            "content",
            "before-update",
            Some("https://github.com/owner/repo"),
            Some("abc"),
        );
        assert!(result.is_ok());
        assert_eq!(
            config.skill_version_history.get("skill-a").map(|v| v.len()),
            Some(1)
        );
    }

    #[test]
    fn duplicate_content_snapshots_get_distinct_files_and_delete_independently() {
        let skill_id = format!(
            "duplicate-snapshot-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let mut config = AppConfig::default();
        let first = create_snapshot(
            &mut config,
            &skill_id,
            "unchanged content",
            "before-update",
            None,
            None,
        )
        .expect("first snapshot");
        let second = create_snapshot(
            &mut config,
            &skill_id,
            "unchanged content",
            "before-update",
            None,
            None,
        )
        .expect("second snapshot");

        assert_ne!(first.version_id, second.version_id);
        assert_ne!(first.snapshot_path, second.snapshot_path);
        assert!(snapshot_path_from_entry(&first).exists());
        assert!(snapshot_path_from_entry(&second).exists());

        let deletion = delete_version(&mut config, &skill_id, &first.version_id)
            .expect("deleting first snapshot succeeds");
        remove_snapshot_file(&deletion.removed_entry).expect("snapshot cleanup succeeds");
        assert!(!snapshot_path_from_entry(&first).exists());
        assert!(snapshot_path_from_entry(&second).exists());
        assert!(load_snapshot(&second).is_ok());

        let _ = std::fs::remove_dir_all(history_dir_for_skill(&skill_id));
    }

    #[test]
    fn failed_delete_persistence_rolls_back_index_before_any_snapshot_is_removed() {
        let skill_id = format!(
            "delete-rollback-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let mut config = AppConfig::default();
        let entry = create_snapshot(
            &mut config,
            &skill_id,
            "durable archive",
            "before-update",
            None,
            None,
        )
        .expect("snapshot");
        let original = config.clone();

        let deletion =
            delete_version(&mut config, &skill_id, &entry.version_id).expect("index deletion");
        assert!(snapshot_path_from_entry(&deletion.removed_entry).exists());

        // Simulate a config-save failure: command code restores this exact
        // durable index before any archive cleanup is attempted.
        config = original;
        assert!(load_restore_target(&config, &skill_id, &entry.version_id).is_ok());
        assert!(snapshot_path_from_entry(&entry).exists());

        let _ = std::fs::remove_dir_all(history_dir_for_skill(&skill_id));
    }

    #[test]
    fn cleanup_failure_leaves_an_unreferenced_snapshot_recoverable_for_retry() {
        use std::sync::atomic::Ordering;

        let skill_id = format!(
            "delete-cleanup-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let mut config = AppConfig::default();
        let entry = create_snapshot(
            &mut config,
            &skill_id,
            "archive awaiting cleanup",
            "before-update",
            None,
            None,
        )
        .expect("snapshot");
        let deletion =
            delete_version(&mut config, &skill_id, &entry.version_id).expect("index deletion");

        FAIL_NEXT_SNAPSHOT_DELETE.store(true, Ordering::SeqCst);
        assert!(remove_snapshot_file(&deletion.removed_entry).is_err());
        assert!(snapshot_path_from_entry(&entry).exists());
        assert!(!config.skill_version_history.contains_key(&skill_id));

        remove_snapshot_file(&deletion.removed_entry).expect("cleanup retry succeeds");
        assert!(!snapshot_path_from_entry(&entry).exists());

        let _ = std::fs::remove_dir_all(history_dir_for_skill(&skill_id));
    }

    #[test]
    fn failed_live_write_rolls_back_capped_history_and_keeps_selected_target_readable() {
        let skill_id = format!(
            "restore-cap-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let mut config = AppConfig {
            max_skill_history_entries: 40,
            ..AppConfig::default()
        };

        let skill_dir = history_dir_for_skill(&skill_id);
        std::fs::create_dir_all(&skill_dir).expect("history directory");
        let mut entries = Vec::new();
        for index in 0..40u64 {
            let content = format!("archived-content-{index}");
            let version_id = format!("{index:04}-fixture");
            let entry = SkillVersionEntry {
                version_id: version_id.clone(),
                created_at: index,
                reason: "before-update".to_string(),
                source_repo_url: None,
                remote_ref: None,
                content_hash: hash_content(&content),
                content_bytes: content.len() as u64,
                snapshot_path: format!("{}/{}.json", sanitize_skill_id(&skill_id), version_id),
            };
            let snapshot = SkillVersionSnapshot {
                version_id,
                skill_id: skill_id.clone(),
                created_at: index,
                reason: entry.reason.clone(),
                source_repo_url: None,
                remote_ref: None,
                content_hash: entry.content_hash.clone(),
                content,
            };
            std::fs::write(
                snapshot_path_from_entry(&entry),
                serde_json::to_string(&snapshot).expect("fixture snapshot serializes"),
            )
            .expect("fixture snapshot writes");
            entries.push(entry);
        }
        entries.reverse();
        config
            .skill_version_history
            .insert(skill_id.clone(), entries);

        let target = config
            .skill_version_history
            .get(&skill_id)
            .and_then(|entries| entries.last())
            .cloned()
            .expect("the oldest archive should exist at the cap");
        let pinned = load_restore_target(&config, &skill_id, &target.version_id)
            .expect("the selected archive should load before eviction");

        let original_config = config.clone();
        let rollback = stage_snapshot(
            &config,
            &skill_id,
            "current-content-before-restore",
            "before-restore",
            None,
            None,
        )
        .expect("rollback archive should stage");
        let staged_entry = rollback.entry.clone();
        let pruned = commit_staged_snapshot_for_skill(&mut config, &skill_id, rollback);

        let retained = config
            .skill_version_history
            .get(&skill_id)
            .expect("history should still exist at cap");
        assert_eq!(retained.len(), 40);
        assert!(
            !retained
                .iter()
                .any(|entry| entry.version_id == target.version_id),
            "the oldest selected archive should be evicted by the rollback snapshot"
        );
        assert!(
            snapshot_path_from_entry(&target).exists(),
            "pruned files stay present until a live restore succeeds"
        );
        assert_eq!(pinned.content, "archived-content-0");

        // This is the error path after an injected atomic live-write failure:
        // the old, durable config is restored before staged data is cleaned.
        rollback_staged_snapshot(&mut config, &original_config, &staged_entry);
        assert!(
            config
                .skill_version_history
                .get(&skill_id)
                .is_some_and(|entries| entries
                    .iter()
                    .any(|entry| entry.version_id == target.version_id)),
            "the cap-selected target must be re-indexed after a failed live write"
        );
        assert!(
            load_restore_target(&config, &skill_id, &target.version_id).is_ok(),
            "the re-indexed target must remain readable"
        );
        assert!(
            !snapshot_path_from_entry(&staged_entry).exists(),
            "the uncommitted rollback archive must be cleaned after config rollback"
        );
        assert_eq!(pruned.len(), 1);

        let _ = std::fs::remove_dir_all(history_dir_for_skill(&skill_id));
    }

    #[test]
    fn restore_target_rejects_snapshot_that_no_longer_matches_history_index() {
        let skill_id = format!(
            "restore-integrity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        );
        let mut config = AppConfig::default();
        let entry = create_snapshot(
            &mut config,
            &skill_id,
            "original-content",
            "before-update",
            None,
            None,
        )
        .expect("archive should be created");

        let replaced_content = "different-but-valid-content";
        let changed = SkillVersionSnapshot {
            version_id: entry.version_id.clone(),
            skill_id: skill_id.clone(),
            created_at: entry.created_at,
            reason: entry.reason.clone(),
            source_repo_url: entry.source_repo_url.clone(),
            remote_ref: entry.remote_ref.clone(),
            content_hash: hash_content(replaced_content),
            content: replaced_content.to_string(),
        };
        std::fs::write(
            snapshot_path_from_entry(&entry),
            serde_json::to_string(&changed).expect("replacement snapshot should serialize"),
        )
        .expect("replacement snapshot should write");

        let error = load_restore_target(&config, &skill_id, &entry.version_id)
            .expect_err("changed snapshot must fail closed");
        assert!(error.contains("no longer matches"));

        let _ = std::fs::remove_dir_all(history_dir_for_skill(&skill_id));
    }
}
