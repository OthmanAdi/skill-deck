/**
 * @agent-context: Central skill store using Svelte 5 runes.
 *
 * PATTERN: Since module-level $state() can't be imported and mutated by other
 * components (Svelte 5 limitation), we wrap all state in a single reactive
 * object and export getter/setter functions for mutations.
 *
 * REFRESH STRATEGY:
 * - Re-scan on every overlay show (CWD may have changed)
 * - Background refresh every 30s while visible
 * - Manual refresh button in ContextBar
 * - CWD re-detected on each scan
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  Skill,
  SkillSortMode,
  RegistrySearchResponse,
  RegistrySkillSummary,
  ScanResult,
  AgentInfo,
  TabView,
  ViewMode,
  UpdateCheckResponse,
  SkillHistoryResponse,
  SkillVersionEntry,
  RestoreSkillVersionResult,
} from "$lib/types";
import { DEFAULT_AGENT_COLOR } from "$lib/types";

export type OverlayMode = "pinned" | "auto-hide";

const REGISTRY_DEFAULT_LIMIT = 20;

export const skillSortOptions: { id: SkillSortMode; label: string }[] = [
  { id: "default", label: "Default" },
  { id: "installed-newest", label: "Recently installed" },
  { id: "installed-oldest", label: "Oldest installed" },
];

function normalizeSkillSortMode(value: string | null | undefined): SkillSortMode {
  if (value === "installed-newest" || value === "installed-oldest") {
    return value;
  }
  return "default";
}

function normalizeHotkey(value: string | null | undefined): string {
  const raw = (value ?? "").trim().replace(/\s+/g, "");
  if (!raw) return "CommandOrControl+Shift+K";

  return raw
    .split("+")
    .filter(Boolean)
    .map((token) => {
      const upper = token.toUpperCase();
      if (upper === "CTRL" || upper === "CONTROL") return "Control";
      if (upper === "SHIFT") return "Shift";
      if (upper === "ALT" || upper === "OPTION") return "Alt";
      if (upper === "CMD" || upper === "COMMAND" || upper === "SUPER" || upper === "META") {
        return "Command";
      }
      if (
        upper === "CMDORCTRL" ||
        upper === "CMDORCONTROL" ||
        upper === "COMMANDORCTRL" ||
        upper === "COMMANDORCONTROL"
      ) {
        return "CommandOrControl";
      }
      if (upper.length === 1) return upper;
      if (upper.startsWith("KEY") && upper.length > 3) return upper.slice(3);
      return token;
    })
    .join("+");
}

// ── Reactive state container ─────────────────────────────────────────────────

class SkillStore {
  skills = $state<Skill[]>([]);
  agents = $state<AgentInfo[]>([]);
  searchQuery = $state("");
  selectedTags = $state<string[]>([]);
  selectedUseCases = $state<string[]>([]);
  selectedArtifactTypes = $state<string[]>([]);
  activeTab = $state<TabView>("all");
  agentFilter = $state<string | null>(null);
  isVisible = $state(false);
  isLoading = $state(false);
  hotkey = $state("CommandOrControl+Shift+K");
  scanDurationMs = $state(0);
  lastScanTime = $state<number>(0);

  /** Clipboard toast state */
  toastMessage = $state<string | null>(null);
  toastTimeout: ReturnType<typeof setTimeout> | null = null;

  /** Full skill modal state */
  fullSkillModalOpen = $state(false);
  fullSkillModalSkill = $state<Skill | null>(null);
  fullSkillModalContent = $state<string | null>(null);
  fullSkillModalLoading = $state(false);
  fullSkillModalError = $state<string | null>(null);

  /** Cache full skill file content by skill id */
  fullSkillContentCache = new Map<string, string>();

  /** Tracks which agent group sections are collapsed in the UI */
  collapsedAgents = $state<Set<string>>(new Set());

  /** Tracks which tree node sections are collapsed in tree view */
  collapsedTreeNodes = $state<Set<string>>(new Set());

  /** Active renderer mode for the main list area */
  viewMode = $state<ViewMode>("grouped");

  /** Overlay behavior mode: pinned (stays open) or auto-hide (hides on focus loss) */
  overlayMode = $state<OverlayMode>("pinned");
  finderOpen = $state(false);
  skillSortMode = $state<SkillSortMode>("default");

  registryQuery = $state("");
  registryResults = $state<RegistrySkillSummary[]>([]);
  registryResultCount = $state(0);
  registryDurationMs = $state(0);
  registryLoading = $state(false);
  registryError = $state<string | null>(null);
  registryLastSearchedQuery = $state("");

  get sortedSkills(): Skill[] {
    const items = this.skills.slice();

    if (this.skillSortMode === "installed-newest") {
      return items.sort((a, b) => {
        const aTs = a.installedAt ?? 0;
        const bTs = b.installedAt ?? 0;
        if (aTs !== bTs) return bTs - aTs;
        return a.name.localeCompare(b.name);
      });
    }

    if (this.skillSortMode === "installed-oldest") {
      return items.sort((a, b) => {
        const aTs = a.installedAt ?? Number.MAX_SAFE_INTEGER;
        const bTs = b.installedAt ?? Number.MAX_SAFE_INTEGER;
        if (aTs !== bTs) return aTs - bTs;
        return a.name.localeCompare(b.name);
      });
    }

    return items;
  }

  /** Backward-compatible boolean toggle used by existing components */
  get treeMode(): boolean {
    return this.viewMode === "tree";
  }

  set treeMode(enabled: boolean) {
    this.viewMode = enabled ? "tree" : "grouped";
  }

  /** Last update check status details per skill ID */
  updateStatus = $state<Record<string, UpdateCheckResponse>>({});

  /** Version history entries per skill ID */
  versionHistory = $state<Record<string, SkillVersionEntry[]>>({});

  /** Skills filtered by current tab, search query, and agent filter */
  get filteredSkills(): Skill[] {
    if (this.activeTab === "registry") {
      return [];
    }

    let result = this.sortedSkills;

    // Tab filter
    if (this.activeTab === "starred") {
      result = result.filter((s) => s.starred);
    }

    // Agent filter
    if (this.agentFilter) {
      const filter = this.agentFilter;
      result = result.filter((s) => {
        const id = typeof s.agentId === "string" ? s.agentId : "custom";
        return id === filter;
      });
    }

    // Search filter
    if (this.searchQuery.trim()) {
      const q = this.searchQuery.toLowerCase();
      result = result.filter(
        (s) =>
          s.name.toLowerCase().includes(q) ||
          s.description.toLowerCase().includes(q) ||
          s.discoveryTags.some((tag) => tag.toLowerCase().includes(q)) ||
          s.useCases.some((value) => value.toLowerCase().includes(q)) ||
          s.artifactType.toLowerCase().includes(q) ||
          (s.metadata.slashCommand ?? "").toLowerCase().includes(q) ||
          (s.metadata.hookEvent ?? "").toLowerCase().includes(q)
      );
    }

    if (this.selectedArtifactTypes.length > 0) {
      result = result.filter((s) => this.selectedArtifactTypes.includes(s.artifactType));
    }

    // Discovery tag filters
    if (this.selectedTags.length > 0) {
      result = result.filter((s) =>
        this.selectedTags.every((tag) => s.discoveryTags.includes(tag))
      );
    }

    // Use-case filters
    if (this.selectedUseCases.length > 0) {
      result = result.filter((s) =>
        this.selectedUseCases.every((useCase) => s.useCases.includes(useCase))
      );
    }

    return result;
  }

  get availableTags(): { label: string; count: number }[] {
    const counts = new Map<string, number>();
    for (const skill of this.skills) {
      for (const tag of skill.discoveryTags ?? []) {
        counts.set(tag, (counts.get(tag) ?? 0) + 1);
      }
    }

    return Array.from(counts.entries())
      .map(([label, count]) => ({ label, count }))
      .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
  }

  get availableUseCases(): { label: string; count: number }[] {
    const counts = new Map<string, number>();
    for (const skill of this.skills) {
      for (const useCase of skill.useCases ?? []) {
        counts.set(useCase, (counts.get(useCase) ?? 0) + 1);
      }
    }

    return Array.from(counts.entries())
      .map(([label, count]) => ({ label, count }))
      .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
  }

  get availableArtifactTypes(): { label: string; count: number }[] {
    const counts = new Map<string, number>();
    for (const skill of this.skills) {
      const key = (skill.artifactType ?? "skill").toLowerCase();
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }

    return Array.from(counts.entries())
      .map(([label, count]) => ({ label, count }))
      .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
  }

  get installedAgentCount(): number {
    return this.agents.filter((a) => a.installed && a.skillCount > 0).length;
  }

  get starredCount(): number {
    return this.skills.filter((s) => s.starred).length;
  }

  /**
   * filteredSkills grouped by agentId for the tree view.
   * Sorted by skill count descending, then agent name alphabetically.
   * Each group includes a cumulative startIndex for keyboard navigation.
   */
  get groupedSkills(): {
    agentId: string;
    agentName: string;
    skills: Skill[];
    count: number;
    startIndex: number;
  }[] {
    const groups = new Map<string, Skill[]>();

    for (const skill of this.filteredSkills) {
      const id = typeof skill.agentId === "string" ? skill.agentId : "custom";
      const bucket = groups.get(id);
      if (bucket) {
        bucket.push(skill);
      } else {
        groups.set(id, [skill]);
      }
    }

    const sorted = Array.from(groups.entries())
      .map(([agentId, skills]) => {
        const installedValues = skills
          .map((skill) => skill.installedAt)
          .filter((value): value is number => typeof value === "number");

        const newestInstalledAt =
          installedValues.length > 0 ? Math.max(...installedValues) : Number.NEGATIVE_INFINITY;
        const oldestInstalledAt =
          installedValues.length > 0 ? Math.min(...installedValues) : Number.POSITIVE_INFINITY;

        return {
          agentId,
          agentName: this.getAgentDisplayName(agentId),
          skills,
          count: skills.length,
          newestInstalledAt,
          oldestInstalledAt,
          startIndex: 0, // filled below
        };
      })
      .sort((a, b) => {
        if (this.skillSortMode === "installed-newest") {
          if (a.newestInstalledAt !== b.newestInstalledAt) {
            return b.newestInstalledAt - a.newestInstalledAt;
          }
          return a.agentName.localeCompare(b.agentName);
        }

        if (this.skillSortMode === "installed-oldest") {
          if (a.oldestInstalledAt !== b.oldestInstalledAt) {
            return a.oldestInstalledAt - b.oldestInstalledAt;
          }
          return a.agentName.localeCompare(b.agentName);
        }

        return b.count - a.count || a.agentName.localeCompare(b.agentName);
      });

    // Compute cumulative startIndex for flat keyboard navigation
    let cursor = 0;
    for (const group of sorted) {
      group.startIndex = cursor;
      cursor += group.count;
    }

    return sorted;
  }

  getAgentInfo(agentId: string): AgentInfo | null {
    return this.agents.find((agent) => {
      const id = typeof agent.id === "string" ? agent.id : "custom";
      return id === agentId;
    }) ?? null;
  }

  getAgentDisplayName(agentId: string): string {
    return this.getAgentInfo(agentId)?.displayName ?? agentId;
  }

  getAgentColor(agentId: string): string {
    return this.getAgentInfo(agentId)?.color ?? DEFAULT_AGENT_COLOR;
  }
}

