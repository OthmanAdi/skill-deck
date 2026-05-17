// @agent-context: SKILL.md parser (Agent Skills spec from agentskills.io).
// Used by: Claude Code, OpenAI Codex (identical format).
// This is the richest metadata format — all other adapters produce a subset.
//
// SKILL.md format:
// ```
// ---
// name: my-skill
// description: What it does
// version: "1.0"
// allowed-tools: "Read Grep Bash"
// ...
// ---
// [Markdown body with instructions]
// ```

use anyhow::Result;
use std::path::Path;

use super::frontmatter::{
    parse_frontmatter, yaml_bool, yaml_str, yaml_string_array, yaml_string_list,
};
use crate::models::{AgentId, ArtifactType, Skill, SkillMetadata, SkillScope};

/// Parse a SKILL.md file into a universal Skill struct.
///
/// `agent_id`: which agent owns this file (ClaudeCode or Codex)
/// `file_path`: absolute path to the SKILL.md file
/// `scope`: Global or Project
/// `project_path`: if Project scope, the project root directory
pub fn parse_skill_md(
    agent_id: AgentId,
    file_path: &Path,
    scope: SkillScope,
    project_path: Option<String>,
) -> Result<Skill> {
    let content = std::fs::read_to_string(file_path)?;
    let parsed = parse_frontmatter(&content)?;

    // Derive name from frontmatter, or from parent directory name
    let dir_name = file_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let (name, description, metadata) = match &parsed.frontmatter {
        Some(fm) => {
            let name = yaml_str(fm, "name").unwrap_or_else(|| dir_name.clone());
            let description = yaml_str(fm, "description").unwrap_or_default();
            let metadata = SkillMetadata {
                version: yaml_str(fm, "version"),
                author: yaml_str(fm, "author")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "author"))),
                category: yaml_str(fm, "category")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_str(m, "category"))),
                tags: yaml_string_list(fm, "tags")
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_string_list(m, "tags"))),
                use_cases: yaml_string_list(fm, "use-cases")
                    .or_else(|| yaml_string_list(fm, "use_cases"))
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_string_list(m, "use-cases")))
                    .or_else(|| fm.get("metadata").and_then(|m| yaml_string_list(m, "use_cases"))),
                globs: yaml_string_array(fm, "paths"),
                trigger: determine_trigger(fm),
                allowed_tools: yaml_str(fm, "allowed-tools"),
                user_invocable: yaml_bool(fm, "user-invocable"),
                language: fm.get("metadata").and_then(|m| yaml_str(m, "language")),
                slash_command: yaml_bool(fm, "user-invocable")
                    .and_then(|invocable| invocable.then(|| yaml_str(fm, "name")))
                    .flatten()
                    .map(|name| slugify(&name))
                    .filter(|slug| !slug.is_empty())
                    .map(|slug| format!("/{}", slug)),
                hook_event: None,
                hook_matcher: None,
                hook_command: None,
                extra: serde_json::to_value(fm).ok(),
                repository_url: None, // populated by repo_detector after parsing
                install_command: None, // populated by repo_detector after parsing
            };
            (name, description, metadata)
        }
        None => {
            // No frontmatter: derive name from directory, first line as description
            let first_line = parsed
                .body
                .lines()
                .next()
                .unwrap_or("")
                .trim_start_matches('#')
                .trim();
            (
                dir_name.clone(),
                first_line.to_string(),
                SkillMetadata::default(),
            )
        }
    };

    // Generate stable ID from agent + path
    let id = format!(
        "{}:{}",
        serde_json::to_string(&agent_id)?.trim_matches('"'),
        dir_name
    );

    Ok(Skill {
        id,
        name,
        description,
        artifact_type: ArtifactType::Skill,
        agent_id: agent_id.clone(),
        source_agents: vec![agent_id],
        file_path: file_path.to_string_lossy().to_string(),
        source_paths: vec![file_path.to_string_lossy().to_string()],
        legacy_ids: vec![],
        scope,
        project_path,
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

fn slugify(value: &str) -> String {
    let mut slug = value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' { ch } else { '-' })
        .collect::<String>();

    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }

    slug.trim_matches('-').to_string()
}

/// Determine the activation trigger from frontmatter fields.
/// Maps different agent conventions to a normalized trigger string.
fn determine_trigger(fm: &serde_yaml::Value) -> Option<String> {
    // Claude/Codex: disable-model-invocation + user-invocable
    if yaml_bool(fm, "disable-model-invocation") == Some(true) {
        return Some("manual".to_string());
    }
    if yaml_bool(fm, "user-invocable") == Some(false) {
        return Some("auto".to_string());
    }
    // Cursor/Windsurf: alwaysApply
    if yaml_bool(fm, "alwaysApply") == Some(true) {
        return Some("always".to_string());
    }
    // Windsurf: trigger field
    if let Some(trigger) = yaml_str(fm, "trigger") {
        return Some(trigger);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp_skill(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_parse_full_skill_md() {
        let content = r#"---
name: test-skill
description: A test skill for unit testing
version: "1.0.0"
allowed-tools: "Read Grep"
---
# Instructions
Do the thing.
"#;
        let file = write_temp_skill(content);
        let skill =
            parse_skill_md(AgentId::ClaudeCode, file.path(), SkillScope::Global, None).unwrap();

        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.description, "A test skill for unit testing");
        assert_eq!(skill.metadata.version.as_deref(), Some("1.0.0"));
        assert_eq!(skill.metadata.allowed_tools.as_deref(), Some("Read Grep"));
    }

    #[test]
    fn test_parse_minimal_skill_md() {
        let content = "# Simple skill\nJust a markdown file with no frontmatter.";
        let file = write_temp_skill(content);
        let skill = parse_skill_md(
            AgentId::Codex,
            file.path(),
            SkillScope::Project,
            Some("/project".to_string()),
        )
        .unwrap();

        assert_eq!(skill.description, "Simple skill");
        assert_eq!(skill.scope, SkillScope::Project);
    }
}
