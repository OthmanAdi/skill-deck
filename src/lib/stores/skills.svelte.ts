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
  ScanResult,
  AgentInfo,
  TabView,
  ViewMode,
  TerminalContext,
  UpdateCheckResponse,
  SkillHistoryResponse,
  SkillVersionEntry,
  RestoreSkillVersionResult,
} from "$lib/types";
import { DEFAULT_AGENT_COLOR } from "$lib/types";

export type OverlayMode = "pinned" | "auto-hide";

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
  activeTab = $state<TabView>("all");
  agentFilter = $state<string | null>(null);
  isVisible = $state(false);
  isLoading = $state(false);
  hotkey = $state("CommandOrControl+Shift+K");
  scanDurationMs = $state(0);
  lastScanTime = $state<number>(0);
  terminalContext = $state<TerminalContext>({
    isTerminalFocused: false,
    terminalName: null,
    cwd: null,
    shellPid: null,
  });

  /** Clipboard toast state */
  toastMessage = $state<string | null>(null);
  toastTimeout: ReturnType<typeof setTimeout> | null = null;

  /** Tracks which agent group sections are collapsed in the UI */
  collapsedAgents = $state<Set<string>>(new Set());

  /** Tracks which tree node sections are collapsed in tree view */
  collapsedTreeNodes = $state<Set<string>>(new Set());

  /** Active renderer mode for the main list area */
  viewMode = $state<ViewMode>("grouped");

  /** Overlay behavior mode: pinned (stays open) or auto-hide (hides on focus loss) */
  overlayMode = $state<OverlayMode>("pinned");

  /** Backward-compatible boolean toggle used by existing components */
  get treeMode(): boolean {
    return this.viewMode === "tree";
  }

  set treeMode(enabled: boolean) {
    this.viewMode = enabled ? "tree" : "grouped";
  }

  /** Whether the cursor is currently over a terminal during a drag */
  dragOverTerminal = $state(false);
  /** PID of the terminal under cursor during a drag */
  dragTerminalPid = $state<number | null>(null);
  /** Human readable terminal target while dragging */
  dragTargetLabel = $state<string | null>(null);
  /** Reference preview shown in the drag affordance */
  dragReferencePreview = $state<string | null>(null);

  /** Last update check status details per skill ID */
  updateStatus = $state<Record<string, UpdateCheckResponse>>({});

  /** Version history entries per skill ID */
  versionHistory = $state<Record<string, SkillVersionEntry[]>>({});

  /** Skills filtered by current tab, search query, and agent filter */
  get filteredSkills(): Skill[] {
    let result = this.skills;

    // Tab filter
    if (this.activeTab === "starred") {
      result = result.filter((s) => s.starred);
    } else if (this.activeTab === "project") {
      result = result.filter((s) => s.scope === "project");
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
          s.description.toLowerCase().includes(q)
      );
    }

    return result;
  }

  get installedAgentCount(): number {
    return this.agents.filter((a) => a.installed && a.skillCount > 0).length;
  }

  get starredCount(): number {
    return this.skills.filter((s) => s.starred).length;
  }

  get projectCount(): number {
    return this.skills.filter((s) => s.scope === "project").length;
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
      .map(([agentId, skills]) => ({
        agentId,
        agentName: this.getAgentDisplayName(agentId),
        skills,
        count: skills.length,
        startIndex: 0, // filled below
      }))
      .sort((a, b) => b.count - a.count || a.agentName.localeCompare(b.agentName));

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
export async function scanSkills(silent = false) {
  if (!silent) store.isLoading = true;
  try {
    const result: ScanResult = await invoke("scan_skills", {
      projectPath: store.terminalContext.cwd,
    });
    store.skills = result.skills;
    store.scanDurationMs = result.scanDurationMs;
    store.lastScanTime = Date.now();

    store.agents = await invoke("list_agents", {
      projectPath: store.terminalContext.cwd,
    });

    const config = await invoke<{
      hotkey?: string;
      collapsedAgents?: string[];
      collapsedTreeNodes?: string[];
      overlayMode?: OverlayMode;
    }>("get_config");
    store.hotkey = normalizeHotkey(config.hotkey);
    store.collapsedAgents = new Set(config.collapsedAgents ?? []);
    store.collapsedTreeNodes = new Set(config.collapsedTreeNodes ?? []);
    store.overlayMode = config.overlayMode === "auto-hide" ? "auto-hide" : "pinned";

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
  await detectContext();
  await scanSkills(false);
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
    s.id === skillId
      ? { ...s, icon: iconValue || null }
      : s
  );
}

/** Detect the current terminal context */
export async function detectContext() {
  const preservePrevious = store.isVisible && !!store.terminalContext.cwd;
  try {
    const next: TerminalContext = await invoke("detect_terminal_context");
    if (!next.cwd && preservePrevious) {
      store.terminalContext = {
        ...next,
        cwd: store.terminalContext.cwd,
        shellPid: store.terminalContext.shellPid,
        terminalName: store.terminalContext.terminalName,
        isTerminalFocused: store.terminalContext.isTerminalFocused,
      };
      return;
    }
    store.terminalContext = next;
  } catch (e) {
    console.error("Context detection failed:", e);
  }
}

/** Toggle overlay visibility.
 * @param contextAlreadyDetected — pass true when the caller already called
 *   detectContext() before showing (e.g. from the hotkey handler while the
 *   terminal still has focus). Prevents overwriting a valid CWD with a stale one.
 */
export function toggleOverlay(contextAlreadyDetected = false) {
  store.isVisible = !store.isVisible;
  if (store.isVisible) {
    if (contextAlreadyDetected) {
      // Context was captured before we grabbed focus — just scan with known CWD
      scanSkills();
    } else {
      detectContext().then(() => scanSkills());
    }
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

// ── Drag & drop terminal injection ───────────────────────────────────────────

let dragPollInterval: ReturnType<typeof setInterval> | null = null;

/** Start polling window-under-cursor during a drag operation */
export function startDragPoll() {
  if (dragPollInterval) return;
  store.dragOverTerminal = false;
  store.dragTerminalPid = null;
  store.dragTargetLabel = null;

  dragPollInterval = setInterval(async () => {
    try {
      const win: {
        found: boolean;
        isTerminal: boolean;
        pid?: number;
        processName?: string | null;
        windowTitle?: string | null;
      } =
        await invoke("get_window_at_cursor");
      store.dragOverTerminal = win.found && win.isTerminal;
      store.dragTerminalPid = win.isTerminal && win.pid != null ? win.pid : null;
      store.dragTargetLabel = win.isTerminal
        ? (win.windowTitle || win.processName || "terminal")
        : null;
    } catch {
      store.dragOverTerminal = false;
      store.dragTerminalPid = null;
      store.dragTargetLabel = null;
    }
  }, 80); // poll every 80ms — fast enough to feel responsive
}

/** Stop the drag poll and reset state */
export function stopDragPoll() {
  if (dragPollInterval) {
    clearInterval(dragPollInterval);
    dragPollInterval = null;
  }
  store.dragOverTerminal = false;
  store.dragTerminalPid = null;
  store.dragTargetLabel = null;
  store.dragReferencePreview = null;
}

/** Inject a skill's content into the terminal under the cursor */
export async function injectSkillToTerminal(skill: Skill, targetPid?: number): Promise<boolean> {
  const pid = targetPid ?? store.dragTerminalPid;
  if (!pid) return false;

  try {
    const result: { success: boolean; error?: string; reference?: string; referenceKind?: string } = await invoke("inject_skill_to_terminal", {
      skillId: skill.id,
      projectPath: store.terminalContext.cwd,
      targetPid: pid,
    });
    if (result.success) {
      showToast(`Injected: ${result.reference ?? skill.name}`);
    } else {
      showToast(`Inject failed: ${result.error ?? "unknown error"}`);
    }
    return result.success;
  } catch (e) {
    showToast(`Inject failed: ${e}`);
    return false;
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
      s.id === skillId
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
      projectPath: skill.projectPath,
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
      projectPath: skill.projectPath,
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
      s.id === skillId
        ? { ...s, metadata: { ...s.metadata, installCommand: cmd || null } }
        : s
    );
    showToast(cmd ? "Install command saved" : "Install command cleared");
  } catch (e) {
    showToast(`Failed to save install command: ${e}`);
  }
}

export async function resolveSkillReference(skill: Skill): Promise<string> {
  const response: { text: string; kind: string; availableInTarget: boolean } = await invoke("resolve_skill_reference", {
    skillId: skill.id,
    projectPath: store.terminalContext.cwd,
  });
  return response.text;
}
