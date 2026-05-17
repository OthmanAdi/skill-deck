/**
 * @agent-context: Global UI font scale.
 *
 * Applies CSS `zoom` to the document root so the entire overlay scales
 * proportionally (text, padding, gaps, icons, popovers). Tauri's WebView2
 * (Windows), WKWebView (macOS), and WebKitGTK (Linux) all support the `zoom`
 * property natively, so this approach works cross-platform without rewriting
 * every pixel size in the component tree.
 *
 * Because `zoom` scales LAYOUT geometry too — not just typography — the OS
 * window itself must grow proportionally or the right-side controls fall off
 * the edge. `setFontScale` therefore resizes the Tauri window by the ratio of
 * the new scale to the previous scale so the visible viewport tracks the
 * scaled UI 1:1. The new size is also persisted so it survives restart.
 *
 * Three preset steps surfaced in `FONT_SCALE_STEPS` keep the UI choice
 * deterministic. Backend clamps to [1.0, 2.0] regardless of what we send.
 */

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

export type FontScale = 1.0 | 1.5 | 2.0;

export const FONT_SCALE_STEPS: { value: FontScale; label: string; previewPx: number }[] = [
  { value: 1.0, label: "Small", previewPx: 11 },
  { value: 1.5, label: "Medium", previewPx: 14 },
  { value: 2.0, label: "Large", previewPx: 18 },
];

const MIN_SCALE = 1.0;
const MAX_SCALE = 2.0;
const DEFAULT_SCALE: FontScale = 1.0;

function normalizeScale(value: number | null | undefined): FontScale {
  if (value === null || value === undefined || !Number.isFinite(value)) return DEFAULT_SCALE;
  const clamped = Math.max(MIN_SCALE, Math.min(MAX_SCALE, value));
  // Snap to nearest preset step so the UI segmented control stays in sync.
  let nearest: FontScale = DEFAULT_SCALE;
  let nearestDist = Number.POSITIVE_INFINITY;
  for (const step of FONT_SCALE_STEPS) {
    const dist = Math.abs(step.value - clamped);
    if (dist < nearestDist) {
      nearest = step.value;
      nearestDist = dist;
    }
  }
  return nearest;
}

class FontScaleStore {
  current = $state<FontScale>(DEFAULT_SCALE);
}

export const fontScaleStore = new FontScaleStore();

function applyToDom(scale: FontScale): void {
  if (typeof document === "undefined") return;
  // Setting "" lets the browser drop the inline rule when we're at the
  // baseline scale, which is the cleanest default state for screenshots
  // and dev tools inspection.
  document.documentElement.style.zoom = scale === 1.0 ? "" : String(scale);
}

/**
 * Initialize the font scale on app startup. Reads persisted value from Rust
 * config and applies it to the DOM root before the user sees the first paint.
 */
export async function initFontScale(): Promise<void> {
  try {
    const config = await invoke<{ fontScale?: number }>("get_config");
    const next = normalizeScale(config.fontScale);
    fontScaleStore.current = next;
    applyToDom(next);
  } catch {
    fontScaleStore.current = DEFAULT_SCALE;
    applyToDom(DEFAULT_SCALE);
  }
}

/**
 * Resize the Tauri window so its inner viewport tracks the scaled UI. We
 * multiply the CURRENT logical window size by the (newScale / oldScale)
 * ratio — preserving any manual resize the user made at the previous scale —
 * and clamp into the bounds enforced by `set_overlay_size` on the backend.
 */
const MIN_W = 380;
const MAX_W = 2400;
const MIN_H = 560;
const MAX_H = 1800;

async function resizeWindowForScale(previous: FontScale, next: FontScale): Promise<void> {
  if (previous === next) return;
  if (typeof window === "undefined") return;
  try {
    const win = getCurrentWindow();
    const inner = await win.innerSize();
    const scaleFactor = await win.scaleFactor();

    // Convert physical to logical so the ratio math lines up with what the
    // Tauri config + persisted overlay_width / height use.
    const logicalWidth = inner.width / scaleFactor;
    const logicalHeight = inner.height / scaleFactor;

    const ratio = next / previous;
    const targetWidth = Math.round(Math.min(MAX_W, Math.max(MIN_W, logicalWidth * ratio)));
    const targetHeight = Math.round(Math.min(MAX_H, Math.max(MIN_H, logicalHeight * ratio)));

    await win.setSize(new LogicalSize(targetWidth, targetHeight));

    // Persist so a restart lands at the same scaled size instead of springing
    // back to the default.
    await invoke("set_overlay_size", { width: targetWidth, height: targetHeight });
  } catch (error) {
    console.warn("Failed to resize window for font scale:", error);
  }
}

/**
 * Set the active font scale. Applies to the DOM immediately, resizes the OS
 * window proportionally so the scaled UI fits, and persists via Tauri.
 * Returns the actual scale that was applied (after backend clamp).
 */
export async function setFontScale(scale: FontScale | number): Promise<FontScale> {
  const previous = fontScaleStore.current;
  const next = normalizeScale(scale);
  fontScaleStore.current = next;
  applyToDom(next);

  // Resize first so the visible window expands BEFORE the persist round-trip
  // returns — feels instant.
  await resizeWindowForScale(previous, next);

  try {
    await invoke<number>("set_font_scale", { scale: next });
  } catch (error) {
    console.warn("Failed to persist font scale:", error);
  }
  return next;
}
