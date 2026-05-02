// @agent-context: The agent registry — single source of truth for all supported coding agents.
// Each entry describes: name, paths, format, and brand color.
//
// PATH PLACEHOLDERS:
// - $HOME → user home directory (resolved at runtime)
// - $PROJECT → current project root (resolved from terminal CWD detection)
//
// FORMAT MAPPING:
// - SkillMd → parsed by parsers/skill_md.rs (richest metadata)
// - PlainMarkdown → parsed by parsers/frontmatter.rs (body-only, no structured fields)
// - RulesDir → each .md file in the directory is one "skill"
// - Mdc → Cursor's .mdc format (YAML frontmatter with description/globs/alwaysApply)
// - InstructionsMd → GitHub Copilot .instructions.md (YAML frontmatter with applyTo)

use crate::models::{AgentId, AgentInfo, SkillFormat};

/// Returns the complete list of all known coding agents and their configurations.
/// This is called once at app startup to build the scan plan.
pub fn get_agent_registry() -> Vec<AgentInfo> {
    vec![
        // ── Claude Code ──────────────────────────────────────────────
        AgentInfo {
            id: AgentId::ClaudeCode,
            display_name: "Claude Code".to_string(),
            description: "Anthropic's CLI coding agent".to_string(),
            color: "#f28c54".to_string(), // warm orange
            installed: false,             // resolved at scan time
            skill_count: 0,
            global_paths: vec![
                // @agent-context: ** glob matches sub-skills at any depth.
                // e.g. ~/.claude/skills/parent/SKILL.md AND
                //      ~/.claude/skills/parent/sub-skill/SKILL.md
                // Tree hierarchy is built by scanner.rs after scanning.
                "$HOME/.claude/skills/**/SKILL.md".to_string(),
                "$HOME/.claude/commands/*.md".to_string(),
                "$HOME/.agents/skills/**/SKILL.md".to_string(),
            ],
            project_paths: vec![
                "$PROJECT/.claude/skills/**/SKILL.md".to_string(),
                "$PROJECT/.claude/commands/*.md".to_string(),
                "$PROJECT/CLAUDE.md".to_string(),
            ],
            format: SkillFormat::SkillMd,
        },
        // ── OpenAI Codex ─────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Codex,
            display_name: "Codex".to_string(),
            description: "OpenAI's terminal coding agent".to_string(),
            color: "#4dc8b0".to_string(), // teal
            installed: false,
            skill_count: 0,
            global_paths: vec![
                "$HOME/.codex/skills/**/SKILL.md".to_string(),
                "$HOME/.codex/AGENTS.md".to_string(),
            ],
            project_paths: vec![
                "$PROJECT/.codex/skills/**/SKILL.md".to_string(),
                "$PROJECT/AGENTS.md".to_string(),
            ],
            format: SkillFormat::SkillMd,
        },
        // ── Cursor ───────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Cursor,
            display_name: "Cursor".to_string(),
            description: "AI-first code editor".to_string(),
            color: "#a58cff".to_string(), // purple
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.cursor/rules/*.mdc".to_string()],
            project_paths: vec![
                "$PROJECT/.cursor/rules/*.mdc".to_string(),
                "$PROJECT/.cursorrules".to_string(),
            ],
            format: SkillFormat::Mdc,
        },
        // ── GitHub Copilot ───────────────────────────────────────────
        AgentInfo {
            id: AgentId::GithubCopilot,
            display_name: "GitHub Copilot".to_string(),
            description: "GitHub's AI pair programmer".to_string(),
            color: "#57a0ff".to_string(), // blue
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec![
                "$PROJECT/.github/copilot-instructions.md".to_string(),
                "$PROJECT/.github/instructions/*.instructions.md".to_string(),
                "$PROJECT/.github/copilot/agents/*.md".to_string(),
            ],
            format: SkillFormat::InstructionsMd,
        },
        // ── Windsurf ─────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Windsurf,
            display_name: "Windsurf".to_string(),
            description: "Codeium's AI IDE".to_string(),
            color: "#49bed9".to_string(), // cyan
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.windsurf/global_rules.md".to_string()],
            project_paths: vec![
                "$PROJECT/.windsurf/rules/*.md".to_string(),
                "$PROJECT/.windsurfrules".to_string(),
            ],
            format: SkillFormat::RulesDir,
        },
        // ── Gemini CLI ───────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Gemini,
            display_name: "Gemini CLI".to_string(),
            description: "Google's terminal coding agent".to_string(),
            color: "#d5b347".to_string(), // yellow
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.gemini/GEMINI.md".to_string()],
            project_paths: vec![
                "$PROJECT/GEMINI.md".to_string(),
                "$PROJECT/.gemini/GEMINI.md".to_string(),
            ],
            format: SkillFormat::PlainMarkdown,
        },
        // ── Cline ────────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Cline,
            display_name: "Cline".to_string(),
            description: "VS Code autonomous coding agent".to_string(),
            color: "#d082d6".to_string(), // magenta
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec![
                "$PROJECT/.clinerules".to_string(),
                "$PROJECT/.clinerules/*.md".to_string(),
            ],
            format: SkillFormat::RulesDir,
        },
        // ── Roo Code ─────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::RooCode,
            display_name: "Roo Code".to_string(),
            description: "VS Code agent with mode-based rules".to_string(),
            color: "#cf7ca7".to_string(), // rose
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec![
                "$PROJECT/.roo/rules/*.md".to_string(),
                "$PROJECT/.roorules".to_string(),
            ],
            format: SkillFormat::RulesDir,
        },
        // ── Continue.dev ─────────────────────────────────────────────
        AgentInfo {
            id: AgentId::ContinueDev,
            display_name: "Continue".to_string(),
            description: "Open-source AI coding assistant".to_string(),
            color: "#5fb77b".to_string(), // green
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.continue/rules/*.md".to_string()],
            project_paths: vec!["$PROJECT/.continue/rules/*.md".to_string()],
            format: SkillFormat::RulesDir,
        },
        // ── Aider ────────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Aider,
            display_name: "Aider".to_string(),
            description: "Terminal pair programming with LLMs".to_string(),
            color: "#82b36d".to_string(), // lime
            installed: false,
            skill_count: 0,
            global_paths: vec!["$HOME/.aider.conf.yml".to_string()],
            project_paths: vec![
                "$PROJECT/.aider.conf.yml".to_string(),
                "$PROJECT/CONVENTIONS.md".to_string(),
            ],
            format: SkillFormat::Yaml,
        },
        // ── Amazon Q ─────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::AmazonQ,
            display_name: "Amazon Q".to_string(),
            description: "AWS AI developer assistant".to_string(),
            color: "#d68748".to_string(), // orange
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec!["$PROJECT/.amazonq/rules/*.md".to_string()],
            format: SkillFormat::RulesDir,
        },
        // ── JetBrains AI ─────────────────────────────────────────────
        AgentInfo {
            id: AgentId::JetbrainsAi,
            display_name: "JetBrains AI".to_string(),
            description: "JetBrains IDE AI assistant".to_string(),
            color: "#d46d95".to_string(), // red-pink
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec!["$PROJECT/.aiassistant/rules/*.md".to_string()],
            format: SkillFormat::RulesDir,
        },
        // ── Tabnine ──────────────────────────────────────────────────
        AgentInfo {
            id: AgentId::Tabnine,
            display_name: "Tabnine".to_string(),
            description: "AI code completion assistant".to_string(),
            color: "#6f88d9".to_string(), // indigo
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec!["$PROJECT/.tabnine/guidelines/*.md".to_string()],
            format: SkillFormat::RulesDir,
        },
        // ── Augment Code ─────────────────────────────────────────────
        AgentInfo {
            id: AgentId::AugmentCode,
            display_name: "Augment Code".to_string(),
            description: "AI coding assistant with deep context".to_string(),
            color: "#3fa89a".to_string(), // sea green
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec!["$PROJECT/.augment/rules/*.md".to_string()],
            format: SkillFormat::RulesDir,
        },
        // ── Universal (AGENTS.md) ────────────────────────────────────
        AgentInfo {
            id: AgentId::Universal,
            display_name: "AGENTS.md".to_string(),
            description: "Universal agent instructions (Linux Foundation standard)".to_string(),
            color: "#7a7fad".to_string(), // neutral blue-gray
            installed: false,
            skill_count: 0,
            global_paths: vec![],
            project_paths: vec![
                "$PROJECT/AGENTS.md".to_string(),
                "$PROJECT/AGENTS.override.md".to_string(),
            ],
            format: SkillFormat::PlainMarkdown,
        },
    ]
}
