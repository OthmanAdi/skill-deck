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
  scope: "global";
  projectPath: null;
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
  | "aider-desk"
  | "adal"
  | "amp"
  | "antigravity"
  | "augment"
  | "bob"
  | "codearts-agent"
  | "codebuddy"
  | "codemaker"
  | "codestudio"
  | "command-code"
  | "continue"
  | "cortex"
  | "crush"
  | "cursor"
  | "deepagents"
  | "dexto"
  | "droid"
  | "firebender"
  | "forgecode"
  | "gemini-cli"
  | "goose"
  | "github-copilot"
  | "iflow-cli"
  | "junie"
  | "kilo"
  | "kimi-cli"
  | "kiro-cli"
  | "kode"
  | "mcpjam"
  | "mistral-vibe"
  | "mux"
  | "neovate"
  | "codex"
  | "opencode"
  | "openclaw"
  | "openhands"
  | "pi"
  | "pochi"
  | "qoder"
  | "qwen-code"
  | "replit"
  | "roo"
  | "rovodev"
  | "tabnine-cli"
  | "trae"
  | "trae-cn"
  | "warp"
  | "windsurf"
  | "zencoder"
  | "cline"
  | "aider"
  | "amazon-q"
  | "jetbrains-ai"
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

export type UpdateErrorKind =
  | "invalidRepoUrl"
  | "repoNotFound"
  | "rateLimited"
  | "accessDenied"
  | "network"
  | "invalidResponse"
  | "providerError";

export interface UpdateCheckResponse {
  checked: boolean;
  updateAvailable: boolean;
  canonicalRepoUrl: string | null;
  remoteRef: string | null;
  source: "cache" | "remote" | "error";
  error: string | null;
  errorKind: UpdateErrorKind | null;
}

export interface SkillVersionEntry {
  versionId: string;
  createdAt: number;
  reason: string;
  sourceRepoUrl: string | null;
  remoteRef: string | null;
  contentHash: string;
  contentBytes: number;
  snapshotPath: string;
}

export interface SkillHistoryResponse {
  skillId: string;
  entries: SkillVersionEntry[];
}

export interface RestoreSkillVersionResult {
  restored: boolean;
  versionId: string;
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

/** App configuration */
export interface AppConfig {
  hotkey: string;
  starredSkills: string[];
  skillIcons: Record<string, string>;
  customScanPaths: string[];
  checkUpdates: boolean;
  theme: string;
  overlayPosition: "bottom-right" | "bottom-left" | "top-right" | "top-left";
  overlayWidth: number;
  overlayHeight: number;
  overlayMode?: "pinned" | "auto-hide";
  skillRepoOverrides: Record<string, string>;
  skillInstallOverrides: Record<string, string>;
  updateCheckCache: Record<string, UpdateCheckEntry>;
  skillVersionHistory?: Record<string, SkillVersionEntry[]>;
  maxSkillHistoryEntries?: number;
  collapsedAgents?: string[];
  collapsedTreeNodes?: string[];
}

/** Cached update check result */
export interface UpdateCheckEntry {
  lastChecked: number;
  updateAvailable: boolean;
  remoteRef: string | null;
  repoRef?: string | null;
  lastError?: string | null;
  lastErrorKind?: UpdateErrorKind | null;
}

/** UI-only types */
export type TabView = "all" | "starred";
export type ViewMode = "grouped" | "tree";

export const DEFAULT_AGENT_COLOR = "#7a7fad";
