// @agent-context: The universal Skill model.
// This is the SINGLE representation that ALL agent adapters produce.
// Maps SKILL.md, .mdc, .instructions.md, .clinerules, etc. into one shape.
// The frontend only knows about this model — never agent-specific formats.

use serde::{Deserialize, Serialize};

/// Universal skill representation across all coding agents.
/// Every adapter converts its native format into this struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Unique identifier: "{agent_id}:{relative_path_hash}"
    pub id: String,

    /// Display name from frontmatter `name` field, or derived from filename
    pub name: String,

    /// What the skill does — from frontmatter `description` or first paragraph
    pub description: String,

    /// Which coding agent owns this skill
    pub agent_id: AgentId,

    /// Absolute path to the skill file on disk
    pub file_path: String,

    /// Whether this is a global (user-level) or project-scoped skill
    pub scope: SkillScope,

    /// For project-scoped skills: which project directory they belong to
    pub project_path: Option<String>,

    /// YAML frontmatter fields preserved as-is for display
    pub metadata: SkillMetadata,

    /// Normalized discovery tags used by faceted filtering in the UI
    #[serde(default)]
    pub discovery_tags: Vec<String>,

    /// Normalized use-case labels that answer when this skill should be used
    #[serde(default)]
    pub use_cases: Vec<String>,

    /// Short provenance hints that explain why this skill was tagged
    #[serde(default)]
    pub discovery_hints: Vec<String>,

    /// User-assigned icon: emoji string, or path to an image in assets/
    pub icon: Option<String>,

    /// Whether the user has starred/favorited this skill
    pub starred: bool,

    /// Whether an update is available (for git-sourced skills)
    pub update_available: bool,

    /// Parent skill ID (for sub-skills in a hierarchy)
    pub parent_id: Option<String>,

    /// Child skills (populated during tree-building phase of scan)
    pub children: Vec<Skill>,
}

/// Identifies which coding agent a skill belongs to.
/// Each variant maps to an adapter in the `agents` module.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    Cursor,
    GithubCopilot,
    Codex,
    Windsurf,
    #[serde(rename = "gemini-cli")]
    Gemini,
    Cline,
    #[serde(rename = "roo")]
    RooCode,
    #[serde(rename = "continue")]
    ContinueDev,
    Aider,
    AmazonQ,
    JetbrainsAi,
    #[serde(rename = "tabnine-cli")]
    Tabnine,
    #[serde(rename = "augment")]
    AugmentCode,
    #[serde(rename = "openhands")]
    OpenHands,
    Devin,
    #[serde(rename = "aider-desk")]
    AiderDesk,
    Bob,
    #[serde(rename = "opencode")]
    OpenCode,
    Crush,
    Amp,
    Antigravity,
    #[serde(rename = "openclaw")]
    OpenClaw,
    #[serde(rename = "codearts-agent")]
    CodeArtsAgent,
    #[serde(rename = "codebuddy")]
    CodeBuddy,
    Codemaker,
    #[serde(rename = "codestudio")]
    CodeStudio,
    CommandCode,
    Cortex,
    #[serde(rename = "deepagents")]
    DeepAgents,
    Dexto,
    Droid,
    Firebender,
    #[serde(rename = "forgecode")]
    ForgeCode,
    Goose,
    Junie,
    #[serde(rename = "iflow-cli")]
    IFlowCli,
    Kilo,
    #[serde(rename = "kimi-cli")]
    KimiCli,
    #[serde(rename = "kiro-cli")]
    KiroCli,
    Kode,
    #[serde(rename = "mcpjam")]
    McpJam,
    MistralVibe,
    Mux,
    Neovate,
    Pi,
    Qoder,
    #[serde(rename = "qwen-code")]
    QwenCode,
    Replit,
    #[serde(rename = "rovodev")]
    RovoDev,
    Trae,
    #[serde(rename = "trae-cn")]
    TraeCn,
    Warp,
    Zencoder,
    Pochi,
    Adal,
    /// For AGENTS.md files (universal, not agent-specific)
    Universal,
    /// User-defined custom source
    Custom(String),
}

/// Where a skill lives in the hierarchy
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillScope {
    /// User-level: ~/.{agent}/skills/ — available everywhere
    Global,
    /// Project-level: .{agent}/skills/ — only in this project
    Project,
}

/// Extracted frontmatter metadata, normalized across formats.
/// Fields are all Optional because different agents expose different metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    /// Semantic version if available
    pub version: Option<String>,

    /// Author name
    pub author: Option<String>,

    /// Category tag (e.g., "framework", "testing", "deployment")
    pub category: Option<String>,

    /// Optional explicit tags from frontmatter (string or string[])
    pub tags: Option<Vec<String>>,

    /// Optional explicit use-case hints from frontmatter
    pub use_cases: Option<Vec<String>>,

    /// File glob patterns for auto-activation (Cursor globs, Copilot applyTo)
    pub globs: Option<Vec<String>>,

    /// Activation mode: "always", "manual", "auto", "agent-decision"
    pub trigger: Option<String>,

    /// Tool restrictions (Claude/Codex: allowed-tools)
    pub allowed_tools: Option<String>,

    /// Whether the skill can be invoked by typing /name
    pub user_invocable: Option<bool>,

    /// Language the skill targets
    pub language: Option<String>,

    /// Any extra frontmatter fields we didn't parse explicitly
    #[serde(default)]
    pub extra: Option<serde_json::Value>,

    /// Detected or user-set GitHub repository URL
    #[serde(default)]
    pub repository_url: Option<String>,

    /// Detected or user-set install command (e.g., `npx skills add owner/repo --skill name -g`)
    #[serde(default)]
    pub install_command: Option<String>,
}

/// Result of scanning a directory for skills
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub skills: Vec<Skill>,
    pub errors: Vec<ScanError>,
    pub scan_duration_ms: u64,
}

/// Non-fatal error during skill scanning (e.g., malformed frontmatter)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanError {
    pub file_path: String,
    pub message: String,
}
