use std::collections::{HashMap, HashSet};

use crate::models::{AgentId, ArtifactType, Skill, SkillVersionEntry, UpdateCheckEntry};

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim().to_lowercase()
}

pub fn canonical_or_normalized_path(path: &str) -> String {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        normalize_path(&canonical.to_string_lossy())
    } else {
        normalize_path(path)
    }
}

fn normalize_token(value: Option<&str>) -> String {
    value
        .unwrap_or_default()
        .trim()
        .to_lowercase()
        .replace('\n', " ")
}

fn artifact_token(skill: &Skill) -> &'static str {
    match skill.artifact_type {
        ArtifactType::Skill => "skill",
        ArtifactType::Command => "command",
        ArtifactType::Hook => "hook",
        ArtifactType::Rule => "rule",
        ArtifactType::Workflow => "workflow",
        ArtifactType::Prompt => "prompt",
        ArtifactType::Config => "config",
        ArtifactType::Other => "other",
    }
}

fn file_content_hash(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(short_hash_bytes(&bytes))
}

fn short_hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let full = hex_lower(&hasher.finalize());
    full[..16].to_string()
}

fn skill_dedupe_key(skill: &Skill) -> String {
    let canonical_path = canonical_or_normalized_path(&skill.file_path);
    let source_fingerprint = file_content_hash(&skill.file_path).unwrap_or(canonical_path);

    if skill.artifact_type == ArtifactType::Hook {
        return format!(
            "{}|{}|{}|{}|{}",
            artifact_token(skill),
            normalize_token(skill.metadata.hook_event.as_deref()),
            normalize_token(skill.metadata.hook_matcher.as_deref()),
            normalize_token(skill.metadata.hook_command.as_deref()),
            source_fingerprint
        );
    }

    format!(
        "{}|{}|{}",
        artifact_token(skill),
        normalize_token(skill.metadata.slash_command.as_deref()),
        source_fingerprint
    )
}

pub struct ConfigIdCollections<'a> {
    pub starred_skills: &'a mut HashSet<String>,
    pub skill_icons: &'a mut HashMap<String, String>,
    pub skill_repo_overrides: &'a mut HashMap<String, String>,
    pub skill_install_overrides: &'a mut HashMap<String, String>,
    pub update_check_cache: &'a mut HashMap<String, UpdateCheckEntry>,
    pub skill_version_history: &'a mut HashMap<String, Vec<SkillVersionEntry>>,
    pub collapsed_tree_nodes: &'a mut HashSet<String>,
}

pub fn remap_config_ids_for_skills(
    skills: &[Skill],
    collections: ConfigIdCollections<'_>,
) -> bool {
    let ConfigIdCollections {
        starred_skills,
        skill_icons,
        skill_repo_overrides,
        skill_install_overrides,
        update_check_cache,
        skill_version_history,
        collapsed_tree_nodes,
    } = collections;

    let mut changed = false;
    let mut legacy_to_canonical: HashMap<String, String> = HashMap::new();

    for skill in skills {
        for legacy in &skill.legacy_ids {
            if legacy != &skill.id {
                legacy_to_canonical.insert(legacy.clone(), skill.id.clone());
            }
        }
    }

    if legacy_to_canonical.is_empty() {
        return false;
    }

    changed |= remap_hashset(starred_skills, &legacy_to_canonical);
    changed |= remap_hashmap(skill_icons, &legacy_to_canonical);
    changed |= remap_hashmap(skill_repo_overrides, &legacy_to_canonical);
    changed |= remap_hashmap(skill_install_overrides, &legacy_to_canonical);
    changed |= remap_hashmap(update_check_cache, &legacy_to_canonical);
    changed |= remap_hashmap(skill_version_history, &legacy_to_canonical);
    changed |= remap_hashset(collapsed_tree_nodes, &legacy_to_canonical);

    changed
}

fn remap_hashset(set: &mut HashSet<String>, mapping: &HashMap<String, String>) -> bool {
    if set.is_empty() {
        return false;
    }

    let mut changed = false;
    let mut next: HashSet<String> = HashSet::with_capacity(set.len());
    for key in set.iter() {
        if let Some(mapped) = mapping.get(key) {
            next.insert(mapped.clone());
            changed = true;
        } else {
            next.insert(key.clone());
        }
    }

    if changed {
        *set = next;
    }

    changed
}

