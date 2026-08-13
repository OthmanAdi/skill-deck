// @agent-context: Filesystem scanner that discovers skills across all installed agents.
//
// FLOW:
// 1. Load the agent registry (registry.rs)
// 2. Resolve $HOME and $PROJECT placeholders in each agent's paths
// 3. Glob each resolved path to find matching files
// 4. Parse each file using the agent's designated format parser
// 5. Return a unified Vec<Skill> for the frontend
//
// PERFORMANCE: Scanning is async and parallelized per-agent.
// Typical scan of ~100 skills across 5 agents completes in <50ms.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use walkdir::WalkDir;

use super::locations::{
    capture_root_mode_from_process, CaptureRoot, CaptureRootMode, LocationEnvironment,
};
use super::registry::{get_agent_registry, get_agent_registry_with_environment};
use crate::models::{
    AgentId, AgentInfo, ArtifactType, ScanError, ScanResult, Skill, SkillFormat, SkillScope,
};
use crate::parsers::frontmatter::{yaml_bool, yaml_str, yaml_string_array, yaml_string_list};
use crate::parsers::{
    claude_hooks::parse_claude_hooks, parse_frontmatter, skill_md::parse_skill_md,
};

const CAPTURE_FIXTURE_SKILL_COUNT: usize = 4;

/// Scan all known agent global directories for skills.
///
/// Equivalent to [`scan_all_skills_with_roots`] with no project roots. Kept as
/// the zero-arg entry point for callers and tests that only care about
/// user-level (global) skills.
pub fn scan_all_skills() -> ScanResult {
    scan_all_skills_with_roots(&[])
}

/// Scan global agent directories plus the project-scoped paths of every
/// `project_root` supplied by the caller.
///
/// PERF (SCAN-02): the registry lists the same shared store
/// (`$HOME/.agents/skills/**/SKILL.md`, `$HOME/.config/agents/skills/**/SKILL.md`)
/// under ~14 agents. Scanning per-agent would glob + parse that whole tree once
/// per agent. Instead we collapse identical `(resolved_pattern, format)` pairs
/// across agents, scan each unique pattern exactly once, and attribute the
/// result to every agent that declared it — `dedupe_skills_by_source` then
/// merges the lightweight per-agent records. I/O is paid once per directory.
///
/// PROJECT (SCAN-01): without a `project_root` the entire `project_paths`
/// registry is dead and three agents whose only paths are project-scoped
/// (AGENTS.md/Universal, Amazon Q, JetBrains AI) can never surface a skill.
/// Given a root we resolve `$PROJECT` the same way `$HOME` is resolved and scan
/// those paths with `SkillScope::Project`.
pub fn scan_all_skills_with_roots(project_roots: &[String]) -> ScanResult {
    match capture_root_mode_from_process() {
        CaptureRootMode::Normal => scan_all_skills_with_context(
            get_agent_registry(),
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            project_roots,
            None,
        ),
        CaptureRootMode::Capture(capture_root) => {
            let environment = LocationEnvironment::for_capture(&capture_root);
            scan_all_skills_with_context(
                get_agent_registry_with_environment(&environment),
                capture_root.root().to_path_buf(),
                &[],
                Some(&capture_root),
            )
        }
        CaptureRootMode::Invalid(message) => ScanResult {
            skills: Vec::new(),
            errors: vec![ScanError {
                file_path: "SKILL_DECK_CAPTURE_ROOT".to_string(),
                message,
            }],
            scan_duration_ms: 0,
        },
    }
}

fn scan_all_skills_with_context(
    registry: Vec<AgentInfo>,
    home: PathBuf,
    project_roots: &[String],
    capture_root: Option<&CaptureRoot>,
) -> ScanResult {
    let start = Instant::now();
    let home_str = home.to_string_lossy().to_string();

    let mut all_skills: Vec<Skill> = Vec::new();
    let mut all_errors: Vec<ScanError> = Vec::new();

    // --- GLOBAL scan (deduped across agents) ---
    let mut order: Vec<(String, SkillFormat)> = Vec::new();
    let mut groups: HashMap<(String, SkillFormat), Vec<AgentId>> = HashMap::new();

    for agent in &registry {
        if !should_scan_agent(agent, &home) {
            continue;
        }

        add_pattern_group(
            &agent.global_paths,
            &agent.id,
            &agent.format,
            |p| p.replace("$HOME", &home_str),
            &mut order,
            &mut groups,
        );

        // Claude Code plugins live under `~/.claude/plugins/cache/...`, which
        // is not reachable from a static glob: each plugin install pins its
        // own version directory and the user can enable/disable plugins
        // independently of what is on disk. Delegate to the plugin scanner so
        // we read the canonical registry instead of guessing.
        if capture_root.is_none() && matches!(agent.id, AgentId::ClaudeCode) {
            let (plugin_skills, plugin_errors) =
                crate::agents::claude_plugins::scan_claude_plugins(&home, agent);
            all_skills.extend(plugin_skills);
            all_errors.extend(plugin_errors);
        }
    }

    scan_pattern_groups(
        &order,
        &groups,
        SkillScope::Global,
        None,
        &mut all_skills,
        &mut all_errors,
        capture_root,
    );

    // --- PROJECT scan (per resolved root, deduped across agents) ---
    for root in resolve_project_roots(project_roots) {
        let mut proj_order: Vec<(String, SkillFormat)> = Vec::new();
        let mut proj_groups: HashMap<(String, SkillFormat), Vec<AgentId>> = HashMap::new();

        for agent in &registry {
            add_pattern_group(
                &agent.project_paths,
                &agent.id,
                &agent.format,
                |p| p.replace("$PROJECT", &root).replace("$HOME", &home_str),
                &mut proj_order,
                &mut proj_groups,
            );
        }

        scan_pattern_groups(
            &proj_order,
            &proj_groups,
            SkillScope::Project,
            Some(&root),
            &mut all_skills,
            &mut all_errors,
            capture_root,
        );
    }

    dedupe_skills_by_physical_source(&mut all_skills);

    // Screenshot fixtures are intentionally a four-card proof surface. A
    // malformed fixture should show no source-derived data, rather than a
    // partial capture that hides a real-path regression.
    if capture_root.is_some() && all_skills.len() != CAPTURE_FIXTURE_SKILL_COUNT {
        all_errors.push(ScanError {
            file_path: "SKILL_DECK_CAPTURE_ROOT".to_string(),
            message: format!(
                "capture fixture must resolve exactly {CAPTURE_FIXTURE_SKILL_COUNT} skills, found {}",
                all_skills.len()
            ),
        });
        all_skills.clear();
    }

    // Build parent/child hierarchy based on path relationships
    build_skill_tree(&mut all_skills);

    ScanResult {
        skills: all_skills,
        errors: all_errors,
        scan_duration_ms: start.elapsed().as_millis() as u64,
    }
}