/** Singleton store instance — import this in all components */
export const store = new SkillStore();

export function toggleTagFilter(tag: string) {
  const normalized = tag.trim().toLowerCase();
  if (!normalized) return;
  const next = new Set(store.selectedTags);
  if (next.has(normalized)) {
    next.delete(normalized);
  } else {
    next.add(normalized);
  }
  store.selectedTags = Array.from(next).sort((a, b) => a.localeCompare(b));
}

export function setFinderOpen(open: boolean) {
  store.finderOpen = open;
  void invoke("set_finder_open", { open }).catch((e) => {
    console.warn("Failed to persist finder state:", e);
  });
}

export async function setSkillSortMode(mode: SkillSortMode) {
  const normalized = normalizeSkillSortMode(mode);
  store.skillSortMode = normalized;

  try {
    const persisted = await invoke<SkillSortMode>("set_skill_sort_mode", { mode: normalized });
    store.skillSortMode = normalizeSkillSortMode(persisted);
  } catch (e) {
    console.warn("Failed to persist skill sort mode:", e);
    showToast("Could not save sort mode");
  }
}

export async function searchSkillsRegistry(
  query = store.registryQuery,
  limit = REGISTRY_DEFAULT_LIMIT,
) {
  const normalizedQuery = query.trim();
  store.registryQuery = query;
  store.registryLastSearchedQuery = normalizedQuery;

  if (normalizedQuery.length < 2) {
    store.registryResults = [];
    store.registryResultCount = 0;
    store.registryDurationMs = 0;
    store.registryError = null;
    store.registryLoading = false;
    return;
  }

  store.registryLoading = true;
  store.registryError = null;

  try {
    const response = await invoke<RegistrySearchResponse>("search_skills_registry", {
      query: normalizedQuery,
      limit,
    });

    store.registryResults = response.skills;
    store.registryResultCount = response.count ?? response.skills.length;
    store.registryDurationMs = response.durationMs ?? 0;
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    store.registryResults = [];
    store.registryResultCount = 0;
    store.registryDurationMs = 0;
    store.registryError = message;
  } finally {
    store.registryLoading = false;
  }
}

