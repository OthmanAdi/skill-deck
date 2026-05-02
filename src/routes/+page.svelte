<!--
  @agent-context: Root page — registers global hotkey, initializes theme, manages overlay visibility.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import Overlay from "$lib/components/Overlay.svelte";
  import { store, toggleOverlay, detectContext } from "$lib/stores/skills.svelte";
  import { initTheme } from "$lib/stores/theme.svelte";
  import type { AppConfig } from "$lib/types";

  const appWindow = getCurrentWindow();

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
      setupHotkey();
      setupEscapeKey();

      unlistenResized = await appWindow.onResized(() => {
        if (persistResizeTimer) clearTimeout(persistResizeTimer);
        persistResizeTimer = setTimeout(() => {
          void persistOverlaySize();
        }, 180);
      });

      // Always show on launch
      await showOverlay();
    };

    void init();

    return () => {
      if (persistResizeTimer) clearTimeout(persistResizeTimer);
      unlistenResized?.();
    };
  });

  async function showOverlay() {
    toggleOverlay();
    await appWindow.show();
    await appWindow.setFocus();
  }

  async function setupHotkey() {
    try {
      const config: AppConfig = await invoke("get_config");
      const hotkey = config.hotkey || "CommandOrControl+Shift+K";

      await unregister(hotkey).catch(() => {});
      await register(hotkey, async (event) => {
        if (event.state === "Pressed") {
          if (!store.isVisible) {
            // CRITICAL: detect terminal context NOW — before showing the overlay.
            // Once our window gains focus, GetForegroundWindow() returns us, not the terminal.
            await detectContext();
            toggleOverlay(true); // skip duplicate context detection inside toggleOverlay
            await appWindow.show();
            await appWindow.setFocus();
          } else {
            toggleOverlay();
            await appWindow.hide();
          }
        }
      });
    } catch (e) {
      console.warn("Hotkey registration failed:", e);
    }
  }

  function setupEscapeKey() {
    document.addEventListener("keydown", async (e) => {
      if (e.key === "Escape" && store.isVisible) {
        toggleOverlay();
        await appWindow.hide();
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
