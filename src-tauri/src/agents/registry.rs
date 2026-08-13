// @agent-context: Versioned, data-only coding-agent catalog.
//
// The upstream portion is pinned to an immutable vercel-labs/skills revision.
// Skill Deck adds three retained legacy adapters after the pinned 75 entries.
// Location templates are resolved locally by locations.rs; no scan path is
// downloaded or inferred through a broad disk crawl.

use super::locations::LocationEnvironment;
use crate::models::{AgentId, AgentInfo, SkillFormat};

pub const AGENT_CATALOG_VERSION: &str = "2026-07-29.1";
pub const AGENT_CATALOG_SOURCE: &str =
    "https://github.com/vercel-labs/skills/blob/e173b8c88f2581cfdaa1b6767c6519a08155790e/src/agents.ts";
pub const AGENT_CATALOG_UPSTREAM_COMMIT: &str = "e173b8c88f2581cfdaa1b6767c6519a08155790e";
pub const PINNED_UPSTREAM_AGENT_COUNT: usize = 75;
pub const RETAINED_LEGACY_AGENT_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogOrigin {
    PinnedUpstream,
    RetainedLegacy,
}

#[derive(Debug, Clone)]
struct CatalogEntry {
    id: AgentId,
    display_name: &'static str,
    global_paths: &'static [&'static str],
    detection_paths: &'static [&'static str],
    project_paths: &'static [&'static str],
    format: SkillFormat,
    origin: CatalogOrigin,
}

const fn upstream(
    id: AgentId,
    display_name: &'static str,
    global_paths: &'static [&'static str],
    detection_paths: &'static [&'static str],
    project_paths: &'static [&'static str],
) -> CatalogEntry {
    CatalogEntry {
        id,
        display_name,
        global_paths,
        detection_paths,
        project_paths,
        format: SkillFormat::SkillMd,
        origin: CatalogOrigin::PinnedUpstream,
    }
}

const fn legacy(
    id: AgentId,
    display_name: &'static str,
    global_paths: &'static [&'static str],
    detection_paths: &'static [&'static str],
    project_paths: &'static [&'static str],
    format: SkillFormat,
) -> CatalogEntry {
    CatalogEntry {
        id,
        display_name,
        global_paths,
        detection_paths,
        project_paths,
        format,
        origin: CatalogOrigin::RetainedLegacy,
    }
}

