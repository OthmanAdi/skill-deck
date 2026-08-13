// @agent-context: Skill scanning commands exposed to the frontend.
// These are the primary data-fetching commands the UI calls on startup and refresh.

use crate::agents::locations::{
    capture_mode_requested, capture_root_mode_from_process, CaptureRootMode,
};
use crate::agents::{detect_installed_agents, scan_all_skills_with_roots, scan_custom_paths};
use crate::commands::preferences::ConfigState;
use crate::detection::repo_detector::{apply_overrides, DetectedSource};
use crate::detection::skill_discovery::enrich_skill_discovery;
use crate::detection::skill_identity::{
    dedupe_skills_by_source, remap_config_ids_for_skills, ConfigIdCollections,
};
use crate::models::{AgentInfo, AppConfig, ScanResult, Skill, SkillInstallEntry};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

const SKILL_CATALOG_TTL: Duration = Duration::from_secs(60);

/// Process-local scan snapshot. It deliberately stores discovery metadata only:
/// raw skill content remains on disk and is always re-validated before reading.
#[derive(Debug, Clone)]
struct SkillCatalogSnapshot {
    scan: ScanResult,
    canonical_ids: HashMap<String, String>,
    published_at: Instant,
}

impl SkillCatalogSnapshot {
    fn new(scan: ScanResult, published_at: Instant) -> Self {
        let mut canonical_ids = HashMap::new();
        for skill in &scan.skills {
            canonical_ids.insert(skill.id.clone(), skill.id.clone());
            for legacy_id in &skill.legacy_ids {
                canonical_ids.insert(legacy_id.clone(), skill.id.clone());
            }
        }
        Self {
            scan,
            canonical_ids,
            published_at,
        }
    }

    fn is_fresh_at(&self, now: Instant) -> bool {
        now.checked_duration_since(self.published_at)
            .map(|age| age <= SKILL_CATALOG_TTL)
            .unwrap_or(false)
    }
}

/// Short-lived, non-persistent catalog shared by browser, update, preference,
/// and harness lookups. A single mutex also coalesces a cache miss into one
/// configured scan instead of allowing callers to stampede the filesystem.
#[derive(Default)]
pub struct SkillCatalogState(Mutex<Option<SkillCatalogSnapshot>>);

impl SkillCatalogState {
    pub fn invalidate(&self) {
        match self.0.lock() {
            Ok(mut snapshot) => *snapshot = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CatalogLookup {
    pub scan: ScanResult,
    canonical_ids: HashMap<String, String>,
}

impl CatalogLookup {
    pub(crate) fn canonical_id(&self, requested_id: &str) -> String {
        self.canonical_ids
            .get(requested_id)
            .cloned()
            .unwrap_or_else(|| requested_id.to_string())
    }

    pub(crate) fn skill(&self, requested_id: &str) -> Option<&Skill> {
        let canonical_id = self.canonical_ids.get(requested_id)?;
        self.scan
            .skills
            .iter()
            .find(|skill| skill.id == *canonical_id)
    }
}

/// Return a fresh configured scan. Callers receive the same canonical and
/// legacy ID mapping whether the snapshot was reused or newly published.
pub(crate) fn scan_for_lookup(
    state: &State<'_, ConfigState>,
    catalog: &State<'_, SkillCatalogState>,
) -> Result<CatalogLookup, String> {
    configured_scan(state, catalog, false)
}

/// Mutations must resolve against the filesystem at the time of the request,
/// rather than against the short-lived UI catalog. The subsequent file binding
/// still protects the interval between this scan and the actual write.
pub(crate) fn scan_for_mutation(
    state: &State<'_, ConfigState>,
    catalog: &State<'_, SkillCatalogState>,
) -> Result<CatalogLookup, String> {
    configured_scan(state, catalog, true)
}

fn configured_scan(
    state: &State<'_, ConfigState>,
    catalog: &State<'_, SkillCatalogState>,
    force_scan: bool,
) -> Result<CatalogLookup, String> {
    let mut cached = catalog
        .0
        .lock()
        .map_err(|_| "Failed to lock skill catalog".to_string())?;
    let now = Instant::now();
    if !force_scan {
        if let Some(snapshot) = cached.as_ref().filter(|snapshot| snapshot.is_fresh_at(now)) {
            return Ok(CatalogLookup {
                scan: snapshot.scan.clone(),
                canonical_ids: snapshot.canonical_ids.clone(),
            });
        }
    }

    // Scanning can touch a large number of files. Take a configuration
    // snapshot for that work, but do not hold ConfigState while traversing the
    // filesystem. Saving that snapshot afterwards would overwrite unrelated
    // preferences that were changed during the scan, so merge only the
    // scan-derived fields back into the current config under the mutex.
    let mut scan_config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?
        .clone();
    let (scan, config_changed) = scan_with_config(&mut scan_config);
    if config_changed {
        let mut config = state
            .0
            .lock()
            .map_err(|_| "Failed to lock config state".to_string())?;
        if merge_scan_config_changes(&mut config, &scan_config, &scan.skills) {
            // Keep the in-memory merge and durable write in the same critical
            // section so another preference command cannot persist an
            // interleaved configuration snapshot.
            let _ = crate::commands::preferences::save_config(&config);
        }
    }

    let snapshot = SkillCatalogSnapshot::new(scan, now);
    let lookup = CatalogLookup {
        scan: snapshot.scan.clone(),
        canonical_ids: snapshot.canonical_ids.clone(),
    };
    *cached = Some(snapshot);
    Ok(lookup)
}

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
        skill
            .metadata
            .slash_command
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase()
    )
}

fn file_timestamp(path: &str, current_time: u64) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;

