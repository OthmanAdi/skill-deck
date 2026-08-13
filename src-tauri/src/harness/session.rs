// @agent-context: HarnessSession persistence. One JSON file per session at
//   {config_dir}/skill-deck/harness-sessions/<id>.json
// matches the existing archive-snapshot convention.
//
// Durability rules (daily-driver grade):
// - Writes are ATOMIC: serialize → write `<file>.tmp-<pid>` in the SAME dir →
//   rename over the target. A crash mid-write can never leave a half-written
//   session; the old file survives until the rename.
// - Reads are CORRUPTION-TOLERANT: a file that fails to parse is quarantined
//   as `<file>.corrupt` (never deleted, never silently overwritten) and the
//   caller starts a fresh session. We log, we never crash.
// - `list()` strips transcripts: the session picker only needs metadata, and
//   shipping hundreds of full transcripts over IPC froze the WebView. The
//   full transcript loads on demand via `load()` / `harness_get_session`.

use crate::harness::memory::conversation::ConversationMemory;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSession {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub memory: ConversationMemory,
    #[serde(default)]
    pub final_summary: Option<String>,
}

pub fn sessions_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir")?;
    let dir = base.join("skill-deck").join("harness-sessions");
    fs::create_dir_all(&dir).context("create sessions dir")?;
    Ok(dir)
}

pub fn session_path(id: &str) -> Result<PathBuf> {
    Ok(session_path_in(&sessions_dir()?, id))
}

fn session_path_in(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{}.json", sanitize(id)))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Write `contents` to `path` atomically: temp file in the same directory,
/// then rename over the target (same-volume rename is atomic on Windows/
/// Unix; std's Windows rename uses MOVEFILE_REPLACE_EXISTING).
/// The temp name carries pid + a process-wide counter so concurrent writers
/// (e.g. two IPC commands saving config) never share a temp file.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITE_SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().context("target path has no parent dir")?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("target path has no file name")?;
    let tmp = dir.join(format!(
        "{file_name}.tmp-{}-{}",
        std::process::id(),
        WRITE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, contents).with_context(|| format!("write temp file {tmp:?}"))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Best-effort cleanup; the temp file is harmless if it survives.
            let _ = fs::remove_file(&tmp);
            Err(e).with_context(|| format!("rename {tmp:?} → {path:?}"))
        }
    }
}

/// Move an unparseable file aside as `<file>.corrupt` so the user's data is
/// never destroyed and the app can start fresh. Any pre-existing quarantine
/// file for the same path is replaced (the newest corruption wins).
fn quarantine_corrupt(path: &Path) {
    let corrupt = PathBuf::from(format!("{}.corrupt", path.display()));
    let _ = fs::remove_file(&corrupt);
    match fs::rename(path, &corrupt) {
        Ok(()) => log::warn!("quarantined corrupt file {path:?} → {corrupt:?}"),
        Err(e) => log::warn!("failed to quarantine corrupt file {path:?}: {e}"),
    }
}

pub fn save(session: &HarnessSession) -> Result<()> {
    save_in(&sessions_dir()?, session)
}

pub(crate) fn save_in(dir: &Path, session: &HarnessSession) -> Result<()> {
    let path = session_path_in(dir, &session.id);
    let json = serde_json::to_string_pretty(session).context("serialize session")?;
    atomic_write(&path, &json).context("write session")
}

pub fn load(id: &str) -> Result<HarnessSession> {
    load_in(&sessions_dir()?, id)
}

pub(crate) fn load_in(dir: &Path, id: &str) -> Result<HarnessSession> {
    let path = session_path_in(dir, id);
    let raw = fs::read_to_string(&path).context("read session")?;
    match serde_json::from_str(&raw) {
        Ok(parsed) => Ok(parsed),
        Err(e) => {
            quarantine_corrupt(&path);
            Err(e).context("parse session (quarantined as .corrupt, starting fresh)")
        }
    }
}