// Keep this order aligned with the pinned upstream source. Shared
// `.agents/skills` stores intentionally appear only on the Universal entry;
// agents that consume the store retain independent detection probes.
const CATALOG: &[CatalogEntry] = &[
    upstream(
        AgentId::AiderDesk,
        "AiderDesk",
        &["$HOME/.aider-desk/skills/**/SKILL.md"],
        &["$HOME/.aider-desk"],
        &["$PROJECT/.aider-desk/skills/**/SKILL.md"],
    ),
    upstream(AgentId::Amp, "Amp", &[], &["$CONFIG_HOME/amp"], &[]),
    upstream(
        AgentId::Antigravity,
        "Antigravity",
        &["$HOME/.gemini/antigravity/skills/**/SKILL.md"],
        &["$HOME/.gemini/antigravity"],
        &[],
    ),
    upstream(
        AgentId::AntigravityCli,
        "Antigravity CLI",
        &["$HOME/.gemini/antigravity-cli/skills/**/SKILL.md"],
        &["$HOME/.gemini/antigravity-cli"],
        &[],
    ),
    upstream(
        AgentId::AstrBot,
        "AstrBot",
        &["$HOME/.astrbot/data/skills/**/SKILL.md"],
        &["$HOME/.astrbot"],
        &["$PROJECT/data/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::AutohandCode,
        "Autohand Code CLI",
        &["$AUTOHAND_HOME/skills/**/SKILL.md"],
        &["$AUTOHAND_HOME"],
        &["$PROJECT/.autohand/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::AugmentCode,
        "Augment",
        &["$HOME/.augment/skills/**/SKILL.md"],
        &["$HOME/.augment"],
        &[
            "$PROJECT/.augment/skills/**/SKILL.md",
            "$PROJECT/.augment/rules/*.md",
        ],
    ),
    upstream(
        AgentId::Bob,
        "IBM Bob",
        &["$HOME/.bob/skills/**/SKILL.md"],
        &["$HOME/.bob"],
        &["$PROJECT/.bob/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::ClaudeCode,
        "Claude Code",
        &[
            "$CLAUDE_HOME/skills/**/SKILL.md",
            "$CLAUDE_HOME/commands/*.md",
            "$CLAUDE_HOME/settings.json",
            "$CLAUDE_HOME/settings.local.json",
        ],
        &["$CLAUDE_HOME"],
        &[
            "$PROJECT/.claude/skills/**/SKILL.md",
            "$PROJECT/.claude/commands/*.md",
            "$PROJECT/.claude/settings.json",
            "$PROJECT/.claude/settings.local.json",
            "$PROJECT/CLAUDE.md",
        ],
    ),
    upstream(
        AgentId::OpenClaw,
        "OpenClaw",
        &[
            "$HOME/.openclaw/skills/**/SKILL.md",
            "$HOME/.clawdbot/skills/**/SKILL.md",
            "$HOME/.moltbot/skills/**/SKILL.md",
        ],
        &["$HOME/.openclaw", "$HOME/.clawdbot", "$HOME/.moltbot"],
        &["$PROJECT/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Cline,
        "Cline",
        &["$HOME/.cline/skills/**/SKILL.md"],
        &[
            "$HOME/.cline",
            "$CLINE_DATA_DIR",
            "$VSCODE_GLOBAL_STORAGE/saoudrizwan.claude-dev",
        ],
        &["$PROJECT/.clinerules", "$PROJECT/.clinerules/*.md"],
    ),
    upstream(
        AgentId::CodeArtsAgent,
        "CodeArts Agent",
        &["$HOME/.codeartsdoer/skills/**/SKILL.md"],
        &["$HOME/.codeartsdoer"],
        &["$PROJECT/.codeartsdoer/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::CodeBuddy,
        "CodeBuddy",
        &["$HOME/.codebuddy/skills/**/SKILL.md"],
        &["$HOME/.codebuddy"],
        &["$PROJECT/.codebuddy/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Codemaker,
        "Codemaker",
        &["$HOME/.codemaker/skills/**/SKILL.md"],
        &["$HOME/.codemaker"],
        &["$PROJECT/.codemaker/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::CodeStudio,
        "Code Studio",
        &["$HOME/.codestudio/skills/**/SKILL.md"],
        &["$HOME/.codestudio"],
        &["$PROJECT/.codestudio/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Codex,
        "Codex",
        &["$CODEX_HOME/skills/**/SKILL.md", "$CODEX_HOME/AGENTS.md"],
        &["$CODEX_HOME", "/etc/codex"],
        &["$PROJECT/.codex/skills/**/SKILL.md", "$PROJECT/AGENTS.md"],
    ),
    upstream(
        AgentId::CommandCode,
        "Command Code",
        &["$HOME/.commandcode/skills/**/SKILL.md"],
        &["$HOME/.commandcode"],
        &["$PROJECT/.commandcode/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::ContinueDev,
        "Continue",
        &["$CONTINUE_HOME/skills/**/SKILL.md"],
        &["$CONTINUE_HOME"],
        &["$PROJECT/.continue/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Cortex,
        "Cortex Code",
        &["$HOME/.snowflake/cortex/skills/**/SKILL.md"],
        &["$HOME/.snowflake/cortex"],
        &["$PROJECT/.cortex/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Crush,
        "Crush",
        &["$CONFIG_HOME/crush/skills/**/SKILL.md"],
        &["$CONFIG_HOME/crush"],
        &["$PROJECT/.crush/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Cursor,
        "Cursor",
        &[
            "$HOME/.cursor/skills/**/SKILL.md",
            "$HOME/.cursor/rules/*.mdc",
        ],
        &["$HOME/.cursor"],
        &[
            "$PROJECT/.cursor/skills/**/SKILL.md",
            "$PROJECT/.cursor/rules/*.mdc",
            "$PROJECT/.cursorrules",
        ],
    ),
    upstream(
        AgentId::DeepAgents,
        "Deep Agents",
        &["$HOME/.deepagents/agent/skills/**/SKILL.md"],
        &["$HOME/.deepagents"],
        &[],
    ),
    upstream(
        AgentId::Devin,
        "Devin for Terminal",
        &["$CONFIG_HOME/devin/skills/**/SKILL.md"],
        &["$CONFIG_HOME/devin"],
        &["$PROJECT/.devin/skills/**/SKILL.md"],
    ),
    upstream(AgentId::Dexto, "Dexto", &[], &["$HOME/.dexto"], &[]),
    upstream(
        AgentId::Droid,
        "Droid",
        &["$HOME/.factory/skills/**/SKILL.md"],
        &["$HOME/.factory"],
        &["$PROJECT/.factory/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Eve,
        "Eve",
        &[],
        &[],
        &[
            "$PROJECT/agent/skills/**/SKILL.md",
            "$PROJECT/agent/subagents/*/skills/**/SKILL.md",
        ],
    ),
    upstream(
        AgentId::Firebender,
        "Firebender",
        &["$HOME/.firebender/skills/**/SKILL.md"],
        &["$HOME/.firebender"],
        &[],
    ),
    upstream(
        AgentId::ForgeCode,
        "ForgeCode",
        &["$HOME/.forge/skills/**/SKILL.md"],
        &["$HOME/.forge"],
        &["$PROJECT/.forge/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Gemini,
        "Gemini CLI",
        &["$GEMINI_HOME/skills/**/SKILL.md", "$GEMINI_HOME/GEMINI.md"],
        &["$GEMINI_HOME"],
        &[
            "$PROJECT/.gemini/skills/**/SKILL.md",
            "$PROJECT/.gemini/GEMINI.md",
            "$PROJECT/GEMINI.md",
        ],
    ),
    upstream(
        AgentId::GithubCopilot,
        "GitHub Copilot",
        &[
            "$COPILOT_HOME/skills/**/SKILL.md",
            "$COPILOT_HOME/skills/**/*.instructions.md",
            "$COPILOT_SKILLS_DIRS/**/SKILL.md",
        ],
        &["$COPILOT_HOME"],
        &[
            "$PROJECT/.github/copilot-instructions.md",
            "$PROJECT/.github/instructions/*.instructions.md",
            "$PROJECT/.github/copilot/agents/*.md",
        ],
    ),
    upstream(
        AgentId::Goose,
        "Goose",
        &["$CONFIG_HOME/goose/skills/**/SKILL.md"],
        &["$CONFIG_HOME/goose"],
        &["$PROJECT/.goose/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Grok,
        "Grok Build",
        &["$GROK_HOME/skills/**/SKILL.md"],
        &["$GROK_HOME"],
        &["$PROJECT/.grok/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::HermesAgent,
        "Hermes Agent",
        &["$HERMES_HOME/skills/**/SKILL.md"],
        &["$HERMES_HOME"],
        &["$PROJECT/.hermes/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::InferenceSh,
        "inference.sh",
        &["$HOME/.inferencesh/skills/**/SKILL.md"],
        &["$HOME/.inferencesh"],
        &["$PROJECT/.inferencesh/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Jazz,
        "Jazz",
        &["$HOME/.jazz/skills/**/SKILL.md"],
        &["$HOME/.jazz"],
        &["$PROJECT/.jazz/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Junie,
        "Junie",
        &["$HOME/.junie/skills/**/SKILL.md"],
        &["$HOME/.junie"],
        &["$PROJECT/.junie/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::IFlowCli,
        "iFlow CLI",
        &["$HOME/.iflow/skills/**/SKILL.md"],
        &["$HOME/.iflow"],
        &["$PROJECT/.iflow/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Kilo,
        "Kilo Code",
        &["$HOME/.kilocode/skills/**/SKILL.md"],
        &[
            "$HOME/.kilocode",
            "$XDG_DATA_HOME/kilo",
            "$VSCODE_GLOBAL_STORAGE/kilocode.kilo-code",
        ],
        &["$PROJECT/.kilocode/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Kimchi,
        "Kimchi",
        &["$CONFIG_HOME/kimchi/harness/skills/**/SKILL.md"],
        &["$CONFIG_HOME/kimchi"],
        &["$PROJECT/.kimchi/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::KimiCodeCli,
        "Kimi Code CLI",
        &[],
        &["$HOME/.kimi-code", "$HOME/.kimi"],
        &[],
    ),
    upstream(
        AgentId::KiroCli,
        "Kiro CLI",
        &["$HOME/.kiro/skills/**/SKILL.md"],
        &["$HOME/.kiro"],
        &["$PROJECT/.kiro/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Kode,
        "Kode",
        &["$HOME/.kode/skills/**/SKILL.md"],
        &["$HOME/.kode"],
        &["$PROJECT/.kode/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Lingma,
        "Lingma",
        &["$HOME/.lingma/skills/**/SKILL.md"],
        &["$HOME/.lingma"],
        &["$PROJECT/.lingma/skills/**/SKILL.md"],
    ),
    upstream(AgentId::Loaf, "Loaf", &[], &["$HOME/.loaf"], &[]),
    upstream(
        AgentId::McpJam,
        "MCPJam",
        &["$HOME/.mcpjam/skills/**/SKILL.md"],
        &["$HOME/.mcpjam"],
        &["$PROJECT/.mcpjam/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::MistralVibe,
        "Mistral Vibe",
        &["$VIBE_HOME/skills/**/SKILL.md"],
        &["$VIBE_HOME"],
        &["$PROJECT/.vibe/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Moxby,
        "Moxby",
        &["$HOME/.moxby/skills/**/SKILL.md"],
        &["$HOME/.moxby"],
        &["$PROJECT/.moxby/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Mux,
        "Mux",
        &["$HOME/.mux/skills/**/SKILL.md"],
        &["$HOME/.mux"],
        &["$PROJECT/.mux/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::OpenCode,
        "OpenCode",
        &["$CONFIG_HOME/opencode/skills/**/SKILL.md"],
        &["$CONFIG_HOME/opencode", "$XDG_DATA_HOME/opencode"],
        &["$PROJECT/.opencode/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::OpenHands,
        "OpenHands",
        &["$HOME/.openhands/skills/**/SKILL.md"],
        &["$HOME/.openhands"],
        &["$PROJECT/.openhands/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Ona,
        "Ona",
        &["$HOME/.ona/skills/**/SKILL.md"],
        &["$HOME/.ona"],
        &["$PROJECT/.ona/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Pi,
        "Pi",
        &["$HOME/.pi/agent/skills/**/SKILL.md"],
        &["$HOME/.pi/agent"],
        &["$PROJECT/.pi/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Qoder,
        "Qoder",
        &["$HOME/.qoder/skills/**/SKILL.md"],
        &["$HOME/.qoder"],
        &["$PROJECT/.qoder/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::QoderCn,
        "Qoder CN",
        &["$HOME/.qoder-cn/skills/**/SKILL.md"],
        &["$HOME/.qoder-cn"],
        &["$PROJECT/.qoder/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::QwenCode,
        "Qwen Code",
        &["$HOME/.qwen/skills/**/SKILL.md"],
        &["$HOME/.qwen"],
        &["$PROJECT/.qwen/skills/**/SKILL.md"],
    ),
    upstream(AgentId::Replit, "Replit", &[], &[], &[]),
    upstream(
        AgentId::Reasonix,
        "Reasonix",
        &["$HOME/.reasonix/skills/**/SKILL.md"],
        &["$HOME/.reasonix"],
        &["$PROJECT/.reasonix/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::RovoDev,
        "Rovo Dev",
        &["$HOME/.rovodev/skills/**/SKILL.md"],
        &["$HOME/.rovodev"],
        &["$PROJECT/.rovodev/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::RooCode,
        "Roo Code",
        &["$HOME/.roo/skills/**/SKILL.md", "$HOME/.roo/rules/*.md"],
        &[
            "$HOME/.roo",
            "$VSCODE_GLOBAL_STORAGE/rooveterinaryinc.roo-cline",
            "$ROO_CUSTOM_STORAGE",
        ],
        &[
            "$PROJECT/.roo/skills/**/SKILL.md",
            "$PROJECT/.roo/rules/*.md",
            "$PROJECT/.roorules",
        ],
    ),
    upstream(
        AgentId::Tabnine,
        "Tabnine CLI",
        &["$HOME/.tabnine/agent/skills/**/SKILL.md"],
        &["$HOME/.tabnine"],
        &[
            "$PROJECT/.tabnine/agent/skills/**/SKILL.md",
            "$PROJECT/.tabnine/guidelines/*.md",
        ],
    ),
    upstream(
        AgentId::Terramind,
        "Terramind",
        &["$HOME/.terramind/skills/**/SKILL.md"],
        &["$HOME/.terramind"],
        &["$PROJECT/.terramind/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Tinycloud,
        "Tinycloud",
        &["$HOME/.tinycloud/skills/**/SKILL.md"],
        &["$HOME/.tinycloud"],
        &["$PROJECT/.tinycloud/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Trae,
        "Trae",
        &["$HOME/.trae/skills/**/SKILL.md"],
        &["$HOME/.trae"],
        &["$PROJECT/.trae/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::TraeCn,
        "Trae CN",
        &["$HOME/.trae-cn/skills/**/SKILL.md"],
        &["$HOME/.trae-cn"],
        &["$PROJECT/.trae/skills/**/SKILL.md"],
    ),
    upstream(AgentId::Warp, "Warp", &[], &["$HOME/.warp"], &[]),
    upstream(
        AgentId::Windsurf,
        "Windsurf",
        &[
            "$HOME/.codeium/windsurf/skills/**/SKILL.md",
            "$HOME/.windsurf/global_rules.md",
        ],
        &["$HOME/.codeium/windsurf", "$HOME/.windsurf"],
        &[
            "$PROJECT/.windsurf/skills/**/SKILL.md",
            "$PROJECT/.windsurf/rules/*.md",
            "$PROJECT/.windsurfrules",
        ],
    ),
    upstream(
        AgentId::Zed,
        "Zed",
        &[],
        &[
            "$CONFIG_HOME/zed",
            "$APPDATA/Zed",
            "$FLATPAK_XDG_CONFIG_HOME/zed",
            "$MAC_APP_SUPPORT/Zed",
        ],
        &[],
    ),
    upstream(
        AgentId::ZCode,
        "ZCode",
        &["$HOME/.zcode/skills/**/SKILL.md"],
        &["$HOME/.zcode", "/Applications/ZCode.app"],
        &["$PROJECT/.zcode/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Zencoder,
        "Zencoder",
        &["$HOME/.zencoder/skills/**/SKILL.md"],
        &["$HOME/.zencoder"],
        &["$PROJECT/.zencoder/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Zenflow,
        "Zenflow",
        &["$HOME/.zencoder/skills/**/SKILL.md"],
        &["$HOME/.zencoder"],
        &["$PROJECT/.zencoder/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Neovate,
        "Neovate",
        &["$HOME/.neovate/skills/**/SKILL.md"],
        &["$HOME/.neovate"],
        &["$PROJECT/.neovate/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Pochi,
        "Pochi",
        &["$HOME/.pochi/skills/**/SKILL.md"],
        &["$HOME/.pochi"],
        &["$PROJECT/.pochi/skills/**/SKILL.md"],
    ),
    upstream(AgentId::PromptScript, "PromptScript", &[], &[], &[]),
    upstream(
        AgentId::Adal,
        "AdaL",
        &["$HOME/.adal/skills/**/SKILL.md"],
        &["$HOME/.adal"],
        &["$PROJECT/.adal/skills/**/SKILL.md"],
    ),
    upstream(
        AgentId::Universal,
        "Universal",
        &[
            "$HOME/.agents/skills/**/SKILL.md",
            "$CONFIG_HOME/agents/skills/**/SKILL.md",
        ],
        &[],
        &["$PROJECT/.agents/skills/**/SKILL.md"],
    ),
    legacy(
        AgentId::Aider,
        "Aider",
        &["$HOME/.aider.conf.yml"],
        &["$HOME/.aider.conf.yml"],
        &["$PROJECT/.aider.conf.yml", "$PROJECT/CONVENTIONS.md"],
        SkillFormat::Yaml,
    ),
    legacy(
        AgentId::AmazonQ,
        "Amazon Q",
        &[],
        &[],
        &["$PROJECT/.amazonq/rules/*.md"],
        SkillFormat::RulesDir,
    ),
    legacy(
        AgentId::JetbrainsAi,
        "JetBrains AI",
        &[],
        &[],
        &["$PROJECT/.aiassistant/rules/*.md"],
        SkillFormat::RulesDir,
    ),
];

pub fn get_agent_registry() -> Vec<AgentInfo> {
    get_agent_registry_with_environment(&LocationEnvironment::from_process())
}

pub(crate) fn get_agent_registry_with_environment(
    environment: &LocationEnvironment,
) -> Vec<AgentInfo> {
    debug_assert_eq!(
        CATALOG
            .iter()
            .filter(|entry| entry.origin == CatalogOrigin::PinnedUpstream)
            .count(),
        PINNED_UPSTREAM_AGENT_COUNT
    );
    debug_assert_eq!(
        CATALOG
            .iter()
            .filter(|entry| entry.origin == CatalogOrigin::RetainedLegacy)
            .count(),
        RETAINED_LEGACY_AGENT_COUNT
    );

    CATALOG
        .iter()
        .map(|entry| AgentInfo {
            id: entry.id.clone(),
            display_name: entry.display_name.to_string(),
            description: format!("{} skill and rule source", entry.display_name),
            color: brand_color(&entry.id).to_string(),
            installed: false,
            skill_count: 0,
            global_paths: resolve_templates(environment, entry.global_paths),
            global_detection_paths: resolve_templates(environment, entry.detection_paths),
            project_paths: resolve_templates(environment, entry.project_paths),
            format: entry.format.clone(),
        })
        .collect()
}

fn resolve_templates(environment: &LocationEnvironment, templates: &[&str]) -> Vec<String> {
    environment.resolve_all(templates)
}

fn brand_color(id: &AgentId) -> &'static str {
    match id {
        AgentId::ClaudeCode => "#f28c54",
        AgentId::Codex => "#4dc8b0",
        AgentId::Cursor => "#a58cff",
        AgentId::GithubCopilot => "#57a0ff",
        AgentId::Windsurf => "#49bed9",
        AgentId::Gemini | AgentId::Antigravity | AgentId::AntigravityCli => "#d5b347",
        AgentId::HermesAgent => "#d49a62",
        AgentId::KimiCodeCli => "#65b5d8",
        AgentId::Zed => "#d28c55",
        AgentId::Universal => "#a6a6a6",
        AgentId::Custom(_) => "#9ca3af",
        _ => "#8294b8",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use super::*;
    use crate::agents::locations::{LocationEnvironment, Platform};

    const PINNED_UPSTREAM_IDS: &[&str] = &[
        "aider-desk",
        "amp",
        "antigravity",
        "antigravity-cli",
        "astrbot",
        "autohand-code",
        "augment",
        "bob",
        "claude-code",
        "openclaw",
        "cline",
        "codearts-agent",
        "codebuddy",
        "codemaker",
        "codestudio",
        "codex",
        "command-code",
        "continue",
        "cortex",
        "crush",
        "cursor",
        "deepagents",
        "devin",
        "dexto",
        "droid",
        "eve",
        "firebender",
        "forgecode",
        "gemini-cli",
        "github-copilot",
        "goose",
        "grok",
        "hermes-agent",
        "inference-sh",
        "jazz",
        "junie",
        "iflow-cli",
        "kilo",
        "kimchi",
        "kimi-code-cli",
        "kiro-cli",
        "kode",
        "lingma",
        "loaf",
        "mcpjam",
        "mistral-vibe",
        "moxby",
        "mux",
        "opencode",
        "openhands",
        "ona",
        "pi",
        "qoder",
        "qoder-cn",
        "qwen-code",
        "replit",
        "reasonix",
        "rovodev",
        "roo",
        "tabnine-cli",
        "terramind",
        "tinycloud",
        "trae",
        "trae-cn",
        "warp",
        "windsurf",
        "zed",
        "zcode",
        "zencoder",
        "zenflow",
        "neovate",
        "pochi",
        "promptscript",
        "adal",
        "universal",
    ];

    fn serialized_id(id: &AgentId) -> String {
        serde_json::to_string(id)
            .expect("serialize agent id")
            .trim_matches('"')
            .to_string()
    }

    #[test]
    fn catalog_matches_pinned_upstream_plus_retained_legacy() {
        let upstream = CATALOG
            .iter()
            .filter(|entry| entry.origin == CatalogOrigin::PinnedUpstream)
            .count();
        let legacy = CATALOG
            .iter()
            .filter(|entry| entry.origin == CatalogOrigin::RetainedLegacy)
            .count();
        let ids: HashSet<_> = CATALOG
            .iter()
            .map(|entry| serialized_id(&entry.id))
            .collect();
        let upstream_ids: Vec<_> = CATALOG
            .iter()
            .filter(|entry| entry.origin == CatalogOrigin::PinnedUpstream)
            .map(|entry| serialized_id(&entry.id))
            .collect();

        assert_eq!(upstream, PINNED_UPSTREAM_AGENT_COUNT);
        assert_eq!(upstream_ids, PINNED_UPSTREAM_IDS);
        assert_eq!(legacy, RETAINED_LEGACY_AGENT_COUNT);
        assert_eq!(CATALOG.len(), 78);
        assert_eq!(ids.len(), CATALOG.len(), "catalog ids must be unique");
        assert_eq!(AGENT_CATALOG_VERSION, "2026-07-29.1");
        assert_eq!(
            AGENT_CATALOG_UPSTREAM_COMMIT,
            "e173b8c88f2581cfdaa1b6767c6519a08155790e"
        );
        assert!(AGENT_CATALOG_SOURCE.contains(AGENT_CATALOG_UPSTREAM_COMMIT));
    }

    #[test]
    fn shared_store_is_owned_only_by_universal() {
        let env = LocationEnvironment::fixture(Platform::Linux, "/home/test");
        let registry = get_agent_registry_with_environment(&env);
        let shared = "/home/test/.agents/skills/**/SKILL.md";
        let owners: Vec<_> = registry
            .iter()
            .filter(|agent| agent.global_paths.iter().any(|path| path == shared))
            .map(|agent| agent.id.clone())
            .collect();

        assert_eq!(owners, vec![AgentId::Universal]);
        for id in [
            AgentId::Amp,
            AgentId::Cline,
            AgentId::Dexto,
            AgentId::KimiCodeCli,
            AgentId::Loaf,
            AgentId::Warp,
            AgentId::Zed,
        ] {
            let agent = registry.iter().find(|agent| agent.id == id).expect("agent");
            assert!(
                !agent
                    .global_detection_paths
                    .iter()
                    .any(|path| path.contains(".agents/skills")),
                "{id:?} must use its independent installation probe"
            );
        }
    }

    #[test]
    fn hermes_kimi_and_windows_zed_resolve_to_real_agent_roots() {
        let mut env = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        env.hermes_home = PathBuf::from("D:/Agents/Hermes");
        env.app_data = Some(PathBuf::from("D:/Profiles/Roaming"));
        let registry = get_agent_registry_with_environment(&env);

        let hermes = registry
            .iter()
            .find(|agent| agent.id == AgentId::HermesAgent)
            .expect("Hermes");
        assert_eq!(
            hermes.global_paths,
            vec!["D:/Agents/Hermes/skills/**/SKILL.md"]
        );

        let kimi = registry
            .iter()
            .find(|agent| agent.id == AgentId::KimiCodeCli)
            .expect("Kimi");
        assert_eq!(
            kimi.global_detection_paths,
            vec!["C:/Users/Test/.kimi-code", "C:/Users/Test/.kimi"]
        );

        let zed = registry
            .iter()
            .find(|agent| agent.id == AgentId::Zed)
            .expect("Zed");
        assert!(zed
            .global_detection_paths
            .contains(&"D:/Profiles/Roaming/Zed".to_string()));
        assert!(!zed
            .global_detection_paths
            .iter()
            .any(|path| path.contains("Application Support")));
    }

    #[test]
    fn kimi_legacy_id_deserializes_to_current_identity() {
        let current: AgentId = serde_json::from_str("\"kimi-code-cli\"").expect("current id");
        let legacy: AgentId = serde_json::from_str("\"kimi-cli\"").expect("legacy id");
        assert_eq!(current, AgentId::KimiCodeCli);
        assert_eq!(legacy, AgentId::KimiCodeCli);
        assert_eq!(
            serde_json::to_string(&legacy).expect("serialize"),
            "\"kimi-code-cli\""
        );
    }

    #[test]
    fn provider_overrides_replace_only_their_documented_roots() {
        let mut env = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        env.gemini_home = PathBuf::from("D:/GeminiParent/.gemini");
        env.copilot_home = PathBuf::from("E:/Copilot");
        env.continue_home = PathBuf::from("F:/Continue");
        env.cline_data_dir = Some(PathBuf::from("G:/ClineData"));
        let registry = get_agent_registry_with_environment(&env);

        let gemini = registry
            .iter()
            .find(|agent| agent.id == AgentId::Gemini)
            .expect("Gemini");
        assert_eq!(
            gemini.global_paths,
            vec![
                "D:/GeminiParent/.gemini/skills/**/SKILL.md",
                "D:/GeminiParent/.gemini/GEMINI.md",
            ]
        );

        let copilot = registry
            .iter()
            .find(|agent| agent.id == AgentId::GithubCopilot)
            .expect("Copilot");
        assert!(copilot
            .global_paths
            .iter()
            .all(|path| path.starts_with("E:/Copilot/")));

        let continue_dev = registry
            .iter()
            .find(|agent| agent.id == AgentId::ContinueDev)
            .expect("Continue");
        assert_eq!(
            continue_dev.global_paths,
            vec!["F:/Continue/skills/**/SKILL.md"]
        );

        let cline = registry
            .iter()
            .find(|agent| agent.id == AgentId::Cline)
            .expect("Cline");
        assert!(cline
            .global_detection_paths
            .contains(&"G:/ClineData".to_string()));
    }

    #[test]
    fn extension_state_is_detection_only_and_never_a_transcript_glob() {
        let env = LocationEnvironment::fixture(Platform::Macos, "/Users/test");
        let registry = get_agent_registry_with_environment(&env);

        for id in [AgentId::Cline, AgentId::RooCode, AgentId::Kilo] {
            let agent = registry.iter().find(|agent| agent.id == id).expect("agent");
            assert!(agent
                .global_detection_paths
                .iter()
                .any(|path| path.contains("/User/globalStorage/")));
            assert!(agent
                .global_paths
                .iter()
                .all(|path| !path.contains("/User/globalStorage/")));
        }

        for agent in &registry {
            for path in agent.global_paths.iter().chain(&agent.project_paths) {
                let lower = path.to_ascii_lowercase();
                assert!(!lower.contains("events.jsonl"));
                assert!(!lower.contains("sessions.db"));
                assert!(!lower.contains("opencode.db"));
                assert!(!lower.contains("/tasks/**"));
            }
        }
    }

    #[test]
    fn copilot_custom_roots_scan_only_skill_artifacts() {
        let mut env = LocationEnvironment::fixture(Platform::Linux, "/home/test");
        env.copilot_skill_dirs = vec![
            PathBuf::from("/opt/copilot-skills"),
            PathBuf::from("/srv/team-skills"),
        ];
        let registry = get_agent_registry_with_environment(&env);
        let copilot = registry
            .iter()
            .find(|agent| agent.id == AgentId::GithubCopilot)
            .expect("Copilot");

        assert_eq!(
            copilot.global_paths,
            vec![
                "/home/test/.copilot/skills/**/SKILL.md",
                "/home/test/.copilot/skills/**/*.instructions.md",
                "/opt/copilot-skills/**/SKILL.md",
                "/srv/team-skills/**/SKILL.md",
            ]
        );
        assert!(copilot.global_paths.iter().all(|path| {
            !path.contains("session-state")
                && !path.contains("events.jsonl")
                && !path.contains("history")
        }));
    }
}