export async function setActiveTab(tab: TabView) {
  if (store.activeTab === tab) {
    return;
  }

  store.activeTab = tab;

  if (tab === "registry") {
    const trimmed = store.registryQuery.trim();
    if (trimmed.length >= 2 && store.registryLastSearchedQuery !== trimmed) {
      await searchSkillsRegistry(trimmed);
    }
  }
}

export function toggleArtifactTypeFilter(artifactType: string) {
  const normalized = artifactType.trim().toLowerCase();
  if (!normalized) return;

  const next = new Set(store.selectedArtifactTypes);
  if (next.has(normalized)) {
    next.delete(normalized);
  } else {
    next.add(normalized);
  }

  store.selectedArtifactTypes = Array.from(next).sort((a, b) => a.localeCompare(b));
}

export function toggleUseCaseFilter(useCase: string) {
  const normalized = useCase.trim().toLowerCase();
  if (!normalized) return;
  const next = new Set(store.selectedUseCases);
  if (next.has(normalized)) {
    next.delete(normalized);
  } else {
    next.add(normalized);
  }
  store.selectedUseCases = Array.from(next).sort((a, b) => a.localeCompare(b));
}

export function clearDiscoveryFilters() {
  store.selectedTags = [];
  store.selectedUseCases = [];
  store.selectedArtifactTypes = [];
}