/// Add an agent's declared patterns to the dedup map, recording first-seen
/// order so the scan stays deterministic across runs.
fn add_pattern_group(
    patterns: &[String],
    agent_id: &AgentId,
    format: &SkillFormat,
    resolve: impl Fn(&str) -> String,
    order: &mut Vec<(String, SkillFormat)>,
    groups: &mut HashMap<(String, SkillFormat), Vec<AgentId>>,
) {
    for pattern in patterns {
        let resolved = resolve(pattern);
        let key = (physical_pattern_key(&resolved), format.clone());
        let agents = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            Vec::new()
        });

        // A shared store is a physical source, not proof that every compatible
        // agent is installed or owns its contents. Universal wins even when an
        // agent-specific directory is a junction/symlink to that same store.
        if matches!(agent_id, AgentId::Universal) {
            agents.clear();
            agents.push(agent_id.clone());
        } else if !agents.contains(&AgentId::Universal) && !agents.contains(agent_id) {
            agents.push(agent_id.clone());
        }
    }
}

/// Scan each unique pattern once and replicate the parsed records across every
/// agent that declared the pattern.
fn scan_pattern_groups(
    order: &[(String, SkillFormat)],
    groups: &HashMap<(String, SkillFormat), Vec<AgentId>>,
    scope: SkillScope,
    project_path: Option<&str>,
    all_skills: &mut Vec<Skill>,
    all_errors: &mut Vec<ScanError>,
    capture_root: Option<&CaptureRoot>,
) {
    let mut seen_physical_files = HashSet::new();

    // Shared-store patterns run first. If an agent-specific tree contains a
    // child junction into `.agents/skills`, the neutral physical source is
    // parsed first and the later alias is skipped.
    for universal_first in [true, false] {
        for key in order {
            let Some(agent_ids) = groups.get(key) else {
                continue;
            };
            let is_universal = agent_ids.contains(&AgentId::Universal);
            if is_universal != universal_first {
                continue;
            }

            let (skills, errors) = scan_pattern_group(
                &key.0,
                &key.1,
                agent_ids,
                scope.clone(),
                project_path,
                &mut seen_physical_files,
                capture_root,
            );
            all_skills.extend(skills);
            all_errors.extend(errors);
        }
    }
}

/// Parse every file matched by one resolved pattern a single time, then emit a
/// per-agent copy so downstream dedup can merge them into a single card whose
/// `source_agents` lists all declaring agents.
fn scan_pattern_group(
    resolved: &str,
    format: &SkillFormat,
    agent_ids: &[AgentId],
    scope: SkillScope,
    project_path: Option<&str>,
    seen_physical_files: &mut HashSet<String>,
    capture_root: Option<&CaptureRoot>,
) -> (Vec<Skill>, Vec<ScanError>) {
    let primary_id = agent_ids.first().cloned().unwrap_or(AgentId::Universal);
    let representative = AgentInfo {
        id: primary_id,
        display_name: String::new(),
        description: String::new(),
        color: String::new(),
        installed: true,
        skill_count: 0,
        global_paths: Vec::new(),
        global_detection_paths: Vec::new(),
        project_paths: Vec::new(),
        format: format.clone(),
    };

    // Parse the matched files once as the representative agent, then re-stamp a
    // copy for every agent that declared this pattern.
    let (parsed, errors) = match scan_glob_pattern(
        resolved,
        &representative,
        scope,
        seen_physical_files,
        capture_root,
    ) {
        Ok(out) => out,
        Err(e) => (
            Vec::new(),
            vec![ScanError {
                file_path: resolved.to_string(),
                message: e.to_string(),
            }],
        ),
    };

    let mut skills = Vec::with_capacity(parsed.len() * agent_ids.len().max(1));
    for base in parsed {
        for agent_id in agent_ids {
            let mut skill = reassign_skill_agent(base.clone(), agent_id);
            skill.project_path = project_path.map(str::to_string);
            skills.push(skill);
        }
    }

    (skills, errors)
}

/// Convert textual glob aliases into one physical scan key. Existing roots
/// are canonicalized so a junction/symlink and its target are parsed once.
/// Case folding is Windows-only; case-sensitive macOS/Linux volumes retain
/// their canonical spelling.
fn physical_pattern_key(pattern: &str) -> String {
    physical_pattern_key_with(pattern, |path| std::fs::canonicalize(path).ok())
}

fn physical_pattern_key_with(
    pattern: &str,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
) -> String {
    let normalized = normalize_path_sep(pattern);
    let wildcard = normalized.find(['*', '?', '[']);

    let (root, suffix) = match wildcard {
        Some(index) => {
            let boundary = normalized[..index].rfind('/').unwrap_or(0);
            (&normalized[..boundary], &normalized[boundary..])
        }
        None => (normalized.as_str(), ""),
    };

    let canonical = canonicalize(Path::new(root)).unwrap_or_else(|| PathBuf::from(root));
    format!("{}{}", physical_path_identity(&canonical), suffix)
}

fn physical_path_identity(path: &Path) -> String {
    physical_path_identity_for_platform(path, cfg!(target_os = "windows"))
}

fn canonical_file_identity(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    physical_path_identity(&canonical)
}

fn physical_path_identity_for_platform(path: &Path, case_insensitive: bool) -> String {
    let normalized = normalize_path_sep(&path.to_string_lossy());
    let without_windows_prefix = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    if case_insensitive {
        without_windows_prefix.to_lowercase()
    } else {
        without_windows_prefix.to_string()
    }
}

/// Preserve case-sensitive filesystem identity before invoking the legacy
/// per-source merge, whose historical normalization lowercases every path.
/// Bucketing first keeps `Skill/` and `skill/` distinct on Linux and on
/// case-sensitive macOS volumes while retaining Windows case folding.
fn dedupe_skills_by_physical_source(skills: &mut Vec<Skill>) {
    dedupe_skills_by_physical_source_for_platform(skills, cfg!(target_os = "windows"));
}

