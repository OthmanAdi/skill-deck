// @agent-context: Agent registry model.
// Each coding agent (Claude Code, Cursor, Copilot, etc.) is described by an AgentInfo.
// The registry auto-detects which agents are installed by checking for their directories.
// To add a new agent: add a variant to AgentId (in skill.rs) and an entry in agents/registry.rs.

use serde::{Deserialize, Serialize};

use super::AgentId;

/// Description of a coding agent and where it stores skills/rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Which agent this describes
    pub id: AgentId,

    /// Human-readable name (e.g., "Claude Code", "GitHub Copilot")
    pub display_name: String,

    /// Short description of the agent
    pub description: String,

    /// Brand color as CSS color string, recommended hex value (for agent badges in the UI)
    pub color: String,

    /// Whether this agent is detected as installed on this system
    pub installed: bool,

    /// Number of skills found for this agent
    pub skill_count: usize,

    /// Glob patterns for where this agent stores global skills/rules.
    /// Uses $HOME as a placeholder, resolved at runtime.
    /// Example: ["$HOME/.claude/skills/*/SKILL.md", "$HOME/.claude/commands/*.md"]
    pub global_paths: Vec<String>,

    /// Glob patterns for project-level skills/rules.
    /// Uses $PROJECT as a placeholder for the current project root.
    /// Example: ["$PROJECT/.claude/skills/*/SKILL.md"]
    pub project_paths: Vec<String>,

    /// The primary config file format this agent uses
    pub format: SkillFormat,
}

/// What file format an agent uses for its skills/rules
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillFormat {
    /// SKILL.md with YAML frontmatter (Claude Code, Codex)
    SkillMd,
    /// .mdc files with YAML frontmatter (Cursor)
    Mdc,
    /// .instructions.md with YAML frontmatter (GitHub Copilot)
    InstructionsMd,
    /// Plain markdown, single file (AGENTS.md, CLAUDE.md, GEMINI.md)
    PlainMarkdown,
    /// Markdown files in a rules directory (.windsurf/rules/, .roo/rules/)
    RulesDir,
    /// YAML config (Aider, OpenHands)
    Yaml,
    /// JSON config (Sourcegraph Cody)
    Json,
}