    let mut best: Option<u64> = None;
    for stamp in [
        metadata.created().ok(),
        metadata.modified().ok(),
        metadata.accessed().ok(),
    ] {
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

/// Best-effort last-modified timestamp from filesystem metadata. Falls back to
/// `created()` if `modified()` is unavailable. Used to drive the
/// "updated X ago" pill in the UI.
fn file_last_modified(path: &str, current_time: u64) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;

    for stamp in [metadata.modified().ok(), metadata.created().ok()] {
        let Some(stamp) = stamp else {
            continue;
        };
        let Some(secs) = stamp.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()) else {
            continue;
        };
        if secs == 0 || secs > current_time + 3600 {
            continue;
        }
        return Some(secs);
    }

    None
}

fn infer_install_timestamp(skill: &Skill, current_time: u64) -> u64 {
    file_timestamp(&skill.file_path, current_time).unwrap_or(current_time)
}

/// Shared scan pipeline used by commands that need the exact same skill IDs as the UI.
pub(crate) fn scan_with_config(config: &mut AppConfig) -> (ScanResult, bool) {
    // Capture mode is an all-or-nothing synthetic fixture contract. Never add
    // remembered project/custom paths after the bounded scanner has run: those
    // preferences may name the real user profile.
    let capture_requested = capture_mode_requested();
    let mut result = if capture_requested {
        scan_all_skills_with_roots(&[])
    } else {
        scan_all_skills_with_roots(&config.project_roots)
    };
    let mut config_changed = false;

    if !capture_requested {
        let (custom_skills, custom_errors) = scan_custom_paths(&config.custom_scan_paths);
        result.skills.extend(custom_skills);
        result.errors.extend(custom_errors);
    }
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

        if let Some(cached) = config.update_check_cache.get(&skill.id).or_else(|| {
            skill
                .legacy_ids
                .iter()
                .find_map(|legacy| config.update_check_cache.get(legacy))
        }) {
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
            config.skill_install_index.insert(
                install_key.clone(),
                SkillInstallEntry {
                    installed_at: inferred,
                },
            );
            config_changed = true;
            inferred
        };

        skill.installed_at = Some(installed_at);
        skill.last_modified_at = file_last_modified(&skill.file_path, current_time);

        // Surface archive count from history so the UI can show an indicator
        // without an extra round-trip. Legacy ids are merged into the canonical
        // id during `remap_config_ids_for_skills`, so a single lookup is enough.
        skill.archive_count = config
            .skill_version_history
            .get(&skill.id)
            .map(|entries| entries.len() as u32)
            .unwrap_or(0);
    }

    enrich_skill_discovery(&mut result.skills);

    (result, config_changed)
}

/// Apply only the fields a configured scan is allowed to change. The scan
/// runs against a clone outside ConfigState; replaying these deterministic
/// migrations against the current configuration prevents the stale clone from
/// replacing unrelated settings saved while the filesystem walk was in flight.
fn merge_scan_config_changes(
    current: &mut AppConfig,
    scanned: &AppConfig,
    skills: &[Skill],
) -> bool {
    let mut changed = remap_config_ids_for_skills(
        skills,
        ConfigIdCollections {
            starred_skills: &mut current.starred_skills,
            skill_icons: &mut current.skill_icons,
            skill_repo_overrides: &mut current.skill_repo_overrides,
            skill_install_overrides: &mut current.skill_install_overrides,
            update_check_cache: &mut current.update_check_cache,
            skill_version_history: &mut current.skill_version_history,
            collapsed_tree_nodes: &mut current.collapsed_tree_nodes,
        },
    );

    // A scan only appends first-seen timestamps. Preserve an entry added by a
    // concurrent preference operation rather than replacing it with the value
    // inferred by the earlier clone.
    for (fingerprint, entry) in &scanned.skill_install_index {
        if !current.skill_install_index.contains_key(fingerprint) {
            current
                .skill_install_index
                .insert(fingerprint.clone(), entry.clone());
            changed = true;
        }
    }

    changed
}