/** Toggle collapse state of an agent section in the grouped list view */
export function toggleAgentCollapse(agentId: string) {
  const next = new Set(store.collapsedAgents);
  if (next.has(agentId)) {
    next.delete(agentId);
  } else {
    next.add(agentId);
  }
  store.collapsedAgents = next; // reassign to trigger Svelte 5 reactivity
  persistCollapsedAgents(next);
}

/** Replace collapsed grouped-agent ids and persist */
export function setCollapsedAgents(collapsed: Set<string>) {
  const next = new Set(collapsed);
  store.collapsedAgents = next;
  persistCollapsedAgents(next);
}

/** Collapse all visible grouped-agent sections and persist */
export function collapseAllAgentGroups(agentIds: string[]) {
  setCollapsedAgents(new Set(agentIds));
}

/** Expand all grouped-agent sections and persist */
export function expandAllAgentGroups() {
  setCollapsedAgents(new Set());
}

/** Toggle one tree node collapse state and persist */
export function toggleTreeNodeCollapse(nodeId: string) {
  const next = new Set(store.collapsedTreeNodes);
  if (next.has(nodeId)) {
    next.delete(nodeId);
  } else {
    next.add(nodeId);
  }
  store.collapsedTreeNodes = next;
  persistCollapsedTreeNodes(next);
}

/** Replace collapsed tree node ids and persist */
export function setCollapsedTreeNodes(collapsed: Set<string>) {
  const next = new Set(collapsed);
  store.collapsedTreeNodes = next;
  persistCollapsedTreeNodes(next);
}

/** Collapse all currently visible tree parents and persist */
export function collapseAllTreeNodes(skills: Skill[]) {
  const parentIds = new Set<string>();
  for (const skill of skills) {
    if (skill.parentId) {
      parentIds.add(skill.parentId);
    }
  }
  setCollapsedTreeNodes(parentIds);
}

/** Expand all tree nodes and persist */
export function expandAllTreeNodes() {
  setCollapsedTreeNodes(new Set());
}

function persistCollapsedAgents(collapsed: Set<string>) {
  const payload = Array.from(collapsed);
  void invoke("set_collapsed_agents", { collapsedAgents: payload }).catch((e) => {
    console.warn("Failed to persist collapsed agents:", e);
  });
}

function persistCollapsedTreeNodes(collapsed: Set<string>) {
  const payload = Array.from(collapsed);
  void invoke("set_collapsed_tree_nodes", { collapsedTreeNodes: payload }).catch((e) => {
    console.warn("Failed to persist collapsed tree nodes:", e);
  });
}