fn remap_hashmap<V: Clone>(
    map: &mut HashMap<String, V>,
    mapping: &HashMap<String, String>,
) -> bool {
    if map.is_empty() {
        return false;
    }

    let mut changed = false;
    let mut next: HashMap<String, V> = HashMap::with_capacity(map.len());
    for (key, value) in map.iter() {
        if let Some(mapped) = mapping.get(key) {
            next.insert(mapped.clone(), value.clone());
            changed = true;
        } else {
            next.insert(key.clone(), value.clone());
        }
    }

    if changed {
        *map = next;
    }

    changed
}

fn source_priority(path: &str) -> u8 {
    let normalized = normalize_path(path);
    if normalized.contains("/.claude/commands/") || normalized.contains("/.claude/settings") {
        3
    } else if normalized.contains("/.agents/skills/") {
        1
    } else {
        2
    }
}

fn should_replace_primary(existing: &Skill, candidate: &Skill) -> bool {
    let existing_priority = source_priority(&existing.file_path);
    let candidate_priority = source_priority(&candidate.file_path);

    if candidate_priority != existing_priority {
        return candidate_priority > existing_priority;
    }

    let existing_agent = serde_json::to_string(&existing.agent_id)
        .unwrap_or_default()
        .to_lowercase();
    let candidate_agent = serde_json::to_string(&candidate.agent_id)
        .unwrap_or_default()
        .to_lowercase();

    candidate_agent < existing_agent
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn short_hash(value: &str) -> String {
    let full = short_hash_bytes(value.as_bytes());
    full[..12].to_string()
}

fn canonical_skill_id(dedupe_key: &str) -> String {
    format!("artifact:{}", short_hash(dedupe_key))
}

pub fn dedupe_skills_by_source(skills: &mut Vec<Skill>) {
    let mut grouped: HashMap<String, Vec<Skill>> = HashMap::new();
    for skill in skills.drain(..) {
        let key = skill_dedupe_key(&skill);
        grouped.entry(key).or_default().push(skill);
    }

    let mut merged: Vec<Skill> = Vec::new();
    for (dedupe_key, mut group) in grouped {
        if group.is_empty() {
            continue;
        }

        group.sort_by(|a, b| a.file_path.cmp(&b.file_path));
        let mut primary = group.remove(0);
        let mut source_agents: Vec<AgentId> = if primary.source_agents.is_empty() {
            vec![primary.agent_id.clone()]
        } else {
            primary.source_agents.clone()
        };
        let mut source_paths: Vec<String> = if primary.source_paths.is_empty() {
            vec![primary.file_path.clone()]
        } else {
            primary.source_paths.clone()
        };
        let mut legacy_ids: Vec<String> = if primary.legacy_ids.is_empty() {
            vec![primary.id.clone()]
        } else {
            primary.legacy_ids.clone()
        };

        if !legacy_ids.iter().any(|id| id == &primary.id) {
            legacy_ids.push(primary.id.clone());
        }

        for candidate in group {
            if should_replace_primary(&primary, &candidate) {
                primary = candidate.clone();
            }

            if candidate.source_agents.is_empty() {
                if !source_agents.iter().any(|agent| agent == &candidate.agent_id) {
                    source_agents.push(candidate.agent_id.clone());
                }
            } else {
                for agent in &candidate.source_agents {
                    if !source_agents.iter().any(|existing| existing == agent) {
                        source_agents.push(agent.clone());
                    }
                }
            }

            if candidate.source_paths.is_empty() {
                if !source_paths
                    .iter()
                    .any(|path| normalize_path(path) == normalize_path(&candidate.file_path))
                {
                    source_paths.push(candidate.file_path.clone());
                }
            } else {
                for source in &candidate.source_paths {
                    if !source_paths
                        .iter()
                        .any(|path| normalize_path(path) == normalize_path(source))
                    {
                        source_paths.push(source.clone());
                    }
                }
            }

            for legacy in &candidate.legacy_ids {
                if !legacy_ids.iter().any(|id| id == legacy) {
                    legacy_ids.push(legacy.clone());
                }
            }

            if !legacy_ids.iter().any(|id| id == &candidate.id) {
                legacy_ids.push(candidate.id);
            }
        }

        source_agents.sort_by_key(|agent| serde_json::to_string(agent).unwrap_or_default());
        source_paths.sort();
        legacy_ids.sort();

        primary.id = canonical_skill_id(&dedupe_key);
        primary.source_agents = source_agents;
        primary.source_paths = source_paths;
        primary.legacy_ids = legacy_ids;
        merged.push(primary);
    }

    merged.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.file_path.to_lowercase().cmp(&b.file_path.to_lowercase()))
    });

    *skills = merged;
}

