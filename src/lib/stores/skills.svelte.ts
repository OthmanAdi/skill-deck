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
import type { Skill, ScanResult, AgentInfo, TabView, TerminalContext } from "$lib/types";
import { AGENT_NAMES } from "$lib/types";

// ── Reactive state container ─────────────────────────────────────────────────

class SkillStore {
  skills = $state<Skill[]>([]);
  agents = $state<AgentInfo[]>([]);
  searchQuery = $state("");
  activeTab = $state<TabView>("all");
  agentFilter = $state<string | null>(null);
  isVisible = $state(false);
  isLoading = $state(false);
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

  /** Tree view mode toggle — renders SkillTree instead of flat list */
  treeMode = $state(false);

  /** Whether the cursor is currently over a terminal during a drag */
  dragOverTerminal = $state(false);
  /** PID of the terminal under cursor during a drag */
  dragTerminalPid = $state<number | null>(null);

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
        agentName: AGENT_NAMES[agentId] ?? agentId,
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

/** Detect the current terminal context */
export async function detectContext() {
  try {
    store.terminalContext = await invoke("detect_terminal_context");
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
  // Build the best reference string based on the agent
  const agentId = typeof skill.agentId === "string" ? skill.agentId : "custom";
  let reference: string;

  // Agent-specific reference formats
  if (agentId === "claude-code" && skill.metadata.userInvocable) {
    // Claude Code slash commands: /skill-name
    const slug = skill.name.toLowerCase().replace(/\s+/g, "-");
    reference = `/${slug}`;
  } else {
    // Universal fallback: file path
    reference = skill.filePath;
  }

  try {
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

  dragPollInterval = setInterval(async () => {
    try {
      const win: { found: boolean; isTerminal: boolean; pid?: number } =
        await invoke("get_window_at_cursor");
      store.dragOverTerminal = win.found && win.isTerminal;
      store.dragTerminalPid = win.isTerminal && win.pid != null ? win.pid : null;
    } catch {
      store.dragOverTerminal = false;
      store.dragTerminalPid = null;
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
}

/** Inject a skill's content into the terminal under the cursor */
export async function injectSkillToTerminal(skill: Skill, targetPid?: number): Promise<boolean> {
  const pid = targetPid ?? store.dragTerminalPid;
  if (!pid) return false;

  // Use the file path as the content to inject — terminals can open it or reference it
  // For Claude Code skills, inject the slash command form if user-invocable
  const agentId = typeof skill.agentId === "string" ? skill.agentId : "custom";
  let content: string;
  if (agentId === "claude-code" && skill.metadata.userInvocable) {
    const slug = skill.name.toLowerCase().replace(/\s+/g, "-");
    content = `/${slug}`;
  } else {
    content = skill.filePath;
  }

  try {
    const result: { success: boolean; error?: string } = await invoke("inject_to_terminal", {
      content,
      targetPid: pid,
    });
    if (result.success) {
      showToast(`Injected: ${skill.name}`);
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
    const updateAvailable: boolean = await invoke("check_skill_update", {
      skillId: skill.id,
      repoUrl,
    });
    store.skills = store.skills.map((s) =>
      s.id === skill.id ? { ...s, updateAvailable } : s
    );
    showToast(updateAvailable ? "Update available!" : "Already up to date");
  } catch (e) {
    showToast(`Update check failed: ${e}`);
  }
}

/** Set or override a skill's repository URL */
export async function setSkillRepo(skillId: string, repoUrl: string): Promise<void> {
  try {
    await invoke("set_skill_repo", { skillId, repoUrl });
    // Update local state — inject the new repo URL into metadata
    store.skills = store.skills.map((s) =>
      s.id === skillId
        ? { ...s, metadata: { ...s.metadata, repositoryUrl: repoUrl || null } }
        : s
    );
    showToast(repoUrl ? "Repo URL saved" : "Repo URL cleared");
  } catch (e) {
    showToast(`Failed to save repo URL: ${e}`);
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
