use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

use crate::models::{AgentId, ArtifactType, Skill, SkillMetadata, SkillScope};

pub fn parse_claude_hooks(
    file_path: &Path,
    scope: SkillScope,
    project_path: Option<String>,
) -> Result<Vec<Skill>> {
    let content = std::fs::read_to_string(file_path)?;
    let parsed: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON in {}", file_path.to_string_lossy()))?;

    let Some(hooks_obj) = parsed.get("hooks").and_then(|v| v.as_object()) else {
        return Ok(Vec::new());
    };

    let mut items = Vec::new();
    let mut next_index = 1usize;

    for (event, blocks_value) in hooks_obj {
        let blocks = as_value_list(blocks_value);

        for (block_index, block) in blocks.iter().enumerate() {
            let matcher = block
                .get("matcher")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            let hook_entries = extract_hook_entries(block);

            for (entry_index, entry) in hook_entries.iter().enumerate() {
                let hook_type = entry
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("command")
                    .trim()
                    .to_string();

                let command = entry
                    .get("command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let title = command
                    .as_deref()
                    .map(first_line)
                    .filter(|line| !line.is_empty())
                    .map(|line| format!("{} hook: {}", event, truncate(line, 68)))
                    .unwrap_or_else(|| format!("{} hook", event));

                let matcher_label = matcher.clone().unwrap_or_else(|| "all tools".to_string());
                let description = if hook_type == "command" {
                    format!(
                        "Claude hook for {} with matcher {}, runs a local command.",
                        event, matcher_label
                    )
                } else {
                    format!(
                        "Claude hook for {} with matcher {}, hook type {}.",
                        event, matcher_label, hook_type
                    )
                };

                let metadata = SkillMetadata {
                    version: None,
                    author: None,
                    category: Some("hooks".to_string()),
                    tags: Some(vec!["hooks".to_string(), "automation".to_string()]),
                    use_cases: Some(vec!["auto-run".to_string()]),
                    globs: None,
                    trigger: Some("auto".to_string()),
                    allowed_tools: matcher.clone(),
                    user_invocable: Some(false),
                    language: None,
                    slash_command: None,
                    hook_event: Some(event.to_string()),
                    hook_matcher: matcher.clone(),
                    hook_command: command,
                    extra: Some(json!({
                        "source": file_path.to_string_lossy().to_string(),
                        "hookType": hook_type,
                        "hookBlockIndex": block_index,
                        "hookEntryIndex": entry_index,
                    })),
                    repository_url: None,
                    install_command: None,
                };

                let id = format!(
                    "claude-code:hook-{}-{}-{}",
                    slugify(&file_stem(file_path)),
                    slugify(event),
                    next_index
                );
                next_index += 1;

                items.push(Skill {
                    id,
                    name: title,
                    description,
                    artifact_type: ArtifactType::Hook,
                    agent_id: AgentId::ClaudeCode,
                    source_agents: vec![AgentId::ClaudeCode],
                    file_path: file_path.to_string_lossy().to_string(),
                    source_paths: vec![file_path.to_string_lossy().to_string()],
                    legacy_ids: vec![],
                    scope: scope.clone(),
                    project_path: project_path.clone(),
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
                });
            }
        }
    }

    Ok(items)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|v| v.to_str())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "settings".to_string())
}

fn as_value_list(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        _ => vec![value],
    }
}

fn extract_hook_entries(block: &Value) -> Vec<&Value> {
    if let Some(entries) = block.get("hooks") {
        return as_value_list(entries);
    }

    if block.get("type").is_some() || block.get("command").is_some() {
        return vec![block];
    }

    Vec::new()
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or("").trim()
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }

    let mut out = value.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

fn slugify(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::parse_claude_hooks;
    use crate::models::SkillScope;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parses_command_hooks_from_claude_settings() {
        let mut file = NamedTempFile::new().expect("temp file");
        writeln!(
            file,
            "{{\n  \"hooks\": {{\n    \"PreToolUse\": [\n      {{\n        \"matcher\": \"Bash\",\n        \"hooks\": [{{ \"type\": \"command\", \"command\": \"echo hi\" }}]\n      }}\n    ]\n  }}\n}}"
        )
        .expect("write settings");

        let parsed = parse_claude_hooks(file.path(), SkillScope::Global, None).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].artifact_type, crate::models::ArtifactType::Hook);
        assert_eq!(parsed[0].metadata.hook_event.as_deref(), Some("PreToolUse"));
        assert_eq!(parsed[0].metadata.hook_command.as_deref(), Some("echo hi"));
    }

    #[test]
    fn returns_empty_when_hooks_missing() {
        let mut file = NamedTempFile::new().expect("temp file");
        writeln!(file, "{{\"env\":{{\"A\":\"B\"}}}}",).expect("write settings");

        let parsed = parse_claude_hooks(file.path(), SkillScope::Global, None).expect("parse");
        assert_eq!(parsed.len(), 0);
    }
}