// ── Background refresh interval ──────────────────────────────────────────────

let refreshInterval: ReturnType<typeof setInterval> | null = null;
const REFRESH_INTERVAL_MS = 30_000; // 30 seconds
const SOFT_CACHE_MS = 60_000;

function startBackgroundRefresh() {
  stopBackgroundRefresh();
  refreshInterval = setInterval(() => {
    if (store.isVisible && !store.isLoading) {
      scanSkills(true); // silent refresh — no loading spinner
    }
  }, REFRESH_INTERVAL_MS);
}

function stopBackgroundRefresh() {
  if (refreshInterval) {
    clearInterval(refreshInterval);
    refreshInterval = null;
  }
}

// ── Actions ──────────────────────────────────────────────────────────────────

/** Scan all agents for skills */
export async function scanSkills(silent = false, force = false) {
  const recentlyScanned = Date.now() - store.lastScanTime < SOFT_CACHE_MS;
  if (!force && store.skills.length > 0 && recentlyScanned) {
    return;
  }

  if (!silent) store.isLoading = true;
  try {
    const result: ScanResult = await invoke("scan_skills");
    store.skills = result.skills;
    store.scanDurationMs = result.scanDurationMs;
    store.lastScanTime = Date.now();

    store.agents = await invoke("list_agents");

    const config = await invoke<{
      hotkey?: string;
      collapsedAgents?: string[];
      collapsedTreeNodes?: string[];
      overlayMode?: OverlayMode;
      finderOpen?: boolean;
      skillSortMode?: SkillSortMode;
    }>("get_config");
    store.hotkey = normalizeHotkey(config.hotkey);
    store.collapsedAgents = new Set(config.collapsedAgents ?? []);
    store.collapsedTreeNodes = new Set(config.collapsedTreeNodes ?? []);
    store.overlayMode = config.overlayMode === "auto-hide" ? "auto-hide" : "pinned";
    store.finderOpen = config.finderOpen === true;
    store.skillSortMode = normalizeSkillSortMode(config.skillSortMode);

    // Apply starred status from config
    const starred: string[] = await invoke("get_starred_skills");
    store.skills = store.skills.map((s) => ({
      ...s,
      starred: starred.includes(s.id),
    }));
  } catch (e) {
    console.error("Scan failed:", e);
  } finally {
    if (!silent) store.isLoading = false;
  }
}

/** Manual refresh — always shows loading indicator */
export async function refreshSkills() {
  await scanSkills(false, true);
}

/** Toggle star on a skill */
export async function toggleStar(skillId: string) {
  const isStarred: boolean = await invoke("toggle_star", { skillId });
  store.skills = store.skills.map((s) =>
    s.id === skillId ? { ...s, starred: isStarred } : s
  );
}

/** Persist a custom icon or clear it for a skill */
export async function setSkillIcon(skillId: string, icon: string | null) {
  const normalized = (icon ?? "").trim();
  const iconValue = normalized.length > 0 ? normalized : "";

  await invoke("set_skill_icon", {
    skillId,
    icon: iconValue,
  });

  store.skills = store.skills.map((s) =>
    s.id === skillId || s.legacyIds.includes(skillId)
      ? { ...s, icon: iconValue || null }
      : s
  );
}

/** Toggle overlay visibility.
 */
export function toggleOverlay() {
  store.isVisible = !store.isVisible;
  if (store.isVisible) {
    scanSkills();
    startBackgroundRefresh();
  } else {
    stopBackgroundRefresh();
  }
}

/** Show a toast notification (auto-dismisses after 2.5s) */
export function showToast(message: string) {
  if (store.toastTimeout) clearTimeout(store.toastTimeout);
  store.toastMessage = message;
  store.toastTimeout = setTimeout(() => {
    store.toastMessage = null;
    store.toastTimeout = null;
  }, 2500);
}

/** Copy skill reference to clipboard with fallback chain */
export async function copySkillReference(skill: Skill) {
  try {
    const reference = await resolveSkillReference(skill);
    await navigator.clipboard.writeText(reference);
    showToast(`Copied: ${reference.length > 50 ? reference.slice(0, 47) + "..." : reference}`);
  } catch {
    // Fallback: try the Tauri clipboard plugin or just show the path
    showToast(`Path: ${skill.filePath}`);
  }
}

