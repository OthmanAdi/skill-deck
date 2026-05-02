<!--
  @agent-context: Root page — registers global hotkey, initializes theme, manages overlay visibility.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import Overlay from "$lib/components/Overlay.svelte";
  import {
    store,
    toggleOverlay,
    detectContext,
    setOverlayMode,
    showToast,
    type OverlayMode,
  } from "$lib/stores/skills.svelte";
  import { initTheme } from "$lib/stores/theme.svelte";
  import type { AppConfig } from "$lib/types";

  const appWindow = getCurrentWindow();
  let unlistenFocusChanged: (() => void) | undefined;
  let unlistenOverlayModeChanged: (() => void) | undefined;
  let removeWindowBlurListener: (() => void) | undefined;
  let autoHideFocusGuardTimer: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    const mode = store.overlayMode;
    void appWindow.setAlwaysOnTop(mode !== "auto-hide").catch(() => {});
  });

  onMount(() => {
    let unlistenResized: (() => void) | undefined;
    let persistResizeTimer: ReturnType<typeof setTimeout> | null = null;

    const persistOverlaySize = async () => {
      try {
        const size = await appWindow.innerSize();
        const scale = await appWindow.scaleFactor();
        const width = Math.round(size.width / scale);
        const height = Math.round(size.height / scale);
        await invoke("set_overlay_size", { width, height });
      } catch (e) {
        console.warn("Failed to persist overlay size:", e);
      }
    };

    const init = async () => {
      await initTheme();
      await setupHotkey();
      setupEscapeKey();

      unlistenResized = await appWindow.onResized(() => {
        if (persistResizeTimer) clearTimeout(persistResizeTimer);
        persistResizeTimer = setTimeout(() => {
          void persistOverlaySize();
        }, 180);
      });

      // Always show on launch
      await showOverlay();

      await appWindow
        .setAlwaysOnTop(store.overlayMode !== "auto-hide")
        .catch(() => {});

      const onWindowBlur = () => {
        if (store.overlayMode === "auto-hide") {
          void hideOverlay();
        }
      };
      window.addEventListener("blur", onWindowBlur);
      removeWindowBlurListener = () => window.removeEventListener("blur", onWindowBlur);

      unlistenFocusChanged = await appWindow
        .onFocusChanged(async ({ payload: focused }) => {
          if (!focused && store.overlayMode === "auto-hide") {
            await hideOverlay();
          }
        })
        .catch(() => undefined);

      autoHideFocusGuardTimer = setInterval(async () => {
        if (!store.isVisible || store.overlayMode !== "auto-hide") return;
        const focused = await appWindow.isFocused().catch(() => document.hasFocus());
        if (!focused) {
          await hideOverlay();
        }
      }, 220);

      unlistenOverlayModeChanged = await listen<OverlayMode>("overlay-mode-changed", ({ payload }) => {
        if (payload === "auto-hide" || payload === "pinned") {
          void setOverlayMode(payload);
          void appWindow.setAlwaysOnTop(payload !== "auto-hide").catch(() => {});
        }
      });
    };

    void init();

    return () => {
      if (persistResizeTimer) clearTimeout(persistResizeTimer);
      unlistenFocusChanged?.();
      unlistenOverlayModeChanged?.();
      removeWindowBlurListener?.();
      if (autoHideFocusGuardTimer) clearInterval(autoHideFocusGuardTimer);
      unlistenResized?.();
    };
  });

  async function showOverlay() {
    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    if (!visible) {
      toggleOverlay();
    }
    await appWindow.show();
    await appWindow.setFocus();
  }

  async function hideOverlay() {
    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    if (!visible) {
      if (store.isVisible) {
        toggleOverlay();
      }
      return;
    }
    if (store.isVisible) {
      toggleOverlay();
    }
    await appWindow.hide();
  }

  async function setupHotkey() {
    const defaultHotkey = "CommandOrControl+Shift+K";
    const config: AppConfig = await invoke("get_config");
    const preferredHotkey = (config.hotkey || "").trim() || defaultHotkey;
    const candidates = Array.from(
      new Set([
        preferredHotkey,
        defaultHotkey,
        "Ctrl+Shift+K",
        "Control+Shift+K",
        "CmdOrControl+Shift+K",
      ])
    );

    for (const hotkey of candidates) {
      try {
        await unregister(hotkey).catch(() => {});

        await register(hotkey, async (event) => {
          if (event.state !== "Pressed") return;
          const visible = await appWindow.isVisible().catch(() => store.isVisible);
          if (!visible) {
            // CRITICAL: detect terminal context NOW — before showing the overlay.
            // Once our window gains focus, GetForegroundWindow() returns us, not the terminal.
            await detectContext();
            if (!store.isVisible) {
              toggleOverlay(true); // skip duplicate context detection inside toggleOverlay
            }
            await appWindow.show();
            await appWindow.setFocus();
          } else {
            await hideOverlay();
          }
        });

        store.hotkey = hotkey;
        return;
      } catch {
        // try next candidate
      }
    }

    showToast("Hotkey failed, use tray Show or Hide");
    console.warn("Hotkey registration failed for all candidates");
  }

  function setupEscapeKey() {
    document.addEventListener("keydown", async (e) => {
      if (e.key === "Escape" && store.isVisible) {
        await hideOverlay();
      }
    });
  }
</script>

<svelte:head>
  <title>Skill Deck</title>
</svelte:head>

<main class="h-screen w-screen overflow-hidden bg-transparent">
  <Overlay />
</main>