/// Scan all installed agents for skills.
/// Called on app startup and when the user refreshes.
///
#[tauri::command]
pub fn scan_skills(
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
) -> ScanResult {
    match configured_scan(&state, &catalog, true) {
        Ok(lookup) => lookup.scan,
        Err(error) => ScanResult {
            skills: Vec::new(),
            errors: vec![crate::models::ScanError {
                file_path: String::new(),
                message: error,
            }],
            scan_duration_ms: 0,
        },
    }
}

/// List all known agents and whether they're installed.
/// Used to populate the agent filter tabs in the UI.
#[tauri::command]
pub fn list_agents() -> Vec<AgentInfo> {
    detect_installed_agents()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AllowedSkillPath {
    Directory(PathBuf),
    File(PathBuf),
}

fn canonical_allowed_path(path: &Path, allow_file: bool) -> Option<AllowedSkillPath> {
    let canonical = std::fs::canonicalize(path).ok()?;
    let metadata = std::fs::metadata(&canonical).ok()?;

    if metadata.is_dir() {
        Some(AllowedSkillPath::Directory(canonical))
    } else if allow_file && metadata.is_file() {
        Some(AllowedSkillPath::File(canonical))
    } else {
        None
    }
}

fn skill_read_allowlist(
    home_dir: Option<&Path>,
    project_roots: &[String],
    custom_scan_paths: &[String],
) -> Vec<AllowedSkillPath> {
    let mut allowed = Vec::new();

    if let Some(home) = home_dir {
        if let Some(path) = canonical_allowed_path(home, false) {
            allowed.push(path);
        }
    }

    for raw in project_roots {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(path) = canonical_allowed_path(Path::new(trimmed), false) {
            if !allowed.contains(&path) {
                allowed.push(path);
            }
        }
    }

    for raw in custom_scan_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.contains("..") {
            continue;
        }
        if let Some(path) = canonical_allowed_path(Path::new(trimmed), true) {
            if !allowed.contains(&path) {
                allowed.push(path);
            }
        }
    }

    allowed
}

fn is_allowed_skill_path(path: &Path, allowed: &[AllowedSkillPath]) -> bool {
    allowed.iter().any(|entry| match entry {
        AllowedSkillPath::Directory(directory) => path.starts_with(directory),
        AllowedSkillPath::File(file) => path == file,
    })
}

#[cfg(unix)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_file_identity(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    left.file_attributes() == right.file_attributes()
        && left.creation_time() == right.creation_time()
        && left.last_write_time() == right.last_write_time()
        && left.file_size() == right.file_size()
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_left: &std::fs::Metadata, _right: &std::fs::Metadata) -> bool {
    false
}

/// A read-time binding for a skill file that must still hold at write time.
/// It combines the resolved path, platform file identity, and full content
/// digest so a same-path delete/replace cannot be restored over accidentally.
#[derive(Debug, Clone)]
pub(crate) struct SkillFileBinding {
    canonical_path: PathBuf,
    metadata: std::fs::Metadata,
    content_fingerprint: String,
}

fn content_fingerprint(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub(crate) fn read_skill_file_in_scope(
    skill_path: &Path,
    home_dir: Option<&Path>,
    project_roots: &[String],
    custom_scan_paths: &[String],
) -> Result<String, String> {
    read_skill_file_with_binding_in_scope(skill_path, home_dir, project_roots, custom_scan_paths)
        .map(|(content, _)| content)
}

/// Read a skill from an allowed root and retain a stable identity/content
/// binding that can later authorize a destructive write to this exact file.
pub(crate) fn read_skill_file_with_binding_in_scope(
    skill_path: &Path,
    home_dir: Option<&Path>,
    project_roots: &[String],
    custom_scan_paths: &[String],
) -> Result<(String, SkillFileBinding), String> {
    let allowed = skill_read_allowlist(home_dir, project_roots, custom_scan_paths);
    if allowed.is_empty() {
        return Err("Failed to resolve allowed skill roots".to_string());
    }

    let canonical_skill = std::fs::canonicalize(skill_path)
        .map_err(|_| "Failed to resolve skill path".to_string())?;
    let canonical_metadata = std::fs::metadata(&canonical_skill)
        .map_err(|_| "Failed to inspect skill file".to_string())?;

    if !canonical_metadata.is_file() {
        return Err("Skill path is not a file".to_string());
    }

    // Canonicalize both sides before comparing. On Windows, canonicalize()
    // produces extended `\\?\` paths. Component-aware Path comparisons avoid
    // accepting prefix siblings, and resolving links prevents an entry under
    // an allowed directory from escaping to another location.
    if !is_allowed_skill_path(&canonical_skill, &allowed) {
        return Err("Skill path is outside allowed scope".to_string());
    }

    // Open the resolved target, then resolve and inspect it again before
    // reading. This narrows the path-swap window and ensures the open handle is
    // still associated with the same in-scope regular-file path.
    let mut file =
        File::open(&canonical_skill).map_err(|_| "Failed to open skill file".to_string())?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| "Failed to inspect skill file".to_string())?;
    if !opened_metadata.is_file() {
        return Err("Skill path is not a file".to_string());
    }
    if !same_file_identity(&canonical_metadata, &opened_metadata) {
        return Err("Skill path changed while opening".to_string());
    }

    let resolved_again = std::fs::canonicalize(skill_path)
        .map_err(|_| "Skill path changed while opening".to_string())?;
    let canonical_again = std::fs::canonicalize(&canonical_skill)
        .map_err(|_| "Skill path changed while opening".to_string())?;
    if resolved_again != canonical_skill
        || canonical_again != canonical_skill
        || !is_allowed_skill_path(&resolved_again, &allowed)
    {
        return Err("Skill path changed while opening".to_string());
    }

    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|_| "Failed to read skill file".to_string())?;
    let content_fingerprint = content_fingerprint(&content);
    Ok((
        content,
        SkillFileBinding {
            canonical_path: canonical_skill,
            metadata: opened_metadata,
            content_fingerprint,
        },
    ))
}

