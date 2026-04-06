/**
 * @agent-context: TypeScript interfaces mirroring the Rust models.
 * These MUST stay in sync with src-tauri/src/models/*.rs.
 * All field names use camelCase (Rust structs use #[serde(rename_all = "camelCase")]).
 */

/** Universal skill representation across all coding agents */
export interface Skill {
  id: string;
  name: string;
  description: string;
  agentId: AgentId;
  filePath: string;
  scope: "global" | "project";
  projectPath: string | null;
  metadata: SkillMetadata;
  icon: string | null;
  starred: boolean;
  updateAvailable: boolean;
  parentId: string | null;
  children: Skill[];
}

/** Coding agent identifiers — matches AgentId enum in Rust */
export type AgentId =
  | "claude-code"
  | "cursor"
  | "github-copilot"
  | "codex"
  | "windsurf"
  | "gemini"
  | "cline"
  | "roo-code"
  | "continue-dev"
  | "aider"
  | "amazon-q"
  | "jetbrains-ai"
  | "tabnine"
  | "augment-code"
  | "open-hands"
  | "devin"
  | "universal"
  | { custom: string };

/** Extracted frontmatter metadata, normalized across formats */
export interface SkillMetadata {
  version: string | null;
  author: string | null;
  category: string | null;
  globs: string[] | null;
  trigger: string | null;
  allowedTools: string | null;
  userInvocable: boolean | null;
  language: string | null;
  extra: Record<string, unknown> | null;
  repositoryUrl: string | null;
  installCommand: string | null;
}

/** Agent info for the filter tabs */
export interface AgentInfo {
  id: AgentId;
  displayName: string;
  description: string;
  color: string;
  installed: boolean;
  skillCount: number;
}

/** Result of scanning all agents */
export interface ScanResult {
  skills: Skill[];
  errors: ScanError[];
  scanDurationMs: number;
}

export interface ScanError {
  filePath: string;
  message: string;
}

/** Terminal context from CWD detection */
export interface TerminalContext {
  isTerminalFocused: boolean;
  terminalName: string | null;
  cwd: string | null;
  shellPid: number | null;
}

/** App configuration */
export interface AppConfig {
  hotkey: string;
  starredSkills: string[];
  skillIcons: Record<string, string>;
  customScanPaths: string[];
  checkUpdates: boolean;
  theme: string;
  overlayPosition: "bottom-right" | "bottom-left" | "top-right" | "top-left";
  skillRepoOverrides: Record<string, string>;
  skillInstallOverrides: Record<string, string>;
  updateCheckCache: Record<string, UpdateCheckEntry>;
}

/** Cached update check result */
export interface UpdateCheckEntry {
  lastChecked: number;
  updateAvailable: boolean;
  remoteRef: string | null;
}

/** UI-only types */
export type TabView = "all" | "starred" | "project";

/** Agent display color mapping (CSS oklch values from registry) */
export const AGENT_COLORS: Record<string, string> = {
  "claude-code": "oklch(0.70 0.18 25)",
  "cursor": "oklch(0.70 0.15 280)",
  "github-copilot": "oklch(0.70 0.10 220)",
  "codex": "oklch(0.70 0.12 160)",
  "windsurf": "oklch(0.70 0.14 200)",
  "gemini": "oklch(0.70 0.16 60)",
  "cline": "oklch(0.70 0.12 310)",
  "roo-code": "oklch(0.70 0.15 340)",
  "continue-dev": "oklch(0.70 0.10 130)",
  "aider": "oklch(0.70 0.10 100)",
  "amazon-q": "oklch(0.70 0.16 35)",
  "jetbrains-ai": "oklch(0.70 0.18 350)",
  "tabnine": "oklch(0.70 0.12 250)",
  "augment-code": "oklch(0.70 0.14 175)",
  "universal": "oklch(0.70 0.08 260)",
};

/** Human-readable agent names */
export const AGENT_NAMES: Record<string, string> = {
  "claude-code": "Claude Code",
  "cursor": "Cursor",
  "github-copilot": "GitHub Copilot",
  "codex": "Codex",
  "windsurf": "Windsurf",
  "gemini": "Gemini",
  "cline": "Cline",
  "roo-code": "Roo Code",
  "continue-dev": "Continue",
  "aider": "Aider",
  "amazon-q": "Amazon Q",
  "jetbrains-ai": "JetBrains AI",
  "tabnine": "Tabnine",
  "augment-code": "Augment",
  "universal": "AGENTS.md",
};
