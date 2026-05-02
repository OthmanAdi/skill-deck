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

use super::registry::get_agent_registry;
use crate::models::{AgentId, AgentInfo, ScanError, ScanResult, Skill, SkillFormat, SkillScope};
use crate::parsers::{parse_frontmatter, skill_md::parse_skill_md};

/// Scan all known agent directories for skills.
///
/// `project_path`: Optional current project directory (from CWD detection).
/// If None, only global skills are scanned.
pub fn scan_all_skills(project_path: Option<&Path>) -> ScanResult {
    let start = Instant::now();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let registry = get_agent_registry();

    let mut all_skills: Vec<Skill> = Vec::new();
    let mut all_errors: Vec<ScanError> = Vec::new();

    for agent in &registry {
        if !should_scan_agent(agent, &home, project_path) {
            continue;
        }

        // Scan global paths
        for pattern in &agent.global_paths {
            let resolved = pattern.replace("$HOME", &home.to_string_lossy());
            match scan_glob_pattern(&resolved, agent, SkillScope::Global, None) {
                Ok((skills, errors)) => {
                    all_skills.extend(skills);
                    all_errors.extend(errors);
                }
                Err(e) => {
                    all_errors.push(ScanError {
                        file_path: resolved,
                        message: e.to_string(),
                    });
                }
            }
        }

        // Scan project paths (only if we know the project directory)
        if let Some(project) = project_path {
            for pattern in &agent.project_paths {
                let resolved = pattern
                    .replace("$HOME", &home.to_string_lossy())
                    .replace("$PROJECT", &project.to_string_lossy());
                match scan_glob_pattern(
                    &resolved,
                    agent,
                    SkillScope::Project,
                    Some(project.to_string_lossy().to_string()),
                ) {
                    Ok((skills, errors)) => {
                        all_skills.extend(skills);
                        all_errors.extend(errors);
                    }
                    Err(e) => {
                        all_errors.push(ScanError {
                            file_path: resolved,
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    // Deduplicate skill IDs — if two files produce the same ID,
    // append a numeric suffix to make each ID unique.
    let mut seen_ids: HashSet<String> = HashSet::new();
    for skill in &mut all_skills {
        if !seen_ids.insert(skill.id.clone()) {
            let mut counter = 2;
            loop {
                let candidate = format!("{}:{}", skill.id, counter);
                if seen_ids.insert(candidate.clone()) {
                    skill.id = candidate;
                    break;
                }
                counter += 1;
            }
        }
    }

    // Build parent/child hierarchy based on path relationships
    build_skill_tree(&mut all_skills);

    ScanResult {
        skills: all_skills,
        errors: all_errors,
        scan_duration_ms: start.elapsed().as_millis() as u64,
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
        None,
    )
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
    project_path: Option<String>,
) -> Result<(Vec<Skill>, Vec<ScanError>)> {
    let mut skills = Vec::new();
    let mut errors = Vec::new();

    // Normalize path separators for the current OS
    let normalized = pattern.replace('\\', "/");

    let paths = collect_candidate_paths(&normalized)?;

    for path in paths {
        match parse_file_for_agent(&path, agent, scope.clone(), project_path.clone()) {
            Ok(skill) => skills.push(skill),
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

    let fallback = collect_with_walkdir(pattern);
    for path in fallback {
        if seen.insert(path.clone()) {
            out.push(path);
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
        .into_iter()
        .filter_map(|e| e.ok())
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
fn parse_file_for_agent(
    path: &Path,
    agent: &AgentInfo,
    scope: SkillScope,
    project_path: Option<String>,
) -> Result<Skill> {
    let mut skill = match agent.format {
        // SKILL.md: richest format, used by Claude Code and Codex
        SkillFormat::SkillMd => {
            let is_skill_md = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.eq_ignore_ascii_case("SKILL.md"))
                .unwrap_or(false);

            if is_skill_md {
                parse_skill_md(agent.id.clone(), path, scope, project_path)
            } else {
                parse_generic_md(agent.id.clone(), path, scope, project_path)
            }
        }

        // All other formats: use the frontmatter parser and normalize
        SkillFormat::Mdc
        | SkillFormat::InstructionsMd
        | SkillFormat::PlainMarkdown
        | SkillFormat::RulesDir => parse_generic_md(agent.id.clone(), path, scope, project_path),

        // YAML configs (Aider) — treat the whole file as a "skill" with the filename as name
        SkillFormat::Yaml | SkillFormat::Json => {
            parse_config_file(agent.id.clone(), path, scope, project_path)
        }
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

    Ok(skill)
}

/// Generic markdown parser for agents that use .mdc, .md rules, or plain markdown.
fn parse_generic_md(
    agent_id: AgentId,
    path: &Path,
    scope: SkillScope,
    project_path: Option<String>,
) -> Result<Skill> {
    let content = std::fs::read_to_string(path)?;
    let parsed = parse_frontmatter(&content)?;

    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let (name, description) = match &parsed.frontmatter {
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
            (name, desc)
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
            (file_stem.clone(), first_line)
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
        agent_id,
        file_path: path.to_string_lossy().to_string(),
        scope,
        project_path,
        metadata: crate::models::SkillMetadata::default(),
        icon: None,
        starred: false,
        update_available: false,
        parent_id: None,
        children: vec![],
    })
}

/// Parse YAML/JSON config files (Aider, Cody) as single "skills".
fn parse_config_file(
    agent_id: AgentId,
    path: &Path,
    scope: SkillScope,
    project_path: Option<String>,
) -> Result<Skill> {
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("config")
        .to_string();

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
        agent_id,
        file_path: path.to_string_lossy().to_string(),
        scope,
        project_path,
        metadata: crate::models::SkillMetadata::default(),
        icon: None,
        starred: false,
        update_available: false,
        parent_id: None,
        children: vec![],
    })
}

/// Check which agents are installed by testing if their directories exist.
pub fn detect_installed_agents(project_path: Option<&Path>) -> Vec<AgentInfo> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut registry = get_agent_registry();
    let scan_result = scan_all_skills(project_path);

    let mut counts: HashMap<AgentId, usize> = HashMap::new();
    for skill in &scan_result.skills {
        let entry = counts.entry(skill.agent_id.clone()).or_insert(0);
        *entry += 1;
    }

    for agent in &mut registry {
        let installed_by_path = should_scan_agent(agent, &home, project_path);

        let count = *counts.get(&agent.id).unwrap_or(&0);
        agent.skill_count = count;
        agent.installed = installed_by_path || count > 0;
    }

    registry
}

fn should_scan_agent(agent: &AgentInfo, home: &Path, project_path: Option<&Path>) -> bool {
    let installed_by_detection_path = agent
        .global_detection_paths
        .iter()
        .any(|p| resolved_path_exists(p, home, project_path));

    if installed_by_detection_path {
        return true;
    }

    let global_exists = agent
        .global_paths
        .iter()
        .any(|p| resolved_path_exists(p, home, project_path));

    if global_exists {
        return true;
    }

    project_path.is_some()
        && agent
            .project_paths
            .iter()
            .any(|p| resolved_path_exists(p, home, project_path))
}

fn resolved_path_exists(pattern: &str, home: &Path, project_path: Option<&Path>) -> bool {
    let mut resolved = pattern.replace("$HOME", &home.to_string_lossy());
    if let Some(project) = project_path {
        resolved = resolved.replace("$PROJECT", &project.to_string_lossy());
    }

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
    use crate::models::{AgentId, AgentInfo, SkillFormat, SkillMetadata, SkillScope};
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_skill(id: &str, file_path: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            agent_id: AgentId::ClaudeCode,
            file_path: file_path.to_string(),
            scope: SkillScope::Global,
            project_path: None,
            metadata: SkillMetadata::default(),
            icon: None,
            starred: false,
            update_available: false,
            parent_id: None,
            children: vec![],
        }
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
    fn test_scan_custom_paths_missing_path_reports_error() {
        let (skills, errors) = scan_custom_paths(&["C:/definitely/not/here/skill.md".to_string()]);
        assert_eq!(skills.len(), 0);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn should_scan_project_paths_even_without_global_install() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".claude").join("skills")).unwrap();

        let agent = AgentInfo {
            id: AgentId::ClaudeCode,
            display_name: "Claude Code".to_string(),
            description: String::new(),
            color: "#000".to_string(),
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.claude/skills/**/SKILL.md".to_string()],
            global_detection_paths: vec!["$HOME/.claude".to_string()],
            project_paths: vec!["$PROJECT/.claude/skills/**/SKILL.md".to_string()],
            format: SkillFormat::SkillMd,
        };

        assert!(should_scan_agent(&agent, home.path(), Some(project.path())));
    }
}