// ── Repo / update management ─────────────────────────────────────────────────

/** Check a skill for updates via GitHub API */
export async function checkSkillUpdate(skill: Skill): Promise<void> {
  const repoUrl = skill.metadata.repositoryUrl;
  if (!repoUrl) {
    showToast("No repo URL — add one first");
    return;
  }
  try {
    const response: UpdateCheckResponse = await invoke("check_skill_update", {
      skillId: skill.id,
      repoUrl,
      force: true,
    });
    store.updateStatus = {
      ...store.updateStatus,
      [skill.id]: response,
    };

    if (response.canonicalRepoUrl && response.canonicalRepoUrl !== repoUrl) {
      await setSkillRepo(skill.id, response.canonicalRepoUrl);
    }

    const updateAvailable = response.updateAvailable;
    store.skills = store.skills.map((s) =>
      s.id === skill.id ? { ...s, updateAvailable } : s
    );

    if (response.error) {
      if (response.errorKind === "repoNotFound") {
        showToast("Repo not found, check URL or permissions");
      } else if (response.errorKind === "rateLimited") {
        showToast("Update check rate limited by GitHub");
      } else if (response.errorKind === "invalidRepoUrl") {
        showToast("Invalid repo URL, use github.com/owner/repo");
      } else {
        showToast(`Update check failed: ${response.error}`);
      }
      return;
    }

    if (updateAvailable) {
      showToast(response.source === "cache" ? "Cached: update available" : "Update available");
    } else {
      showToast(response.source === "cache" ? "Cached: up to date" : "Already up to date");
    }
  } catch (e) {
    showToast(`Update check failed: ${e}`);
  }
}

/** Persist overlay behavior mode */
export async function setOverlayMode(mode: OverlayMode) {
  const normalized: OverlayMode = mode === "auto-hide" ? "auto-hide" : "pinned";
  store.overlayMode = normalized;
  try {
    await invoke("set_overlay_mode", { mode: normalized });
  } catch (e) {
    console.warn("Failed to persist overlay mode:", e);
    showToast("Could not save window behavior setting");
  }
}

export async function setHotkey(hotkey: string) {
  const normalized = normalizeHotkey(hotkey);
  const previous = store.hotkey;
  store.hotkey = normalized;
  try {
    const active = await invoke<string>("set_hotkey", { hotkey: normalized });
    store.hotkey = normalizeHotkey(active);
    showToast(`Shortcut set: ${store.hotkey}`);
  } catch (e) {
    store.hotkey = previous;
    console.warn("Failed to persist hotkey:", e);
    const message = e instanceof Error ? e.message : String(e);
    showToast(`Shortcut not saved: ${message}`);
  }
}

/** Set or override a skill's repository URL */
export async function setSkillRepo(skillId: string, repoUrl: string): Promise<void> {
  try {
    const canonicalRepoUrl: string | null = await invoke("set_skill_repo", { skillId, repoUrl });
    const resolvedRepo = canonicalRepoUrl ?? null;
    // Update local state — inject the new repo URL into metadata
    store.skills = store.skills.map((s) =>
      s.id === skillId || s.legacyIds.includes(skillId)
        ? { ...s, metadata: { ...s.metadata, repositoryUrl: resolvedRepo } }
        : s
    );
    showToast(resolvedRepo ? "Repo URL saved" : "Repo URL cleared");
  } catch (e) {
    showToast(`Failed to save repo URL: ${e}`);
  }
}

/** Snapshot skill content before external update actions */
export async function snapshotSkillBeforeUpdate(
  skill: Skill,
  remoteRef?: string,
  reason = "before-update",
): Promise<SkillVersionEntry | null> {
  try {
    const entry: SkillVersionEntry = await invoke("snapshot_skill_before_update", {
      skillId: skill.id,
      sourceRepoUrl: skill.metadata.repositoryUrl,
      remoteRef: remoteRef ?? null,
      reason,
    });
    const prev = store.versionHistory[skill.id] ?? [];
    store.versionHistory = {
      ...store.versionHistory,
      [skill.id]: [entry, ...prev.filter((v) => v.versionId !== entry.versionId)],
    };
    showToast("Snapshot saved before update");
    return entry;
  } catch (e) {
    showToast(`Snapshot failed: ${e}`);
    return null;
  }
}