fn dedupe_skills_by_physical_source_for_platform(skills: &mut Vec<Skill>, case_insensitive: bool) {
    let mut buckets: HashMap<String, Vec<Skill>> = HashMap::new();
    for skill in std::mem::take(skills) {
        let source = std::fs::canonicalize(&skill.file_path)
            .unwrap_or_else(|_| PathBuf::from(&skill.file_path));
        let identity = physical_path_identity_for_platform(&source, case_insensitive);
        buckets.entry(identity).or_default().push(skill);
    }

    let mut merged = Vec::new();
    for mut bucket in buckets.into_values() {
        crate::detection::skill_identity::dedupe_skills_by_source(&mut bucket);
        merged.extend(bucket);
    }
    merged.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(
                left.file_path
                    .to_lowercase()
                    .cmp(&right.file_path.to_lowercase()),
            )
    });
    *skills = merged;
}

/// Re-stamp a parsed skill for a specific agent. The raw `agent:stem` id is
/// preserved per agent so it lands in `legacy_ids` for config migration.
fn reassign_skill_agent(mut skill: Skill, agent_id: &AgentId) -> Skill {
    let stem = skill
        .id
        .split_once(':')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| skill.id.clone());
    let agent_str = serde_json::to_string(agent_id)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string();
    skill.id = format!("{}:{}", agent_str, stem);
    skill.agent_id = agent_id.clone();
    skill.source_agents = vec![agent_id.clone()];
    skill
}

/// Resolve the project roots to scan: configured roots that still exist, plus
/// the current working directory when it is a git repository (so launching the
/// overlay from inside a project surfaces that project's CLAUDE.md / AGENTS.md
/// / .cursorrules without any configuration).
fn resolve_project_roots(configured: &[String]) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();

    for raw in configured {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            push_existing_dir(Path::new(trimmed), &mut roots);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join(".git").exists() {
            push_existing_dir(&cwd, &mut roots);
        }
    }

    roots
}

fn push_existing_dir(path: &Path, roots: &mut Vec<String>) {
    if !path.is_dir() {
        return;
    }
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if !normalized.is_empty() && !roots.contains(&normalized) {
        roots.push(normalized);
    }
}

/// Scan additional user-provided markdown files/directories from config.custom_scan_paths.
/// Files are parsed using generic markdown parser and marked as custom skills.
pub fn scan_custom_paths(custom_scan_paths: &[String]) -> (Vec<Skill>, Vec<ScanError>) {
    let mut skills = Vec::new();
    let mut errors = Vec::new();

    for raw in custom_scan_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Reject obvious traversal patterns — custom paths should be absolute
        // or relative without parent-directory escapes.
        if trimmed.contains("..") {
            errors.push(ScanError {
                file_path: trimmed.to_string(),
                message: "custom scan path must not contain '..'".to_string(),
            });
            continue;
        }

        let path = Path::new(trimmed);
        if path.is_file() {
            match parse_custom_file(path) {
                Ok(skill) => skills.push(skill),
                Err(e) => errors.push(ScanError {
                    file_path: trimmed.to_string(),
                    message: e.to_string(),
                }),
            }
            continue;
        }

        if path.is_dir() {
            let pattern = format!("{}/**/*.md", path.to_string_lossy().replace('\\', "/"));
            match glob::glob(&pattern) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(file_path) => match parse_custom_file(&file_path) {
                                Ok(skill) => skills.push(skill),
                                Err(e) => errors.push(ScanError {
                                    file_path: file_path.to_string_lossy().to_string(),
                                    message: e.to_string(),
                                }),
                            },
                            Err(e) => errors.push(ScanError {
                                file_path: pattern.clone(),
                                message: e.to_string(),
                            }),
                        }
                    }
                }
                Err(e) => errors.push(ScanError {
                    file_path: pattern,
                    message: e.to_string(),
                }),
            }
            continue;
        }

        errors.push(ScanError {
            file_path: trimmed.to_string(),
            message: "custom scan path does not exist".to_string(),
        });
    }

    (skills, errors)
}

fn parse_custom_file(path: &Path) -> Result<Skill> {
    parse_generic_md(
        AgentId::Custom("custom-scan".to_string()),
        path,
        SkillScope::Global,
    )
}

fn file_stem_or_default(path: &Path, fallback: &str) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback)
        .to_string()
}

fn is_claude_settings_file(path: &Path, agent_id: &AgentId) -> bool {
    if !matches!(agent_id, AgentId::ClaudeCode) {
        return false;
    }

    let normalized = normalize_path_sep(&path.to_string_lossy().to_lowercase());
    normalized.ends_with("/.claude/settings.json")
        || normalized.ends_with("/.claude/settings.local.json")
}

fn detect_artifact_type(agent_id: &AgentId, path: &Path) -> ArtifactType {
    let normalized = normalize_path_sep(&path.to_string_lossy().to_lowercase());
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if matches!(agent_id, AgentId::ClaudeCode)
        && (normalized.contains("/.claude/commands/")
            || normalized.ends_with("/.claude/commands.md"))
    {
        return ArtifactType::Command;
    }

    // Claude Code plugins ship commands at
    // `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/commands/*.md`.
    // The path doesn't contain `/.claude/commands/`, so the rule above misses
    // them; classify by the plugin-cache prefix instead.
    if matches!(agent_id, AgentId::ClaudeCode)
        && normalized.contains("/.claude/plugins/cache/")
        && normalized.contains("/commands/")
    {
        return ArtifactType::Command;
    }

    if normalized.contains("/.github/prompts/") || file_name.ends_with(".prompt.md") {
        return ArtifactType::Prompt;
    }

    if normalized.contains("/workflows/") {
        return ArtifactType::Workflow;
    }

    if normalized.contains("/rules/")
        || file_name == ".cursorrules"
        || file_name == ".windsurfrules"
        || file_name == ".clinerules"
        || file_name == ".roorules"
    {
        return ArtifactType::Rule;
    }

    ArtifactType::Skill
}

fn sanitize_slash_segment(value: &str) -> String {
    let mut slug = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();

    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }

    slug.trim_matches('-').to_string()
}

