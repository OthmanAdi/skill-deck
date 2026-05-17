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

pub fn create_snapshot(
    config: &mut AppConfig,
    skill_id: &str,
    content: &str,
    reason: &str,
    source_repo_url: Option<&str>,
    remote_ref: Option<&str>,
) -> Result<SkillVersionEntry, String> {
    let created_at = now_secs();
    let reason = sanitize_reason(reason);
    let content_hash = hash_content(content);
    let short_hash = &content_hash[..12];
    let version_id = format!("{}-{}", created_at, short_hash);
    let skill_dir = history_dir_for_skill(skill_id);
    std::fs::create_dir_all(&skill_dir).map_err(|e| {
        format!(
            "Failed to create history directory {}: {}",
            skill_dir.to_string_lossy(),
            e
        )
    })?;

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
    std::fs::write(&snapshot_path, json).map_err(|e| {
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

    let history = config
        .skill_version_history
        .entry(skill_id.to_string())
        .or_default();
    history.push(entry.clone());
    history.sort_by_key(|v| v.created_at);
    history.reverse();

    let max = config.max_skill_history_entries.max(1);
    if history.len() > max {
        let removed = history.split_off(max);
        for old in removed {
            let absolute = history_root_dir().join(&old.snapshot_path);
            let _ = std::fs::remove_file(absolute);
        }
    }

    Ok(entry)
}

fn snapshot_path_from_entry(entry: &SkillVersionEntry) -> PathBuf {
    history_root_dir().join(&entry.snapshot_path)
}

pub fn load_snapshot(entry: &SkillVersionEntry) -> Result<SkillVersionSnapshot, String> {
    let path = snapshot_path_from_entry(entry);
    load_snapshot_from_path(&path)
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

/// Remove a single archived version (snapshot file + history index entry).
/// Returns the remaining history list for the skill so the UI can refresh
/// without a follow-up query.
pub fn delete_version(
    config: &mut AppConfig,
    skill_id: &str,
    version_id: &str,
) -> Result<Vec<SkillVersionEntry>, String> {
    let history = config
        .skill_version_history
        .get_mut(skill_id)
        .ok_or_else(|| "No history found for skill".to_string())?;

    let position = history
        .iter()
        .position(|entry| entry.version_id == version_id)
        .ok_or_else(|| "Requested version not found".to_string())?;

    let removed = history.remove(position);

    // Best-effort delete — a missing snapshot file should not block removing
    // the index entry, since the user's intent is clear.
    let absolute = history_root_dir().join(&removed.snapshot_path);
    let _ = std::fs::remove_file(absolute);

    // Drop the per-skill bucket entirely when it becomes empty so the next
    // scan stops reporting archiveCount > 0.
    if history.is_empty() {
        config.skill_version_history.remove(skill_id);
        return Ok(Vec::new());
    }

    Ok(history.clone())
}

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
}