/// Write one discovered skill after the same canonical allowlist and file
/// identity checks used for content reads. No caller may write based solely on
/// a catalog path, because the file can move between lookup and mutation.
pub(crate) fn write_skill_file_in_scope(
    skill_path: &Path,
    content: &str,
    expected_binding: &SkillFileBinding,
    home_dir: Option<&Path>,
    project_roots: &[String],
    custom_scan_paths: &[String],
) -> Result<(), String> {
    let allowed = skill_read_allowlist(home_dir, project_roots, custom_scan_paths);
    if allowed.is_empty() {
        return Err("Failed to resolve allowed skill roots".to_string());
    }

    let canonical_skill = std::fs::canonicalize(skill_path)
        .map_err(|_| "Failed to resolve skill path".to_string())?;
    let canonical_metadata = std::fs::metadata(&canonical_skill)
        .map_err(|_| "Failed to inspect skill file".to_string())?;
    if !canonical_metadata.is_file() {
        return Err("Skill path is not a file".to_string());
    }
    if !is_allowed_skill_path(&canonical_skill, &allowed) {
        return Err("Skill path is outside allowed scope".to_string());
    }
    if canonical_skill != expected_binding.canonical_path
        || !same_file_identity(&canonical_metadata, &expected_binding.metadata)
    {
        return Err("Skill path changed before restore".to_string());
    }

    let mut file = open_bound_skill_file(&canonical_skill)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| "Failed to inspect skill file".to_string())?;
    if !opened_metadata.is_file() {
        return Err("Skill path is not a file".to_string());
    }
    if !same_file_identity(&canonical_metadata, &opened_metadata) {
        return Err("Skill path changed while opening".to_string());
    }
    if !same_file_identity(&opened_metadata, &expected_binding.metadata) {
        return Err("Skill path changed before restore".to_string());
    }

    let resolved_again = std::fs::canonicalize(skill_path)
        .map_err(|_| "Skill path changed while opening".to_string())?;
    let canonical_again = std::fs::canonicalize(&canonical_skill)
        .map_err(|_| "Skill path changed while opening".to_string())?;
    if resolved_again != canonical_skill
        || canonical_again != canonical_skill
        || !is_allowed_skill_path(&resolved_again, &allowed)
    {
        return Err("Skill path changed while opening".to_string());
    }

    let mut current = String::new();
    file.read_to_string(&mut current)
        .map_err(|_| "Failed to read skill file".to_string())?;
    if content_fingerprint(&current) != expected_binding.content_fingerprint {
        return Err("Skill path changed before restore".to_string());
    }

    // Re-check the pathname after the content verification. If it was
    // replaced, writing the already-open original handle is harmless, but we
    // fail closed instead of claiming that the replacement was restored.
    let resolved_before_write = std::fs::canonicalize(skill_path)
        .map_err(|_| "Skill path changed before restore".to_string())?;
    if resolved_before_write != expected_binding.canonical_path
        || !same_file_identity(
            &file
                .metadata()
                .map_err(|_| "Failed to inspect skill file".to_string())?,
            &expected_binding.metadata,
        )
    {
        return Err("Skill path changed before restore".to_string());
    }

    // Write through the verified handle. On Windows it is opened with deny
    // write/delete sharing, so external editors cannot substitute this path
    // during the destructive interval. On Unix, a late rename detaches this
    // handle and the final path check fails without touching the replacement.
    #[cfg(test)]
    if consume_injected_atomic_replace_failure() {
        return Err("Injected atomic skill replacement failure".to_string());
    }
    #[cfg(test)]
    {
        run_late_path_swap_hook()?;
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("Restore write may have modified the bound file: {e}"))?;
    file.set_len(0)
        .map_err(|e| format!("Restore write may have modified the bound file: {e}"))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Restore write may have modified the bound file: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Restore write may have modified the bound file: {e}"))?;

    let resolved_after = std::fs::canonicalize(skill_path).map_err(|_| {
        "Restore write may have modified the bound file: skill path changed during restore"
            .to_string()
    })?;
    let live_metadata = std::fs::metadata(&resolved_after).map_err(|_| {
        "Restore write may have modified the bound file: skill path changed during restore"
            .to_string()
    })?;
    let bound_metadata = file
        .metadata()
        .map_err(|_| "Restore write may have modified the bound file".to_string())?;
    if resolved_after != canonical_skill || !same_file_identity(&live_metadata, &bound_metadata) {
        return Err(
            "Restore write may have modified the bound file: skill path changed during restore"
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn open_bound_skill_file(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .map_err(|_| "Failed to open skill file".to_string())
}

#[cfg(not(windows))]
fn open_bound_skill_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| "Failed to open skill file".to_string())
}

