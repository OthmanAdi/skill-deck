/**
 * @agent-context: Theme registry and store for Skill Deck.
 *
 * Supported modes:
 * - system (follows OS setting)
 * - dark
 * - light
 */

import { invoke } from "@tauri-apps/api/core";

export type ThemeId = "system" | "dark" | "light";
type ResolvedThemeId = "dark" | "light";

export interface ThemeDefinition {
  id: ThemeId;
  name: string;
  description: string;
  colorScheme: "system" | "dark" | "light";
}

export const THEMES: ThemeDefinition[] = [
  {
    id: "system",
    name: "System",
    description: "Follow OS light or dark preference.",
    colorScheme: "system",
  },
  {
    id: "dark",
    name: "Dark",
    description: "Linear-inspired dark canvas.",
    colorScheme: "dark",
  },
  {
    id: "light",
    name: "Light",
    description: "Linear-inspired inverse surface.",
    colorScheme: "light",
  },
];

const LEGACY_THEME_MAP: Record<string, ThemeId> = {
  "system": "system",
  "dark": "dark",
  "light": "light",
  "obsidian": "dark",
  "obsidian-light": "light",
};

// ── Reactive store ─────────────────────────────────────────────────────────

class ThemeStore {
  currentThemeId = $state<ThemeId>("system");
  resolvedThemeId = $state<ResolvedThemeId>("dark");

  get currentTheme(): ThemeDefinition {
    return THEMES.find((t) => t.id === this.currentThemeId) ?? THEMES[0];
  }

  get availableThemes(): ThemeDefinition[] {
    return THEMES;
  }
}

export const themeStore = new ThemeStore();

let systemMediaQuery: MediaQueryList | null = null;
let systemListenerAttached = false;

function normalizeThemeId(value?: string | null): ThemeId {
  if (!value) return "system";
  return LEGACY_THEME_MAP[value] ?? "system";
}

function detectSystemTheme(): ResolvedThemeId {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function resolveTheme(themeId: ThemeId): ResolvedThemeId {
  return themeId === "system" ? detectSystemTheme() : themeId;
}

function ensureSystemListener(): void {
  if (typeof window === "undefined") return;
  if (!systemMediaQuery) {
    systemMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  }
  if (systemListenerAttached) return;

  const handler = () => {
    if (themeStore.currentThemeId === "system") {
      applyTheme("system");
    }
  };

  if (typeof systemMediaQuery.addEventListener === "function") {
    systemMediaQuery.addEventListener("change", handler);
  } else {
    systemMediaQuery.addListener(handler);
  }

  systemListenerAttached = true;
}

// ── Actions ────────────────────────────────────────────────────────────────

/**
 * Initialize theme on app startup.
 * Reads persisted theme from Rust config with legacy migration support.
 */
export async function initTheme(): Promise<void> {
  ensureSystemListener();

  try {
    const config = await invoke<{ theme?: string }>("get_config");
    themeStore.currentThemeId = normalizeThemeId(config.theme);
  } catch {
    themeStore.currentThemeId = "system";
  }

  applyTheme(themeStore.currentThemeId);
}

/**
 * Change the active theme, persist to disk, apply to DOM.
 */
export async function setTheme(themeId: ThemeId | string): Promise<void> {
  const normalized = normalizeThemeId(themeId);
  ensureSystemListener();
  themeStore.currentThemeId = normalized;
  applyTheme(normalized);

  try {
    await invoke("set_theme", { theme: normalized });
  } catch {
    // Non-fatal — theme is still applied to DOM
    console.warn("Failed to persist theme to config");
  }
}

/**
 * Apply a theme mode to the DOM via resolved data-theme attribute.
 * This is the single point that touches the DOM.
 */
function applyTheme(themeId: ThemeId): void {
  const resolved = resolveTheme(themeId);
  themeStore.resolvedThemeId = resolved;

  document.documentElement.dataset.theme = resolved;
  document.documentElement.dataset.themeMode = themeId;
  document.documentElement.style.colorScheme = resolved;
}
