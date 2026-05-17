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
  artifactType: ArtifactType;
  agentId: AgentId;
  sourceAgents: AgentId[];
  filePath: string;
  sourcePaths: string[];
  legacyIds: string[];
  scope: "global";
  projectPath: null;
  metadata: SkillMetadata;
  discoveryTags: string[];
  useCases: string[];
  discoveryHints: string[];
  icon: string | null;
  starred: boolean;
  updateAvailable: boolean;
  installedAt: number | null;
  parentId: string | null;
  children: Skill[];
}

export type SkillSortMode = "default" | "installed-newest" | "installed-oldest";

/** Marketplace / registry provider identifiers — match Rust ProviderId. */
export type RegistryProviderId = "skills-sh" | "claw-hub";

/** UI-only convenience: the "all sources" virtual provider for the selector. */
export type RegistryProviderSelection = RegistryProviderId | "all";

export type RegistryKind = "skill" | "plugin" | "hook" | "command" | "unknown";

/** Universal marketplace item — every provider normalizes into this shape. */
export interface RegistryItem {
  id: string;
  provider: RegistryProviderId;
  kind: RegistryKind;
  name: string;
  slug: string;
  installCommand: string;
  author: string | null;
  authorHandle: string | null;
  authorAvatarUrl: string | null;
  description: string | null;
  version: string | null;
  installs: number;
  source: string | null;
  /** Origin URL — the repo or canonical upstream (GitHub, GitLab, etc.). */
  sourceUrl: string | null;
  /** Marketplace listing URL on the hub itself (skills.sh, clawhub.ai, ...). */
  homepageUrl: string | null;
  updatedAt: string | null;
  score: number | null;
}

export interface MarketplaceSearchResponse {
  provider: RegistryProviderId;
  query: string;
  count: number;
  durationMs: number;
  items: RegistryItem[];
}

export interface ProviderSearchOutcome {
  provider: RegistryProviderId;
  response: MarketplaceSearchResponse | null;
  error: string | null;
}

export interface AggregatedMarketplaceResponse {
  query: string;
  providers: ProviderSearchOutcome[];
}

export type ArtifactType =
  | "skill"
  | "command"
  | "hook"
  | "rule"
  | "workflow"
  | "prompt"
  | "config"
  | "other";

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
  tags: string[] | null;
  useCases: string[] | null;
  globs: string[] | null;
  trigger: string | null;
  allowedTools: string | null;
  userInvocable: boolean | null;
  language: string | null;
  slashCommand: string | null;
  hookEvent: string | null;
  hookMatcher: string | null;
  hookCommand: string | null;
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
  finderOpen?: boolean;
  skillSortMode?: SkillSortMode;
  skillRepoOverrides: Record<string, string>;
  skillInstallOverrides: Record<string, string>;
  updateCheckCache: Record<string, UpdateCheckEntry>;
  skillVersionHistory?: Record<string, SkillVersionEntry[]>;
  maxSkillHistoryEntries?: number;
  collapsedAgents?: string[];
  collapsedTreeNodes?: string[];
  fontScale?: number;
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
export type TabView = "all" | "starred" | "registry";
export type ViewMode = "grouped" | "tree";

export const DEFAULT_AGENT_COLOR = "#7a7fad";
