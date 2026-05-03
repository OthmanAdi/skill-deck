<!--
  @agent-context: Root page — registers global hotkey, initializes theme, manages overlay visibility.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "@tauri-apps/api/core";
  import Overlay from "$lib/components/Overlay.svelte";
  import {
    store,
    toggleOverlay,
    setOverlayMode,
    type OverlayMode,
  } from "$lib/stores/skills.svelte";
  import { initTheme } from "$lib/stores/theme.svelte";
  import type { AppConfig } from "$lib/types";

  const appWindow = getCurrentWindow();
  let unlistenHotkeyPressed: (() => void) | undefined;
  let unlistenFocusChanged: (() => void) | undefined;
  let unlistenOverlayModeChanged: (() => void) | undefined;
  let unlistenOverlayVisibilityChanged: (() => void) | undefined;
  let removeWindowBlurListener: (() => void) | undefined;
  let autoHideFocusGuardTimer: ReturnType<typeof setInterval> | null = null;
  let hideInFlight = false;
  let showInFlight = false;

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
      await setupHotkeyBridge();
      setupEscapeKey();

      unlistenResized = await appWindow.onResized(() => {
        if (persistResizeTimer) clearTimeout(persistResizeTimer);
        persistResizeTimer = setTimeout(() => {
          void persistOverlaySize();
        }, 180);
      });

      // Always show on launch
      await showOverlay();

      const onWindowBlur = () => {
        void maybeAutoHide();
      };
      window.addEventListener("blur", onWindowBlur);
      removeWindowBlurListener = () => window.removeEventListener("blur", onWindowBlur);

      unlistenFocusChanged = await appWindow
        .onFocusChanged(async ({ payload: focused }) => {
          if (!focused) {
            await maybeAutoHide();
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
          store.overlayMode = payload;
          if (payload === "auto-hide") {
            void maybeAutoHide();
          }
        }
      });

      unlistenOverlayVisibilityChanged = await listen<boolean>(
        "overlay-visibility-changed",
        async ({ payload }) => {
          const visible = !!payload;

          if (visible === store.isVisible) {
            return;
          }

          if (visible) {
            toggleOverlay();
            await appWindow.show().catch(() => undefined);
            await appWindow.setFocus().catch(() => undefined);
          } else {
            toggleOverlay();
          }
        }
      );
    };

    void init();

    return () => {
      if (persistResizeTimer) clearTimeout(persistResizeTimer);
      unlistenHotkeyPressed?.();
      unlistenFocusChanged?.();
      unlistenOverlayModeChanged?.();
      unlistenOverlayVisibilityChanged?.();
      removeWindowBlurListener?.();
      if (autoHideFocusGuardTimer) clearInterval(autoHideFocusGuardTimer);
      unlistenResized?.();
    };
  });

  async function showOverlay() {
    if (showInFlight) return;
    showInFlight = true;
    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    try {
      if (!visible) {
        toggleOverlay();
      }
      await appWindow.show();
      await appWindow.setFocus();
    } finally {
      showInFlight = false;
    }
  }

  async function hideOverlay() {
    if (hideInFlight) return;
    hideInFlight = true;
    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    try {
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
    } finally {
      hideInFlight = false;
    }
  }

  async function maybeAutoHide() {
    if (store.overlayMode !== "auto-hide") return;
    if (!store.isVisible) return;

    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    if (!visible) return;

    const focused = await appWindow.isFocused().catch(() => document.hasFocus());
    if (!focused) {
      await hideOverlay();
    }
  }

  async function handleGlobalHotkeyPressed() {
    const visible = await appWindow.isVisible().catch(() => store.isVisible);
    if (!visible) {
      if (!store.isVisible) {
        toggleOverlay();
      }
      await appWindow.show();
      await appWindow.setFocus();
      return;
    }
    await hideOverlay();
  }

  async function setupHotkeyBridge() {
    const config: AppConfig = await invoke("get_config");
    store.hotkey = (config.hotkey || "").trim() || "CommandOrControl+Shift+K";

    unlistenHotkeyPressed = await listen("overlay-hotkey-pressed", async () => {
      await handleGlobalHotkeyPressed();
    });
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