fn derive_slash_command_from_path(agent_id: &AgentId, path: &Path) -> Option<String> {
    let normalized = normalize_path_sep(&path.to_string_lossy().to_lowercase());

    if matches!(agent_id, AgentId::ClaudeCode) && normalized.contains("/.claude/commands/") {
        let file = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let cmd = sanitize_slash_segment(file);
        if !cmd.is_empty() {
            return Some(format!("/{}", cmd));
        }
    }

    // Plugin commands live at `.../plugins/cache/<mkt>/<plugin>/<version>/commands/<cmd>.md`.
    // Emit the bare `/<cmd>` here; `annotate_plugin_origin` will namespace it
    // to `/<plugin>:<cmd>` once it knows which plugin owns the path.
    if matches!(agent_id, AgentId::ClaudeCode)
        && normalized.contains("/.claude/plugins/cache/")
        && normalized.contains("/commands/")
    {
        let file = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let cmd = sanitize_slash_segment(file);
        if !cmd.is_empty() {
            return Some(format!("/{}", cmd));
        }
    }

    None
}

/// Detect parent/child relationships between skills based on filesystem paths.
///
/// A skill B is a child of skill A if:
/// - B's file is inside A's directory (path prefix relationship)
/// - A and B are NOT the same skill
/// - A's directory is the nearest ancestor (not an intermediate ancestor)
///
/// Result: `parent_id` is set on child skills.
/// The `children` Vec stays empty — the frontend builds the visual tree from flat data.
///
/// @agent-context: This is a post-processing step. It does NOT modify the flat list
/// structure — the frontend receives all skills flat and renders tree/flat based on
/// its own state. Tree IDs are used by SkillTree.svelte to group skills.
pub fn build_skill_tree(skills: &mut [Skill]) {
    // Collect (skill_id, directory) pairs for all skills
    let id_dirs: Vec<(String, String)> = skills
        .iter()
        .filter_map(|s| {
            let dir = Path::new(&s.file_path)
                .parent()
                .and_then(|p| p.to_str())
                .map(normalize_path_sep)?;
            Some((s.id.clone(), dir))
        })
        .collect();

    // For each skill, find its nearest parent (the longest matching prefix)
    for skill in skills.iter_mut() {
        let skill_dir = match Path::new(&skill.file_path)
            .parent()
            .and_then(|p| p.to_str())
            .map(normalize_path_sep)
        {
            Some(d) => d,
            None => continue,
        };

        // Find the longest ancestor directory that belongs to another skill
        let mut best_parent: Option<(String, usize)> = None; // (id, dir_len)
        for (other_id, other_dir) in &id_dirs {
            if other_id == &skill.id {
                continue;
            }
            let parent_dir = format!("{}{}", other_dir, std::path::MAIN_SEPARATOR);
            if skill_dir.starts_with(&parent_dir)
                || skill_dir.starts_with(other_dir.as_str()) && skill_dir.len() > other_dir.len()
            {
                // Prefer the longest parent (nearest ancestor)
                if best_parent
                    .as_ref()
                    .is_none_or(|(_, len)| other_dir.len() > *len)
                {
                    best_parent = Some((other_id.clone(), other_dir.len()));
                }
            }
        }

        if let Some((parent_id, _)) = best_parent {
            skill.parent_id = Some(parent_id);
        }
    }
}

/// Normalize path separators for cross-platform prefix comparison
fn normalize_path_sep(s: &str) -> String {
    s.replace('\\', "/")
}

/// Expand a glob pattern and parse each matching file.
fn scan_glob_pattern(
    pattern: &str,
    agent: &AgentInfo,
    scope: SkillScope,
    seen_physical_files: &mut HashSet<String>,
    capture_root: Option<&CaptureRoot>,
) -> Result<(Vec<Skill>, Vec<ScanError>)> {
    let mut skills = Vec::new();
    let mut errors = Vec::new();

    // Normalize path separators for the current OS
    let normalized = pattern.replace('\\', "/");

    let paths = collect_candidate_paths(&normalized)?;

    for path in paths {
        // In a capture session a glob or WalkDir result is never trusted on
        // its own: a junction below the fixture root must not redirect parsing
        // into the real profile (or anywhere else).
        if capture_root.is_some_and(|root| !root.contains_file(&path)) {
            continue;
        }
        if !seen_physical_files.insert(canonical_file_identity(&path)) {
            continue;
        }
        match parse_file_for_agent(&path, agent, scope.clone()) {
            Ok(parsed) => skills.extend(parsed),
            Err(e) => errors.push(ScanError {
                file_path: path.to_string_lossy().to_string(),
                message: e.to_string(),
            }),
        }
    }

    Ok((skills, errors))
}

fn collect_candidate_paths(pattern: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let globbed = glob::glob(pattern)?;
    for path in globbed.flatten() {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }

    // The WalkDir pass (which follows symlinks) only adds value when glob found
    // nothing — e.g. a symlinked skills root that glob will not traverse. When
    // glob already matched files, walking the whole subtree again is pure
    // redundant I/O, so skip it (SCAN-02).
    if out.is_empty() {
        for path in collect_with_walkdir(pattern) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }

    Ok(out)
}

fn collect_with_walkdir(pattern: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let normalized = pattern.replace('\\', "/");
    let Some((base, suffix)) = normalized.split_once("/**/") else {
        return out;
    };

    let base_path = Path::new(base);
    if !base_path.exists() {
        return out;
    }

    let file_match = suffix.trim_start_matches('/');
    if file_match.is_empty() {
        return out;
    }

    for entry in WalkDir::new(base_path)
        .follow_links(true)
        .max_depth(16)
        .into_iter()
        .filter_map(|e| e.ok())
        .take(10_000)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let Some(name) = entry.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if wildcard_match(name, file_match) {
            out.push(entry.path().to_path_buf());
        }
    }

    out
}

fn wildcard_match(input: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return input == pattern;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 2 {
        let prefix = parts[0];
        let suffix = parts[1];
        return input.starts_with(prefix) && input.ends_with(suffix);
    }

    let mut cursor = 0usize;
    for part in parts {
        if part.is_empty() {
            continue;
        }
        let Some(found) = input[cursor..].find(part) else {
            return false;
        };
        cursor += found + part.len();
    }

    true
}