/// List sessions for the picker — metadata only. Each entry's transcript
/// (`memory.messages` / `spilled_blocks`) is cleared so a user with hundreds
/// of long sessions gets a small payload; `load()` returns the full session.
pub fn list() -> Vec<HarnessSession> {
    match sessions_dir() {
        Ok(d) => list_in(&d),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn list_in(dir: &Path) -> Vec<HarnessSession> {
    let mut out = Vec::new();
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in read.flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        match serde_json::from_str::<HarnessSession>(&raw) {
            Ok(mut session) => {
                // Bound the payload: the picker renders title/provider/model
                // only, so drop the transcript body.
                session.memory.messages.clear();
                session.memory.spilled_blocks.clear();
                session.memory.running_summary = None;
                out.push(session);
            }
            Err(_) => {
                // Corrupt session on disk: put it aside once so it stops
                // erroring every refresh, keep listing the rest.
                quarantine_corrupt(&entry.path());
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    out
}

pub fn delete(id: &str) -> Result<()> {
    delete_in(&sessions_dir()?, id)
}

pub(crate) fn delete_in(dir: &Path, id: &str) -> Result<()> {
    let path = session_path_in(dir, id);
    if path.exists() {
        fs::remove_file(path).context("delete session")?;
    }
    Ok(())
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str, updated_at: u64) -> HarnessSession {
        let mut memory = ConversationMemory::new(id);
        memory.push_user("hello");
        memory.push_assistant("world");
        HarnessSession {
            id: id.to_string(),
            title: format!("title-{id}"),
            provider_id: "ollama-local".into(),
            model: "hermes3:8b".into(),
            created_at: 1,
            updated_at,
            memory,
            final_summary: None,
        }
    }

    #[test]
    fn save_load_roundtrip_in_temp_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let session = sample("s1", 10);
        save_in(dir.path(), &session).expect("save");
        let loaded = load_in(dir.path(), "s1").expect("load");
        assert_eq!(loaded.id, "s1");
        assert_eq!(loaded.memory.messages.len(), 2);
        // No stray temp files left behind.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .flatten()
            .filter(|e| e.path().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files must be renamed away");
    }

    #[test]
    fn save_replaces_existing_file_atomically() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut session = sample("s2", 10);
        save_in(dir.path(), &session).expect("first save");
        session.title = "updated".into();
        session.updated_at = 20;
        save_in(dir.path(), &session).expect("second save over existing");
        let loaded = load_in(dir.path(), "s2").expect("load");
        assert_eq!(loaded.title, "updated");
        assert_eq!(loaded.updated_at, 20);
    }

    #[test]
    fn corrupt_session_is_quarantined_not_deleted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bad.json");
        fs::write(&path, "{ this is not json").expect("write corrupt");
        let err = load_in(dir.path(), "bad").expect_err("must fail to parse");
        assert!(err.to_string().contains("quarantined"), "err: {err:#}");
        assert!(!path.exists(), "original must be moved aside");
        let corrupt = dir.path().join("bad.json.corrupt");
        assert!(corrupt.exists(), "quarantine file must exist");
        assert_eq!(
            fs::read_to_string(corrupt).expect("read quarantine"),
            "{ this is not json",
            "user data preserved byte-for-byte"
        );
    }

    #[test]
    fn list_skips_corrupt_files_and_strips_transcripts() {
        let dir = tempfile::tempdir().expect("temp dir");
        save_in(dir.path(), &sample("good-a", 5)).expect("save a");
        save_in(dir.path(), &sample("good-b", 9)).expect("save b");
        fs::write(dir.path().join("broken.json"), "not json at all").expect("write corrupt");

        let listed = list_in(dir.path());
        assert_eq!(listed.len(), 2, "corrupt entry skipped");
        // Sorted newest first.
        assert_eq!(listed[0].id, "good-b");
        assert_eq!(listed[1].id, "good-a");
        // Transcript stripped, metadata kept.
        assert!(listed[0].memory.messages.is_empty());
        assert_eq!(listed[0].title, "title-good-b");
        // The corrupt file was quarantined so the next list is clean.
        assert!(dir.path().join("broken.json.corrupt").exists());
        assert!(!dir.path().join("broken.json").exists());
        // Full transcript still available through load.
        let full = load_in(dir.path(), "good-b").expect("load full");
        assert_eq!(full.memory.messages.len(), 2);
    }

    #[test]
    fn delete_is_idempotent() {
        let dir = tempfile::tempdir().expect("temp dir");
        save_in(dir.path(), &sample("s3", 1)).expect("save");
        delete_in(dir.path(), "s3").expect("delete existing");
        delete_in(dir.path(), "s3").expect("delete missing is ok");
    }

    #[test]
    fn session_ids_are_sanitized_for_paths() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut session = sample("weird/../id", 1);
        session.id = "weird/../id".into();
        save_in(dir.path(), &session).expect("save");
        // Path traversal characters are flattened; file lands inside dir.
        let entries: Vec<_> = fs::read_dir(dir.path()).expect("read").flatten().collect();
        assert_eq!(entries.len(), 1);
        let loaded = load_in(dir.path(), "weird/../id").expect("load back");
        assert_eq!(loaded.id, "weird/../id");
    }
}
