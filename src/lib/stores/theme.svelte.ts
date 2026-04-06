/**
 * @agent-context: Theme registry and store for Skill Deck.
 *
 * ADDING A NEW THEME:
 * 1. Add a [data-theme="your-id"] block in app.css with all --t-* CSS variables
 * 2. Add an entry to the THEMES array below
 * Done — the menu, persistence, and DOM application all work automatically.
 */

import { invoke } from "@tauri-apps/api/core";

export interface ThemeDefinition {
  id: string;
  name: string;
  description: string;
  colorScheme: "dark" | "light";
}

/**
 * Master theme registry.
 * Other sessions add their theme here (step 2 of 2 for adding a theme).
 */
export const THEMES: ThemeDefinition[] = [
  {
    id: "obsidian",
    name: "Obsidian",
    description: "Dark. Minimal. shadcn-inspired.",
    colorScheme: "dark",
  },
  {
    id: "obsidian-light",
    name: "Obsidian Light",
    description: "Light mode variant.",
    colorScheme: "light",
  },
];

// ── Reactive store ─────────────────────────────────────────────────────────

class ThemeStore {
  currentThemeId = $state<string>("obsidian");

  get currentTheme(): ThemeDefinition {
    return THEMES.find((t) => t.id === this.currentThemeId) ?? THEMES[0];
  }

  get availableThemes(): ThemeDefinition[] {
    return THEMES;
  }
}

export const themeStore = new ThemeStore();

// ── Actions ────────────────────────────────────────────────────────────────

/**
 * Initialize theme on app startup.
 * Reads persisted theme from Rust config, falls back to system preference.
 */
export async function initTheme(): Promise<void> {
  try {
    const config = await invoke<{ theme: string }>("get_config");
    const savedId = config.theme;

    if (savedId && THEMES.some((t) => t.id === savedId)) {
      themeStore.currentThemeId = savedId;
    } else {
      // Default: respect OS dark/light preference → map to obsidian variant
      const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      themeStore.currentThemeId = prefersDark ? "obsidian" : "obsidian-light";
    }
  } catch {
    // Fallback if IPC fails
    themeStore.currentThemeId = "obsidian";
  }

  applyTheme(themeStore.currentThemeId);
}

/**
 * Change the active theme, persist to disk, apply to DOM.
 */
export async function setTheme(themeId: string): Promise<void> {
  if (!THEMES.some((t) => t.id === themeId)) return;
  themeStore.currentThemeId = themeId;
  applyTheme(themeId);

  try {
    await invoke("set_theme", { theme: themeId });
  } catch {
    // Non-fatal — theme is still applied to DOM
    console.warn("Failed to persist theme to config");
  }
}

/**
 * Apply a theme ID to the DOM via data-theme attribute.
 * This is the single point that touches the DOM.
 */
function applyTheme(themeId: string): void {
  document.documentElement.dataset.theme = themeId;
  const theme = THEMES.find((t) => t.id === themeId);
  document.documentElement.style.colorScheme = theme?.colorScheme ?? "dark";
}