/// Parse a single file using the appropriate parser for the agent's format,
/// then run repo/install detection to enrich metadata.
pub(crate) fn parse_file_for_agent(
    path: &Path,
    agent: &AgentInfo,
    scope: SkillScope,
) -> Result<Vec<Skill>> {
    if is_claude_settings_file(path, &agent.id) {
        return parse_claude_hooks(path, scope, None);
    }

    let mut skill = match agent.format {
        // SKILL.md: richest format, used by Claude Code and Codex
        SkillFormat::SkillMd => {
            let is_skill_md = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
                .unwrap_or(false);

            if is_skill_md {
                parse_skill_md(agent.id.clone(), path, scope, None)
            } else {
                parse_generic_md(agent.id.clone(), path, scope)
            }
        }

        // All other formats: use the frontmatter parser and normalize
        SkillFormat::Mdc
        | SkillFormat::InstructionsMd
        | SkillFormat::PlainMarkdown
        | SkillFormat::RulesDir => parse_generic_md(agent.id.clone(), path, scope),

        // YAML configs (Aider) — treat the whole file as a "skill" with the filename as name
        SkillFormat::Yaml | SkillFormat::Json => parse_config_file(agent.id.clone(), path, scope),
    }?;

    // @agent-context: Post-parse enrichment — detect repo URLs and install commands.
    // Read the file content once for regex scanning (already read by parser,
    // but parsers don't return raw content — acceptable perf cost for <1KB files).
    if let Ok(content) = std::fs::read_to_string(path) {
        let parsed = parse_frontmatter(&content).ok();
        let fm = parsed.as_ref().and_then(|p| p.frontmatter.as_ref());
        let body = parsed.as_ref().map(|p| p.body.as_str()).unwrap_or(&content);

        let detected = crate::detection::repo_detector::detect_sources(fm, body, path);
        if skill.metadata.repository_url.is_none() {
            skill.metadata.repository_url = detected.repository_url;
        }
        if skill.metadata.install_command.is_none() {
            skill.metadata.install_command = detected.install_command;
        }
    }

    Ok(vec![skill])
}

/// Generic markdown parser for agents that use .mdc, .md rules, or plain markdown.
fn parse_generic_md(agent_id: AgentId, path: &Path, scope: SkillScope) -> Result<Skill> {
    let content = std::fs::read_to_string(path)?;
    let parsed = parse_frontmatter(&content)?;

    let file_stem = file_stem_or_default(path, "unknown");

    let artifact_type = detect_artifact_type(&agent_id, path);
    let slash_from_path = derive_slash_command_from_path(&agent_id, path);

    let (name, description, metadata) = match &parsed.frontmatter {
        Some(fm) => {
            let name = crate::parsers::frontmatter::yaml_str(fm, "name")
                .or_else(|| {
                    crate::parsers::frontmatter::yaml_str(fm, "description").map(|d| {
                        // Some formats use description as the identifier
                        d.chars().take(50).collect::<String>()
                    })
                })
                .unwrap_or_else(|| file_stem.clone());
            let desc = crate::parsers::frontmatter::yaml_str(fm, "description").unwrap_or_default();

            let trigger = if yaml_bool(fm, "alwaysApply") == Some(true) {
                Some("always".to_string())
            } else if yaml_bool(fm, "disable-model-invocation") == Some(true) {
                Some("manual".to_string())
            } else if yaml_bool(fm, "user-invocable") == Some(false) {
                Some("auto".to_string())
            } else {
                yaml_str(fm, "trigger")
            };

            let user_invocable = yaml_bool(fm, "user-invocable");
            let slash_from_name = if user_invocable == Some(true) {
                yaml_str(fm, "name")
                    .map(|name| sanitize_slash_segment(&name))
                    .filter(|slug| !slug.is_empty())
                    .map(|slug| format!("/{}", slug))
            } else {
                None
            };

            let metadata = crate::models::SkillMetadata {
                version: yaml_str(fm, "version"),
                author: yaml_str(fm, "author")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "author"))),
                category: yaml_str(fm, "category")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "category"))),
                tags: yaml_string_list(fm, "tags")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_string_list(m, "tags"))),
                use_cases: yaml_string_list(fm, "use-cases")
                    .or_else(|| yaml_string_list(fm, "use_cases"))
                    .or_else(|| {
                        fm.get("metadata")
                            .and_then(|m| yaml_string_list(m, "use-cases"))
                    })
                    .or_else(|| {
                        fm.get("metadata")
                            .and_then(|m| yaml_string_list(m, "use_cases"))
                    }),
                globs: yaml_string_array(fm, "globs")
                    .or_else(|| yaml_string_array(fm, "paths"))
                    .or_else(|| yaml_string_array(fm, "applyTo")),
                trigger,
                allowed_tools: yaml_str(fm, "allowed-tools"),
                user_invocable,
                language: yaml_str(fm, "language")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "language"))),
                slash_command: slash_from_name.or_else(|| slash_from_path.clone()),
                hook_event: None,
                hook_matcher: None,
                hook_command: None,
                extra: serde_json::to_value(fm).ok(),
                repository_url: None,
                install_command: None,
            };

            (name, desc, metadata)
        }
        None => {
            // No frontmatter: use first heading or first line
            let first_line = parsed
                .body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim_start_matches('#')
                .trim()
                .to_string();
            let metadata = crate::models::SkillMetadata {
                slash_command: slash_from_path.clone(),
                ..crate::models::SkillMetadata::default()
            };
            (file_stem.clone(), first_line, metadata)
        }
    };

    let id = format!(
        "{}:{}",
        serde_json::to_string(&agent_id)?.trim_matches('"'),
        file_stem
    );

    Ok(Skill {
        id,
        name,
        description,
        artifact_type,
        agent_id: agent_id.clone(),
        source_agents: vec![agent_id],
        file_path: path.to_string_lossy().to_string(),
        source_paths: vec![path.to_string_lossy().to_string()],
        legacy_ids: vec![],
        scope,
        project_path: None,
        metadata,
        discovery_tags: vec![],
        use_cases: vec![],
        discovery_hints: vec![],
        icon: None,
        starred: false,
        update_available: false,
        installed_at: None,
        last_modified_at: None,
        archive_count: 0,
        parent_id: None,
        children: vec![],
    })
}