/** Load local version history entries for a skill */
export async function loadSkillVersionHistory(skillId: string): Promise<SkillVersionEntry[]> {
  try {
    const response: SkillHistoryResponse = await invoke("list_skill_versions", { skillId });
    store.versionHistory = {
      ...store.versionHistory,
      [skillId]: response.entries,
    };
    return response.entries;
  } catch {
    return store.versionHistory[skillId] ?? [];
  }
}

/** Restore a specific version snapshot for a skill */
export async function restoreSkillVersion(skill: Skill, versionId: string): Promise<boolean> {
  try {
    const result: RestoreSkillVersionResult = await invoke("restore_skill_version", {
      skillId: skill.id,
      versionId,
    });
    if (result.restored) {
      await scanSkills(true);
      await loadSkillVersionHistory(skill.id);
      showToast("Skill restored from snapshot");
      return true;
    }
    showToast("Restore failed");
    return false;
  } catch (e) {
    showToast(`Restore failed: ${e}`);
    return false;
  }
}

/** Set or override a skill's install command */
export async function setSkillInstallCommand(skillId: string, cmd: string): Promise<void> {
  try {
    await invoke("set_skill_install_command", { skillId, installCommand: cmd });
    store.skills = store.skills.map((s) =>
      s.id === skillId || s.legacyIds.includes(skillId)
        ? { ...s, metadata: { ...s.metadata, installCommand: cmd || null } }
        : s
    );
    showToast(cmd ? "Install command saved" : "Install command cleared");
  } catch (e) {
    showToast(`Failed to save install command: ${e}`);
  }
}

export async function resolveSkillReference(skill: Skill): Promise<string> {
  const hookCommand = (skill.metadata.hookCommand ?? "").trim();
  if (hookCommand.length > 0) {
    return hookCommand;
  }

  const slashCommand = (skill.metadata.slashCommand ?? "").trim();
  if (slashCommand.length > 0) {
    return slashCommand;
  }

  const agentId = typeof skill.agentId === "string" ? skill.agentId : "custom";
  const normalizedPath = skill.filePath.replace(/\\/g, "/").toLowerCase();

  if (agentId === "claude-code" && normalizedPath.includes("/.claude/commands/")) {
    const filename = skill.filePath.split(/[\\/]/).pop()?.replace(/\.md$/i, "");
    if (filename) return `/${filename}`;
  }

  if (agentId === "claude-code" && skill.metadata.userInvocable) {
    const slug = skill.name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9._-]+/g, "-")
      .replace(/^-+|-+$/g, "");
    if (slug) return `/${slug}`;
  }

  return `"${skill.filePath.replace(/"/g, '\\"')}"`;
}

let fullSkillModalRequestToken = 0;

export async function openFullSkillModal(skill: Skill, preloadedContent?: string | null) {
  fullSkillModalRequestToken += 1;
  const token = fullSkillModalRequestToken;

  store.fullSkillModalOpen = true;
  store.fullSkillModalSkill = skill;
  store.fullSkillModalError = null;

  const normalizedPreloaded = (preloadedContent ?? "").trim();
  if (normalizedPreloaded && normalizedPreloaded !== "// Could not read file") {
    store.fullSkillModalContent = preloadedContent ?? null;
    store.fullSkillContentCache.set(skill.id, preloadedContent ?? "");
    store.fullSkillModalLoading = false;
    return;
  }

  const cached = store.fullSkillContentCache.get(skill.id);
  if (cached) {
    store.fullSkillModalContent = cached;
    store.fullSkillModalLoading = false;
    return;
  }

  store.fullSkillModalContent = null;
  store.fullSkillModalLoading = true;

  try {
    const content = await invoke<string>("read_skill_content", { skillId: skill.id });

    if (token !== fullSkillModalRequestToken) {
      return;
    }

    store.fullSkillModalContent = content;
    store.fullSkillContentCache.set(skill.id, content);
    store.fullSkillModalLoading = false;
  } catch (e) {
    if (token !== fullSkillModalRequestToken) {
      return;
    }

    store.fullSkillModalLoading = false;
    store.fullSkillModalError =
      e instanceof Error ? e.message : "Could not load full skill content";
  }
}

export function closeFullSkillModal() {
  fullSkillModalRequestToken += 1;
  store.fullSkillModalOpen = false;
  store.fullSkillModalSkill = null;
  store.fullSkillModalContent = null;
  store.fullSkillModalLoading = false;
  store.fullSkillModalError = null;
}
