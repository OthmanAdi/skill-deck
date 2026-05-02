// @agent-context: Tauri commands for drag-and-drop terminal injection.
// Exposes window-at-cursor detection and content injection to the frontend.

use crate::detection::terminal_inject::{self, InjectionResult};
use crate::detection::window_at_point::{self, WindowAtPoint};
use crate::models::{AgentId, Skill, SkillScope};
use std::path::{Path, PathBuf};
use tauri::State;

use super::preferences::ConfigState;
use super::skills::{resolve_project_root, scan_with_config};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillReference {
    pub text: String,
    pub kind: String,
    pub available_in_target: bool,
}

/// Get information about the window currently under the mouse cursor.
/// Called during drag to determine if the user is hovering over a terminal.
#[tauri::command]
pub fn get_window_at_cursor() -> WindowAtPoint {
    window_at_point::get_window_at_cursor()
}

/// Inject text content into a terminal window identified by PID.
/// Called when the user drops a skill card onto a terminal.
#[tauri::command]
pub fn inject_to_terminal(content: String, target_pid: u32) -> InjectionResult {
    terminal_inject::inject_to_terminal(&content, target_pid)
}

/// Resolve the exact text Skill Deck should paste for this skill in this project.
#[tauri::command]
pub fn resolve_skill_reference(
    state: State<'_, ConfigState>,
    skill_id: String,
    project_path: Option<String>,
) -> Result<SkillReference, String> {
    let config = state
        .0
        .lock()
        .map_err(|_| "Failed to lock config state".to_string())?
        .clone();
    let project_root = resolve_project_root(project_path.as_deref());
    let scan = scan_with_config(&config, project_root.as_deref());
    let skill = scan
        .skills
        .into_iter()
        .find(|s| s.id == skill_id)
        .ok_or_else(|| "Skill not found in allowed scan scope".to_string())?;

    Ok(build_skill_reference(&skill, project_root.as_deref()))
}

/// Inject a scanned skill into a terminal. The frontend supplies only a skill ID.
/// Rust derives slash-command vs file-path text after validating the target terminal.
#[tauri::command]
pub fn inject_skill_to_terminal(
    state: State<'_, ConfigState>,
    skill_id: String,
    project_path: Option<String>,
    target_pid: u32,
) -> InjectionResult {
    let config = match state.0.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            return InjectionResult {
                success: false,
                error: Some("Failed to lock config state".to_string()),
                reference: None,
                reference_kind: None,
            }
        }
    };

    let target_context = super::context::detect_terminal_context_for_pid(target_pid);
    let target_root = resolve_project_root(target_context.cwd.as_deref())
        .or_else(|| resolve_project_root(project_path.as_deref()));
    let source_root = resolve_project_root(project_path.as_deref());

    let skill = find_skill_for_reference(&config, &skill_id, target_root.as_deref())
        .or_else(|| find_skill_for_reference(&config, &skill_id, source_root.as_deref()));

    let Some(skill) = skill else {
        return InjectionResult {
            success: false,
            error: Some("Skill not found in allowed scan scope".to_string()),
            reference: None,
            reference_kind: None,
        };
    };

    let reference = build_skill_reference(&skill, target_root.as_deref());
    let mut result = terminal_inject::inject_to_terminal(&reference.text, target_pid);
    result.reference = Some(reference.text);
    result.reference_kind = Some(reference.kind);
    result
}

fn find_skill_for_reference(
    config: &crate::models::AppConfig,
    skill_id: &str,
    project_root: Option<&Path>,
) -> Option<Skill> {
    scan_with_config(config, project_root)
        .skills
        .into_iter()
        .find(|s| s.id == skill_id)
}

fn build_skill_reference(skill: &Skill, target_project_root: Option<&Path>) -> SkillReference {
    let available_in_target = is_available_in_target(skill, target_project_root);

    if available_in_target {
        if let Some(command) = slash_command_for_skill(skill) {
            return SkillReference {
                text: command,
                kind: "slash-command".to_string(),
                available_in_target,
            };
        }
    }

    SkillReference {
        text: quote_path_for_terminal(&skill.file_path),
        kind: "file-path".to_string(),
        available_in_target,
    }
}

fn is_available_in_target(skill: &Skill, target_project_root: Option<&Path>) -> bool {
    if skill.scope == SkillScope::Global {
        return true;
    }

    let Some(target) = target_project_root.and_then(canonical_path) else {
        return false;
    };

    if let Some(project_path) = skill
        .project_path
        .as_deref()
        .and_then(|p| canonical_path(Path::new(p)))
    {
        return project_path == target;
    }

    canonical_path(Path::new(&skill.file_path))
        .map(|p| p.starts_with(&target))
        .unwrap_or(false)
}

fn canonical_path(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn slash_command_for_skill(skill: &Skill) -> Option<String> {
    if skill.agent_id != AgentId::ClaudeCode {
        return None;
    }

    let path = Path::new(&skill.file_path);
    let normalized = skill.file_path.replace('\\', "/").to_lowercase();
    if normalized.contains("/.claude/commands/") {
        return path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(slashify_command_name);
    }

    if skill.metadata.user_invocable == Some(true) {
        return slashify_command_name(&skill.name);
    }

    None
}

fn slashify_command_name(name: &str) -> Option<String> {
    let mut out = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_whitespace() && !out.ends_with('-') {
            out.push('-');
        }
    }

    let out = out.trim_matches('-');
    if out.is_empty() {
        None
    } else {
        Some(format!("/{}", out))
    }
}

fn quote_path_for_terminal(path: &str) -> String {
    format!("\"{}\"", path.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SkillMetadata, SkillScope};

    fn test_skill(path: &str) -> Skill {
        Skill {
            id: "claude-code:test".to_string(),
            name: "Test Skill".to_string(),
            description: String::new(),
            agent_id: AgentId::ClaudeCode,
            file_path: path.to_string(),
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
    fn claude_command_file_uses_file_stem_slash_command() {
        let skill = test_skill("C:/Users/me/.claude/commands/review-code.md");
        assert_eq!(
            slash_command_for_skill(&skill).as_deref(),
            Some("/review-code")
        );
    }

    #[test]
    fn non_command_path_falls_back_to_quoted_path() {
        let skill = test_skill("C:/Users/me/My Skills/demo/SKILL.md");
        let reference = build_skill_reference(&skill, None);
        assert_eq!(reference.kind, "file-path");
        assert_eq!(reference.text, "\"C:/Users/me/My Skills/demo/SKILL.md\"");
    }

    #[test]
    fn user_invocable_skill_uses_sanitized_name() {
        let mut skill = test_skill("C:/Users/me/.claude/skills/demo/SKILL.md");
        skill.name = "Review Code".to_string();
        skill.metadata.user_invocable = Some(true);
        assert_eq!(
            slash_command_for_skill(&skill).as_deref(),
            Some("/review-code")
        );
    }
}