/// Parse YAML/JSON config files (Aider, Cody) as single "skills".
fn parse_config_file(agent_id: AgentId, path: &Path, scope: SkillScope) -> Result<Skill> {
    let file_stem = file_stem_or_default(path, "config");

    let id = format!(
        "{}:{}",
        serde_json::to_string(&agent_id)?.trim_matches('"'),
        file_stem
    );

    Ok(Skill {
        id,
        name: file_stem,
        description: format!(
            "Configuration file: {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ),
        artifact_type: ArtifactType::Config,
        agent_id: agent_id.clone(),
        source_agents: vec![agent_id],
        file_path: path.to_string_lossy().to_string(),
        source_paths: vec![path.to_string_lossy().to_string()],
        legacy_ids: vec![],
        scope,
        project_path: None,
        metadata: crate::models::SkillMetadata::default(),
        discovery_tags: vec![],
        use_cases: vec![],
        discovery_hints: vec![],
        icon: None,
        starred: false,
        update_available: false,
        installed_at: None,
        last_modified_at: None,
        archive_count: 0,
        parent_id: None,
        children: vec![],
    })
}

/// Check which agents are installed by testing if their directories exist.
pub fn detect_installed_agents() -> Vec<AgentInfo> {
    let (home, mut registry) = match capture_root_mode_from_process() {
        CaptureRootMode::Normal => (
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            get_agent_registry(),
        ),
        CaptureRootMode::Capture(capture_root) => {
            let environment = LocationEnvironment::for_capture(&capture_root);
            (
                capture_root.root().to_path_buf(),
                get_agent_registry_with_environment(&environment),
            )
        }
        // Do not resolve the normal registry while an explicitly requested
        // capture contract is malformed: even registry construction can read
        // bounded editor settings from a real profile.
        CaptureRootMode::Invalid(_) => return Vec::new(),
    };

    // API-01: report a real per-agent skill_count instead of a hardcoded 0.
    // Shared stores are counted under Universal, not copied onto every agent
    // that happens to support the shared format.
    let scan = scan_all_skills();
    let mut counts: HashMap<AgentId, usize> = HashMap::new();
    for skill in &scan.skills {
        let agents = if skill.source_agents.is_empty() {
            std::slice::from_ref(&skill.agent_id)
        } else {
            skill.source_agents.as_slice()
        };
        for agent in agents {
            *counts.entry(agent.clone()).or_insert(0) += 1;
        }
    }

    for agent in &mut registry {
        agent.installed = is_agent_installed(agent, &home);
        agent.skill_count = counts.get(&agent.id).copied().unwrap_or(0);
    }

    registry
}

fn should_scan_agent(agent: &AgentInfo, home: &Path) -> bool {
    is_agent_installed(agent, home)
        || agent
            .global_paths
            .iter()
            .any(|path| resolved_path_exists(path, home))
}

/// Installation is determined only from agent-specific probes. A compatible
/// shared skill store is never installation evidence for Cline, Kimi, Warp,
/// Zed, or any other consumer.
pub(crate) fn is_agent_installed(agent: &AgentInfo, home: &Path) -> bool {
    if matches!(agent.id, AgentId::Universal) {
        return false;
    }

    agent
        .global_detection_paths
        .iter()
        .any(|path| resolved_path_exists(path, home))
}

fn resolved_path_exists(pattern: &str, home: &Path) -> bool {
    let mut resolved = pattern.replace("$HOME", &home.to_string_lossy());
    resolved = resolved.replace("$PROJECT", "");

    if resolved.contains('*') {
        let parent = resolved
            .rsplit_once('/')
            .map(|(dir, _)| dir.to_string())
            .unwrap_or(resolved.clone());
        let clean_parent = parent
            .split('*')
            .next()
            .unwrap_or(&parent)
            .trim_end_matches('/');
        if clean_parent.is_empty() {
            return false;
        }
        Path::new(clean_parent).exists()
    } else {
        Path::new(&resolved).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::locations::CaptureRoot;
    use crate::models::{AgentId, AgentInfo, ArtifactType, SkillFormat, SkillMetadata, SkillScope};
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_skill(id: &str, file_path: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            artifact_type: ArtifactType::Skill,
            agent_id: AgentId::ClaudeCode,
            source_agents: vec![AgentId::ClaudeCode],
            file_path: file_path.to_string(),
            source_paths: vec![file_path.to_string()],
            legacy_ids: vec![],
            scope: SkillScope::Global,
            project_path: None,
            metadata: SkillMetadata::default(),
            discovery_tags: vec![],
            use_cases: vec![],
            discovery_hints: vec![],
            icon: None,
            starred: false,
            update_available: false,
            installed_at: None,
            last_modified_at: None,
            archive_count: 0,
            parent_id: None,
            children: vec![],
        }
    }

    fn write_capture_skill(root: &Path, name: &str) {
        let directory = root.join(".agents").join("skills").join(name);
        fs::create_dir_all(&directory).expect("capture skill directory");
        fs::write(
            directory.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: synthetic\n---\nfixture"),
        )
        .expect("capture skill");
    }

    #[test]
    fn capture_context_scans_only_the_four_sentinel_bound_fixture_skills() {
        let temp = tempfile::tempdir().expect("capture tempdir");
        fs::write(
            temp.path().join(".skill-deck-capture-root"),
            "skill-deck-capture-v1\n",
        )
        .expect("capture sentinel");
        for name in ["one", "two", "three", "four"] {
            write_capture_skill(temp.path(), name);
        }
        let root =
            CaptureRoot::from_declared_path(temp.path().to_path_buf()).expect("valid capture root");
        let environment = LocationEnvironment::for_capture(&root);

        let scan = scan_all_skills_with_context(
            get_agent_registry_with_environment(&environment),
            root.root().to_path_buf(),
            &[],
            Some(&root),
        );

        assert!(
            scan.errors.is_empty(),
            "unexpected errors: {:?}",
            scan.errors
        );
        assert_eq!(scan.skills.len(), CAPTURE_FIXTURE_SKILL_COUNT);
        assert!(scan
            .skills
            .iter()
            .all(|skill| root.contains_file(Path::new(&skill.file_path))));
    }

    #[test]
    fn capture_context_fails_closed_when_a_fixture_skill_is_a_symlink_escape() {
        let temp = tempfile::tempdir().expect("capture tempdir");
        fs::write(
            temp.path().join(".skill-deck-capture-root"),
            "skill-deck-capture-v1\n",
        )
        .expect("capture sentinel");
        for name in ["one", "two", "three"] {
            write_capture_skill(temp.path(), name);
        }
        let outside = temp.path().with_extension("outside-skill.md");
        fs::write(&outside, "---\nname: Outside\n---\nnope").expect("outside skill");
        let link_dir = temp.path().join(".agents").join("skills").join("four");
        fs::create_dir_all(&link_dir).expect("link directory");
        let link = link_dir.join("SKILL.md");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&outside, &link);
        if link_result.is_err() {
            return;
        }

        let root =
            CaptureRoot::from_declared_path(temp.path().to_path_buf()).expect("valid capture root");
        let environment = LocationEnvironment::for_capture(&root);
        let scan = scan_all_skills_with_context(
            get_agent_registry_with_environment(&environment),
            root.root().to_path_buf(),
            &[],
            Some(&root),
        );

        assert!(scan.skills.is_empty(), "partial capture must fail closed");
        assert!(scan
            .errors
            .iter()
            .any(|error| error.message.contains("exactly 4 skills")));
    }

    #[test]
    fn test_build_skill_tree_flat_no_relationships() {
        let mut skills = vec![
            make_skill("a", "/skills/foo/SKILL.md"),
            make_skill("b", "/skills/bar/SKILL.md"),
        ];
        build_skill_tree(&mut skills);
        assert!(skills[0].parent_id.is_none());
        assert!(skills[1].parent_id.is_none());
    }

    #[test]
    fn test_build_skill_tree_detects_child() {
        let mut skills = vec![
            make_skill("parent", "/skills/parent/SKILL.md"),
            make_skill("child", "/skills/parent/sub-skill/SKILL.md"),
        ];
        build_skill_tree(&mut skills);
        // child should have parent_id set
        let child = skills.iter().find(|s| s.id == "child").unwrap();
        assert_eq!(child.parent_id.as_deref(), Some("parent"));
        // parent should NOT have a parent_id
        let parent = skills.iter().find(|s| s.id == "parent").unwrap();
        assert!(parent.parent_id.is_none());
    }

    #[test]
    fn test_build_skill_tree_nearest_ancestor() {
        // grandparent/parent/child hierarchy
        let mut skills = vec![
            make_skill("grandparent", "/skills/gp/SKILL.md"),
            make_skill("parent", "/skills/gp/parent/SKILL.md"),
            make_skill("child", "/skills/gp/parent/child/SKILL.md"),
        ];
        build_skill_tree(&mut skills);
        let child = skills.iter().find(|s| s.id == "child").unwrap();
        // child's nearest ancestor is "parent", not "grandparent"
        assert_eq!(child.parent_id.as_deref(), Some("parent"));
        let parent = skills.iter().find(|s| s.id == "parent").unwrap();
        assert_eq!(parent.parent_id.as_deref(), Some("grandparent"));
        let gp = skills.iter().find(|s| s.id == "grandparent").unwrap();
        assert!(gp.parent_id.is_none());
    }

    #[test]
    fn test_build_skill_tree_siblings_no_parent_relationship() {
        let mut skills = vec![
            make_skill("parent", "/skills/parent/SKILL.md"),
            make_skill("child-a", "/skills/parent/a/SKILL.md"),
            make_skill("child-b", "/skills/parent/b/SKILL.md"),
        ];
        build_skill_tree(&mut skills);
        let a = skills.iter().find(|s| s.id == "child-a").unwrap();
        let b = skills.iter().find(|s| s.id == "child-b").unwrap();
        // siblings both have the same parent
        assert_eq!(a.parent_id.as_deref(), Some("parent"));
        assert_eq!(b.parent_id.as_deref(), Some("parent"));
        // and NOT each other
        assert_ne!(a.parent_id.as_deref(), Some("child-b"));
        assert_ne!(b.parent_id.as_deref(), Some("child-a"));
    }

    #[test]
    fn test_scan_custom_paths_single_file() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "# Custom Skill").unwrap();
        writeln!(file, "Description body").unwrap();

        let (skills, errors) = scan_custom_paths(&[file.path().to_string_lossy().to_string()]);
        assert_eq!(errors.len(), 0);
        assert_eq!(skills.len(), 1);
        assert_eq!(
            skills[0].agent_id,
            AgentId::Custom("custom-scan".to_string())
        );
    }

    #[test]
    fn test_scan_custom_path_with_malformed_fenced_yaml_keeps_visible_body() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "---").unwrap();
        writeln!(
            file,
            "description: No written brief exists yet: a new landing page"
        )
        .unwrap();
        writeln!(file, "---").unwrap();
        writeln!(file, "# Visible scanned skill").unwrap();
        writeln!(file, "Readable instructions.").unwrap();

        let (skills, errors) = scan_custom_paths(&[file.path().to_string_lossy().to_string()]);

        assert!(errors.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "Visible scanned skill");
    }

    #[test]
    fn test_scan_custom_paths_missing_path_reports_error() {
        let (skills, errors) = scan_custom_paths(&["C:/definitely/not/here/skill.md".to_string()]);
        assert_eq!(skills.len(), 0);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn test_parse_file_for_agent_classifies_claude_command() {
        let temp = tempfile::tempdir().expect("temp dir");
        let command_dir = temp.path().join(".claude").join("commands");
        fs::create_dir_all(&command_dir).expect("create command dir");
        let command_path = command_dir.join("review.md");
        fs::write(&command_path, "# Review command\n").expect("write command file");

        let agent = AgentInfo {
            id: AgentId::ClaudeCode,
            display_name: "Claude Code".to_string(),
            description: "Anthropic CLI coding agent".to_string(),
            color: "#f28c54".to_string(),
            installed: true,
            skill_count: 0,
            global_paths: vec![],
            global_detection_paths: vec![],
            project_paths: vec![],
            format: SkillFormat::SkillMd,
        };

        let parsed = parse_file_for_agent(&command_path, &agent, SkillScope::Project)
            .expect("parse command");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].artifact_type, ArtifactType::Command);
        assert_eq!(parsed[0].metadata.slash_command.as_deref(), Some("/review"));
    }

    #[test]
    fn reassign_skill_agent_restamps_id_and_agents() {
        let base = make_skill("claude-code:foo", "/x/foo/SKILL.md");
        let out = reassign_skill_agent(base, &AgentId::Codex);
        assert_eq!(out.id, "codex:foo");
        assert_eq!(out.agent_id, AgentId::Codex);
        assert_eq!(out.source_agents, vec![AgentId::Codex]);
    }

    #[test]
    fn resolve_project_roots_keeps_existing_drops_missing() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().to_string_lossy().to_string();
        let roots = resolve_project_roots(&[
            existing.clone(),
            "Q:/definitely/not/here".to_string(),
            "   ".to_string(),
        ]);
        let norm = existing
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();
        assert!(
            roots.contains(&norm),
            "expected configured root in {:?}",
            roots
        );
        assert!(!roots.iter().any(|r| r.contains("definitely/not/here")));
    }

    #[test]
    fn scan_pattern_group_replicates_across_agents() {
        // SCAN-02: one parse, one record per declaring agent. dedup later folds
        // them into a single card whose source_agents lists both.
        let temp = tempfile::tempdir().unwrap();
        let skill_dir = temp.path().join("demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Demo\ndescription: d\n---\nbody",
        )
        .unwrap();

        let pattern = format!(
            "{}/**/SKILL.md",
            temp.path().to_string_lossy().replace('\\', "/")
        );
        let mut seen = HashSet::new();
        let (skills, errors) = scan_pattern_group(
            &pattern,
            &SkillFormat::SkillMd,
            &[AgentId::ClaudeCode, AgentId::Codex],
            SkillScope::Global,
            None,
            &mut seen,
            None,
        );

        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert_eq!(skills.len(), 2);
        let agents: Vec<_> = skills.iter().map(|s| s.agent_id.clone()).collect();
        assert!(agents.contains(&AgentId::ClaudeCode));
        assert!(agents.contains(&AgentId::Codex));
    }

    #[test]
    fn shared_store_universal_attribution_overrides_agent_aliases() {
        let mut order = Vec::new();
        let mut groups = HashMap::new();
        let canonical = PathBuf::from("C:/physical/shared");

        for (pattern, agent) in [
            ("C:/aliases/claude/**/SKILL.md", AgentId::ClaudeCode),
            (
                "C:/users/test/.agents/skills/**/SKILL.md",
                AgentId::Universal,
            ),
        ] {
            let resolved = pattern.to_string();
            let key = (
                physical_pattern_key_with(&resolved, |_| Some(canonical.clone())),
                SkillFormat::SkillMd,
            );
            let agents = groups.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                Vec::new()
            });
            if matches!(agent, AgentId::Universal) {
                agents.clear();
                agents.push(agent);
            } else if !agents.contains(&AgentId::Universal) {
                agents.push(agent);
            }
        }

        assert_eq!(
            order.len(),
            1,
            "junction aliases must share one physical key"
        );
        assert_eq!(
            groups.get(&order[0]),
            Some(&vec![AgentId::Universal]),
            "shared store must not inherit arbitrary agent ownership"
        );
    }

    #[test]
    fn physical_identity_folds_case_only_on_windows() {
        let path = Path::new("C:/Users/Test/.Agents/Skills");
        assert_eq!(
            physical_path_identity_for_platform(path, true),
            "c:/users/test/.agents/skills"
        );
        assert_eq!(
            physical_path_identity_for_platform(path, false),
            "C:/Users/Test/.Agents/Skills"
        );
    }

    #[test]
    fn dedupe_keeps_case_distinct_sources_on_case_sensitive_platforms() {
        let upper = make_skill("one", "/home/test/Skills/demo/SKILL.md");
        let mut lower = make_skill("two", "/home/test/skills/demo/SKILL.md");
        lower.agent_id = AgentId::Codex;
        lower.source_agents = vec![AgentId::Codex];

        let mut case_sensitive = vec![upper.clone(), lower.clone()];
        dedupe_skills_by_physical_source_for_platform(&mut case_sensitive, false);
        assert_eq!(case_sensitive.len(), 2);

        let mut case_insensitive = vec![upper, lower];
        dedupe_skills_by_physical_source_for_platform(&mut case_insensitive, true);
        assert_eq!(case_insensitive.len(), 1);
    }

    #[test]
    fn project_scan_records_its_source_root() {
        let temp = tempfile::tempdir().expect("temp dir");
        let skill_dir = temp.path().join(".agents").join("skills").join("demo");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Demo\ndescription: d\n---\nbody",
        )
        .expect("write skill");
        let root = temp.path().to_string_lossy().replace('\\', "/");
        let pattern = format!("{root}/.agents/skills/**/SKILL.md");

        let mut seen = HashSet::new();
        let (skills, errors) = scan_pattern_group(
            &pattern,
            &SkillFormat::SkillMd,
            &[AgentId::Universal],
            SkillScope::Project,
            Some(&root),
            &mut seen,
            None,
        );

        assert!(errors.is_empty());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].scope, SkillScope::Project);
        assert_eq!(skills[0].project_path.as_deref(), Some(root.as_str()));
        assert_eq!(skills[0].agent_id, AgentId::Universal);
    }

    #[test]
    fn shared_store_does_not_mark_absent_agent_installed() {
        let temp = tempfile::tempdir().expect("temp dir");
        let shared = temp.path().join(".agents").join("skills");
        fs::create_dir_all(&shared).expect("create shared store");
        let agent = AgentInfo {
            id: AgentId::Warp,
            display_name: "Warp".to_string(),
            description: String::new(),
            color: String::new(),
            installed: false,
            skill_count: 0,
            global_paths: vec![format!(
                "{}/**/SKILL.md",
                shared.to_string_lossy().replace('\\', "/")
            )],
            global_detection_paths: vec![temp.path().join(".warp").to_string_lossy().to_string()],
            project_paths: Vec::new(),
            format: SkillFormat::SkillMd,
        };

        assert!(should_scan_agent(&agent, temp.path()));
        assert!(!is_agent_installed(&agent, temp.path()));
    }

    #[test]
    fn current_device_probes_detect_hermes_kimi_and_zed_when_present() {
        let home = dirs::home_dir().expect("home");
        let registry = get_agent_registry();
        for id in [AgentId::HermesAgent, AgentId::KimiCodeCli, AgentId::Zed] {
            let agent = registry
                .iter()
                .find(|agent| agent.id == id)
                .expect("catalog agent");
            let any_probe_exists = agent
                .global_detection_paths
                .iter()
                .any(|path| resolved_path_exists(path, &home));
            if any_probe_exists {
                assert!(
                    is_agent_installed(agent, &home),
                    "{id:?} has a device probe but was not detected"
                );
            }
        }
    }

    #[test]
    fn current_device_shared_junction_targets_remain_universal() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let shared = home.join(".agents").join("skills");
        let Ok(shared_canonical) = std::fs::canonicalize(&shared) else {
            return;
        };

        let scan = scan_all_skills_with_roots(&[]);
        let shared_skills: Vec<_> = scan
            .skills
            .iter()
            .filter(|skill| {
                std::fs::canonicalize(&skill.file_path)
                    .is_ok_and(|path| path.starts_with(&shared_canonical))
            })
            .collect();

        assert!(
            !shared_skills.is_empty(),
            "existing shared store should produce visible skills"
        );
        assert!(
            shared_skills
                .iter()
                .all(|skill| skill.agent_id == AgentId::Universal),
            "shared junction targets must retain neutral attribution: {:?}",
            shared_skills
                .iter()
                .filter(|skill| skill.agent_id != AgentId::Universal)
                .map(|skill| (&skill.agent_id, &skill.file_path))
                .collect::<Vec<_>>()
        );
    }
}