#[cfg(test)]
mod tests {
    use super::dedupe_skills_by_source;
    use crate::models::{AgentId, ArtifactType, Skill, SkillMetadata, SkillScope};

    fn make_skill(id: &str, file_path: &str, agent_id: AgentId) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: String::new(),
            artifact_type: ArtifactType::Skill,
            agent_id: agent_id.clone(),
            source_agents: vec![agent_id],
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
            parent_id: None,
            children: vec![],
        }
    }

    #[test]
    fn merges_same_file_from_multiple_agents() {
        let mut skills = vec![
            make_skill(
                "claude-code:session-handoff",
                "C:/Users/test/.agents/skills/session-handoff/SKILL.md",
                AgentId::ClaudeCode,
            ),
            make_skill(
                "codex:session-handoff",
                "C:/Users/test/.agents/skills/session-handoff/SKILL.md",
                AgentId::Codex,
            ),
        ];

        dedupe_skills_by_source(&mut skills);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source_paths.len(), 1);
        assert_eq!(skills[0].source_agents.len(), 2);
        assert_eq!(skills[0].legacy_ids.len(), 2);
    }

    #[test]
    fn keeps_distinct_files() {
        let mut skills = vec![
            make_skill(
                "a",
                "C:/Users/test/.agents/skills/a/SKILL.md",
                AgentId::ClaudeCode,
            ),
            make_skill(
                "b",
                "C:/Users/test/.agents/skills/b/SKILL.md",
                AgentId::ClaudeCode,
            ),
        ];

        dedupe_skills_by_source(&mut skills);
        assert_eq!(skills.len(), 2);
    }

    #[test]
    fn keeps_distinct_hooks_in_same_file() {
        let mut first = make_skill(
            "claude-code:hook-settings-pretool-1",
            "C:/Users/test/.claude/settings.json",
            AgentId::ClaudeCode,
        );
        first.artifact_type = ArtifactType::Hook;
        first.metadata.hook_event = Some("PreToolUse".to_string());
        first.metadata.hook_command = Some("echo first".to_string());

        let mut second = make_skill(
            "claude-code:hook-settings-posttool-2",
            "C:/Users/test/.claude/settings.json",
            AgentId::ClaudeCode,
        );
        second.artifact_type = ArtifactType::Hook;
        second.metadata.hook_event = Some("PostToolUse".to_string());
        second.metadata.hook_command = Some("echo second".to_string());

        let mut skills = vec![first, second];
        dedupe_skills_by_source(&mut skills);
        assert_eq!(skills.len(), 2);
    }

    #[test]
    fn merges_identical_content_across_different_locations() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path_a = temp.path().join("a").join("SKILL.md");
        let path_b = temp.path().join("b").join("SKILL.md");
        std::fs::create_dir_all(path_a.parent().expect("a parent")).expect("mkdir a");
        std::fs::create_dir_all(path_b.parent().expect("b parent")).expect("mkdir b");
        std::fs::write(&path_a, "# same\nline").expect("write a");
        std::fs::write(&path_b, "# same\nline").expect("write b");

        let mut skills = vec![
            make_skill(
                "claude-code:same",
                path_a.to_string_lossy().as_ref(),
                AgentId::ClaudeCode,
            ),
            make_skill(
                "codex:same",
                path_b.to_string_lossy().as_ref(),
                AgentId::Codex,
            ),
        ];

        dedupe_skills_by_source(&mut skills);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source_paths.len(), 2);
    }
}