#[cfg(test)]
std::thread_local! {
    // This test seam must be thread-local. Rust executes unit tests in
    // parallel, and a process-wide one-shot flag can otherwise be consumed by
    // an unrelated restore test before its owning assertion executes.
    static FAIL_NEXT_SKILL_ATOMIC_REPLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn inject_next_atomic_replace_failure() {
    FAIL_NEXT_SKILL_ATOMIC_REPLACE.with(|flag| flag.set(true));
}

#[cfg(test)]
fn consume_injected_atomic_replace_failure() -> bool {
    FAIL_NEXT_SKILL_ATOMIC_REPLACE.with(|flag| flag.replace(false))
}

#[cfg(test)]
static PATH_SWAP_BEFORE_FINAL_BINDING: std::sync::Mutex<Option<(PathBuf, String)>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
fn run_late_path_swap_hook() -> Result<(), String> {
    let Some((replacement_path, replacement_content)) = PATH_SWAP_BEFORE_FINAL_BINDING
        .lock()
        .map_err(|_| "Failed to lock test pathname-swap hook".to_string())?
        .take()
    else {
        return Ok(());
    };

    // Windows rejects the remove while our bound handle is open. Unix may
    // permit it, in which case the live path remains the external replacement
    // and the post-write canonical-path check reports failure.
    if std::fs::remove_file(&replacement_path).is_ok() {
        std::fs::write(&replacement_path, replacement_content)
            .map_err(|_| "Injected pathname-swap replacement failed".to_string())?;
    }
    Ok(())
}

/// Read the raw content of a skill file.
/// Used when the user clicks a card to see the full skill body.
#[tauri::command]
pub fn read_skill_content(
    state: State<'_, ConfigState>,
    catalog: State<'_, SkillCatalogState>,
    skill_id: String,
) -> Result<String, String> {
    let lookup = scan_for_lookup(&state, &catalog)?;
    let skill = lookup
        .skill(&skill_id)
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;

    let config_snapshot = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?
        .clone();

    let (home_dir, project_roots, custom_scan_paths) = match capture_root_mode_from_process() {
        CaptureRootMode::Normal => (
            dirs::home_dir(),
            config_snapshot.project_roots.as_slice(),
            config_snapshot.custom_scan_paths.as_slice(),
        ),
        CaptureRootMode::Capture(root) => (
            Some(root.root().to_path_buf()),
            &[] as &[String],
            &[] as &[String],
        ),
        CaptureRootMode::Invalid(_) => {
            return Err("Capture fixture contract is invalid".to_string());
        }
    };
    read_skill_file_in_scope(
        Path::new(&skill.file_path),
        home_dir.as_deref(),
        project_roots,
        custom_scan_paths,
    )
}

#[cfg(test)]
mod read_skill_content_tests {
    use super::{
        read_skill_file_in_scope, read_skill_file_with_binding_in_scope, write_skill_file_in_scope,
    };
    use std::fs;
    use std::path::Path;

    fn read_with(
        skill: &Path,
        home: Option<&Path>,
        project_roots: &[String],
        custom_scan_paths: &[String],
    ) -> Result<String, String> {
        read_skill_file_in_scope(skill, home, project_roots, custom_scan_paths)
    }

    #[test]
    fn reads_valid_skill_inside_home() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("SKILL.md");
        fs::write(&skill, "# Valid skill\n").expect("skill fixture");

        let content = read_with(&skill, Some(&home), &[], &[]).expect("in-home read succeeds");

        assert_eq!(content, "# Valid skill\n");
    }

    #[test]
    fn denies_skill_outside_home() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let outside = temp.path().join("outside.md");
        fs::write(&outside, "outside").expect("outside fixture");

        let error = read_with(&outside, Some(&home), &[], &[]).expect_err("outside read is denied");

        assert_eq!(error, "Skill path is outside allowed scope");
    }

    #[test]
    fn allows_project_root_on_another_scope() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        let project = temp.path().join("external-project");
        fs::create_dir(&home).expect("home directory");
        fs::create_dir(&project).expect("project directory");
        let skill = project.join("AGENTS.md");
        fs::write(&skill, "project skill").expect("project fixture");
        let roots = vec![project.to_string_lossy().to_string()];

        let content =
            read_with(&skill, Some(&home), &roots, &[]).expect("project-root read succeeds");

        assert_eq!(content, "project skill");
    }

    #[test]
    fn allows_custom_directory_and_exact_file_outside_home() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        let custom_dir = temp.path().join("custom-directory");
        let exact_file = temp.path().join("exact-custom.md");
        fs::create_dir(&home).expect("home directory");
        fs::create_dir(&custom_dir).expect("custom directory");
        let nested_file = custom_dir.join("nested.md");
        fs::write(&nested_file, "nested custom").expect("nested fixture");
        fs::write(&exact_file, "exact custom").expect("exact fixture");

        let directory_paths = vec![custom_dir.to_string_lossy().to_string()];
        let exact_paths = vec![exact_file.to_string_lossy().to_string()];

        assert_eq!(
            read_with(&nested_file, Some(&home), &[], &directory_paths)
                .expect("custom-directory read succeeds"),
            "nested custom"
        );
        assert_eq!(
            read_with(&exact_file, Some(&home), &[], &exact_paths)
                .expect("exact custom-file read succeeds"),
            "exact custom"
        );
    }

    #[test]
    fn exact_custom_file_does_not_allow_a_sibling() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let configured = temp.path().join("configured.md");
        let sibling = temp.path().join("sibling.md");
        fs::write(&configured, "configured").expect("configured fixture");
        fs::write(&sibling, "sibling").expect("sibling fixture");
        let custom = vec![configured.to_string_lossy().to_string()];

        let error = read_with(&sibling, Some(&home), &[], &custom)
            .expect_err("exact file does not authorize its sibling");

        assert_eq!(error, "Skill path is outside allowed scope");
    }

    #[test]
    fn directory_prefix_sibling_is_denied() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        let allowed = temp.path().join("project");
        let prefix_sibling = temp.path().join("project-secrets");
        fs::create_dir(&home).expect("home directory");
        fs::create_dir(&allowed).expect("allowed directory");
        fs::create_dir(&prefix_sibling).expect("prefix sibling directory");
        let skill = prefix_sibling.join("SKILL.md");
        fs::write(&skill, "outside").expect("outside fixture");
        let roots = vec![allowed.to_string_lossy().to_string()];

        let error = read_with(&skill, Some(&home), &roots, &[])
            .expect_err("component-prefix sibling is denied");

        assert_eq!(error, "Skill path is outside allowed scope");
    }

    #[test]
    fn returns_empty_content_for_empty_skill() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("EMPTY.md");
        fs::write(&skill, "").expect("empty skill fixture");

        let content = read_with(&skill, Some(&home), &[], &[]).expect("empty file is readable");

        assert!(content.is_empty());
    }

    #[test]
    fn rejects_missing_skill_directory_target_and_unresolved_allowlist() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let missing = home.join("missing.md");

        let missing_error =
            read_with(&missing, Some(&home), &[], &[]).expect_err("missing file is rejected");
        let directory_error =
            read_with(&home, Some(&home), &[], &[]).expect_err("directory target is rejected");
        let allowlist_error =
            read_with(&missing, None, &[], &[]).expect_err("empty allowlist is rejected");

        assert_eq!(missing_error, "Failed to resolve skill path");
        assert_eq!(directory_error, "Skill path is not a file");
        assert_eq!(allowlist_error, "Failed to resolve allowed skill roots");
    }

    #[test]
    fn configured_external_root_works_without_resolved_home() {
        let temp = tempfile::tempdir().expect("temp directory");
        let project = temp.path().join("external-project");
        fs::create_dir(&project).expect("project directory");
        let skill = project.join("SKILL.md");
        fs::write(&skill, "external").expect("external fixture");
        let roots = vec![project.to_string_lossy().to_string()];

        let content =
            read_with(&skill, None, &roots, &[]).expect("configured root is independently valid");

        assert_eq!(content, "external");
    }

    #[test]
    fn symlink_inside_allowed_directory_cannot_escape() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let outside = temp.path().join("outside.md");
        let link = home.join("linked.md");
        fs::write(&outside, "outside").expect("outside fixture");

        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&outside, &link);

        if link_result.is_err() {
            return;
        }

        let error = read_with(&link, Some(&home), &[], &[]).expect_err("symlink escape is denied");

        assert_eq!(error, "Skill path is outside allowed scope");
    }

    #[test]
    fn explicitly_configured_symlink_file_allows_only_its_canonical_target() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let outside = temp.path().join("outside.md");
        let sibling = temp.path().join("sibling.md");
        let link = temp.path().join("configured-link.md");
        fs::write(&outside, "outside").expect("outside fixture");
        fs::write(&sibling, "sibling").expect("sibling fixture");

        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&outside, &link);

        if link_result.is_err() {
            return;
        }

        let custom = vec![link.to_string_lossy().to_string()];
        assert_eq!(
            read_with(&link, Some(&home), &[], &custom).expect("explicit symlink file is allowed"),
            "outside"
        );
        assert_eq!(
            read_with(&sibling, Some(&home), &[], &custom)
                .expect_err("configured symlink does not authorize siblings"),
            "Skill path is outside allowed scope"
        );
    }

    #[test]
    fn bound_write_rejects_same_path_delete_and_replace() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("SKILL.md");
        fs::write(&skill, "original skill").expect("original fixture");

        let (_, binding) = read_skill_file_with_binding_in_scope(&skill, Some(&home), &[], &[])
            .expect("read and bind original");

        fs::remove_file(&skill).expect("delete original");
        fs::write(&skill, "replacement skill").expect("replacement fixture");

        let error =
            write_skill_file_in_scope(&skill, "restored archive", &binding, Some(&home), &[], &[])
                .expect_err("replacement must not be restored over");

        assert_eq!(error, "Skill path changed before restore");
        assert_eq!(
            fs::read_to_string(&skill).expect("replacement remains readable"),
            "replacement skill"
        );
    }

    #[test]
    fn bound_write_restores_unchanged_skill() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("SKILL.md");
        fs::write(&skill, "current skill").expect("current fixture");

        let (_, binding) = read_skill_file_with_binding_in_scope(&skill, Some(&home), &[], &[])
            .expect("read and bind current skill");

        write_skill_file_in_scope(&skill, "restored archive", &binding, Some(&home), &[], &[])
            .expect("unchanged file accepts a restore");

        assert_eq!(
            fs::read_to_string(&skill).expect("restored skill remains readable"),
            "restored archive"
        );
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::{
        inject_next_atomic_replace_failure, read_skill_file_with_binding_in_scope,
        write_skill_file_in_scope, CatalogLookup, SkillCatalogSnapshot, SkillCatalogState,
        PATH_SWAP_BEFORE_FINAL_BINDING, SKILL_CATALOG_TTL,
    };
    use crate::commands::preferences::ConfigState;
    use crate::models::{
        AgentId, AppConfig, ArtifactType, ScanResult, Skill, SkillInstallEntry, SkillMetadata,
        SkillScope,
    };
    use std::fs;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn fixture_skill() -> Skill {
        Skill {
            id: "canonical-skill".to_string(),
            name: "Catalog fixture".to_string(),
            description: String::new(),
            artifact_type: ArtifactType::Skill,
            agent_id: AgentId::Codex,
            source_agents: Vec::new(),
            file_path: "C:/fixture/SKILL.md".to_string(),
            source_paths: Vec::new(),
            legacy_ids: vec!["legacy-skill".to_string()],
            scope: SkillScope::Global,
            project_path: None,
            metadata: SkillMetadata::default(),
            discovery_tags: Vec::new(),
            use_cases: Vec::new(),
            discovery_hints: Vec::new(),
            icon: None,
            starred: false,
            update_available: false,
            installed_at: None,
            last_modified_at: None,
            archive_count: 0,
            parent_id: None,
            children: Vec::new(),
        }
    }

    fn fixture_scan() -> ScanResult {
        ScanResult {
            skills: vec![fixture_skill()],
            errors: Vec::new(),
            scan_duration_ms: 0,
        }
    }

    #[test]
    fn snapshot_resolves_canonical_and_legacy_ids() {
        let snapshot = SkillCatalogSnapshot::new(fixture_scan(), Instant::now());
        let lookup = CatalogLookup {
            scan: snapshot.scan.clone(),
            canonical_ids: snapshot.canonical_ids.clone(),
        };

        assert_eq!(lookup.canonical_id("canonical-skill"), "canonical-skill");
        assert_eq!(lookup.canonical_id("legacy-skill"), "canonical-skill");
        assert_eq!(
            lookup.skill("legacy-skill").map(|skill| skill.id.as_str()),
            Some("canonical-skill")
        );
    }

    #[test]
    fn snapshot_expiry_and_invalidation_turn_a_hit_into_a_miss() {
        let published_at = Instant::now();
        let snapshot = SkillCatalogSnapshot::new(fixture_scan(), published_at);
        assert!(snapshot.is_fresh_at(published_at + SKILL_CATALOG_TTL));
        assert!(!snapshot.is_fresh_at(published_at + SKILL_CATALOG_TTL + Duration::from_millis(1)));

        let catalog = SkillCatalogState::default();
        *catalog.0.lock().expect("catalog lock") = Some(snapshot);
        catalog.invalidate();
        assert!(catalog.0.lock().expect("catalog lock").is_none());
    }

    #[test]
    fn configured_scan_includes_project_and_custom_sources() {
        let temp = tempfile::tempdir().expect("temp directory");
        let project = temp.path().join("project");
        let custom = temp.path().join("custom.md");
        let project_skill_dir = project.join(".codex").join("skills").join("fixture");
        std::fs::create_dir_all(&project_skill_dir).expect("project directory");
        let project_artifact = project_skill_dir.join("SKILL.md");
        std::fs::write(&project_artifact, "# Project artifact\n").expect("project artifact");
        std::fs::write(&custom, "# Custom artifact\n").expect("custom artifact");

        let mut config = AppConfig {
            project_roots: vec![project.to_string_lossy().to_string()],
            custom_scan_paths: vec![custom.to_string_lossy().to_string()],
            ..AppConfig::default()
        };
        let (scan, _) = super::scan_with_config(&mut config);
        let paths: Vec<String> = scan
            .skills
            .into_iter()
            .map(|skill| skill.file_path)
            .collect();

        let project_suffix = "/project/.codex/skills/fixture/skill.md";
        let custom_suffix = "/custom.md";
        assert!(
            paths.iter().any(|path| path
                .replace('\\', "/")
                .to_lowercase()
                .ends_with(project_suffix)),
            "project artifact missing from {paths:?}"
        );
        assert!(paths.iter().any(|path| path
            .replace('\\', "/")
            .to_lowercase()
            .ends_with(custom_suffix)));
    }

    #[test]
    fn scan_merge_preserves_a_concurrent_non_catalog_preference_save() {
        let mut initial = AppConfig::default();
        initial.starred_skills.insert("legacy-skill".to_string());
        let state = ConfigState(Mutex::new(initial));

        // This is the detached snapshot used by the filesystem scan. While it
        // is scanning, a normal preference command saves a theme change on the
        // live ConfigState. A stale whole-config write here used to revert it.
        let mut scanned = state.0.lock().expect("scan snapshot lock").clone();
        scanned.skill_install_index.insert(
            "scan-derived-fingerprint".to_string(),
            SkillInstallEntry { installed_at: 42 },
        );
        state.0.lock().expect("theme save lock").theme = "light".to_string();

        let mut current = state.0.lock().expect("merge lock");
        assert!(super::merge_scan_config_changes(
            &mut current,
            &scanned,
            &fixture_scan().skills,
        ));
        assert_eq!(current.theme, "light");
        assert!(current.starred_skills.contains("canonical-skill"));
        assert!(!current.starred_skills.contains("legacy-skill"));
        assert_eq!(
            current
                .skill_install_index
                .get("scan-derived-fingerprint")
                .map(|entry| entry.installed_at),
            Some(42)
        );
    }

    #[test]
    fn injected_atomic_restore_failure_preserves_original_skill_bytes() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("SKILL.md");
        fs::write(&skill, "original skill bytes").expect("skill fixture");
        let (_, binding) = read_skill_file_with_binding_in_scope(&skill, Some(&home), &[], &[])
            .expect("binding should be created");

        inject_next_atomic_replace_failure();
        let result = write_skill_file_in_scope(
            &skill,
            "restored archive bytes",
            &binding,
            Some(&home),
            &[],
            &[],
        );

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(&skill).expect("original file stays readable"),
            "original skill bytes"
        );
    }

    #[test]
    fn bound_handle_never_overwrites_a_late_pathname_replacement() {
        let temp = tempfile::tempdir().expect("temp directory");
        let home = temp.path().join("home");
        fs::create_dir(&home).expect("home directory");
        let skill = home.join("SKILL.md");
        fs::write(&skill, "bound original bytes").expect("skill fixture");
        let (_, binding) = read_skill_file_with_binding_in_scope(&skill, Some(&home), &[], &[])
            .expect("binding should be created");

        *PATH_SWAP_BEFORE_FINAL_BINDING
            .lock()
            .expect("pathname-swap hook lock") =
            Some((skill.clone(), "external replacement bytes".to_string()));
        let result = write_skill_file_in_scope(
            &skill,
            "restored archive bytes",
            &binding,
            Some(&home),
            &[],
            &[],
        );

        #[cfg(windows)]
        {
            // The exclusive bound handle denies the external remove/replace.
            let content = fs::read_to_string(&skill).expect("bound file remains readable");
            assert_ne!(
                content, "external replacement bytes",
                "an external replacement must not become the restore target"
            );
            if result.is_ok() {
                assert_eq!(content, "restored archive bytes");
            } else {
                assert_eq!(content, "bound original bytes");
            }
        }

        #[cfg(not(windows))]
        {
            assert_eq!(
                result.expect_err("late path swap must fail closed"),
                "Skill path changed during restore"
            );
            assert_eq!(
                fs::read_to_string(&skill).expect("replacement remains readable"),
                "external replacement bytes"
            );
        }
    }
}
