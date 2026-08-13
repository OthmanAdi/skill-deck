<!--
  @agent-context: Settings popover triggered by the gear .btn-icon in the title
  bar. Four sections (theme, window behavior, project locations, global hotkey
  capture) in one
  .layer-popover dropdown with the shared .popover-enter entrance; it is
  height-clamped + scrolls so it can never run off the window bottom. Active
  theme / mode carry a checkmark; click to switch instantly. Positioning stays
  CSS-absolute (anchored below-right of the trigger) rather than the
  utils/position.ts fixed-panel util, which targets viewport-flipping popovers,
  not a simple anchored dropdown — see needs_from_integrator.
-->
<script lang="ts">
  import { themeStore, setTheme, THEMES, type ThemeId } from "$lib/stores/theme.svelte";
  import {
    store,
    setHotkey,
    setOverlayMode,
    setProjectRoots,
    type OverlayMode,
  } from "$lib/stores/skills.svelte";

  const DEFAULT_HOTKEY = "CommandOrControl+Shift+K";

  let isOpen = $state(false);
  let menuEl: HTMLDivElement | undefined = $state();
  let triggerEl: HTMLButtonElement | undefined = $state();

  // Press-to-capture shortcut UI. ONE inset field that renders the active
  // binding when idle and live-renders the held modifiers / main key while
  // recording. Auto-saves the moment a non-modifier key fires — no Save
  // button to click. Esc or focus loss cancels.
  let recording = $state(false);
  let recordingMods = $state<Set<string>>(new Set());
  let pendingCombo = $state<string | null>(null);
  let captureError = $state<string | null>(null);
  let saving = $state(false);
  let projectPath = $state("");

  // Tauri's accelerator strings use these exact tokens. CommandOrControl maps
  // to Ctrl on Win/Linux and Cmd on macOS so the persisted accelerator is
  // cross-platform; we only translate to "Ctrl"/"Cmd"/"⌘" for display.
  function modifierTokensFromEvent(e: KeyboardEvent): string[] {
    const mods: string[] = [];
    if (e.ctrlKey || e.metaKey) mods.push("CommandOrControl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    return mods;
  }

  function keyTokenFromEvent(e: KeyboardEvent): string | null {
    // Modifier-only keys don't qualify as the main key.
    if (["Control", "Shift", "Alt", "Meta", "OS", "ContextMenu"].includes(e.key)) {
      return null;
    }
    const named = new Set([
      "Tab", "Enter", "Backspace", "Delete", "Escape", "Insert", "Home", "End",
      "PageUp", "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
    ]);
    if (named.has(e.key)) return e.key;
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.key)) return e.key;
    if (e.key === " ") return "Space";
    if (/^[a-zA-Z]$/.test(e.key)) return e.key.toUpperCase();
    if (/^[0-9]$/.test(e.key)) return e.key;
    const punct: Record<string, string> = {
      "[": "BracketLeft", "]": "BracketRight", ",": "Comma", ".": "Period",
      "/": "Slash", "\\": "Backslash", "`": "Backquote", "-": "Minus",
      "=": "Equal", ";": "Semicolon", "'": "Quote",
    };
    return punct[e.key] ?? null;
  }

  // Display label for a single Tauri accelerator token. Short, native-style
  // ("Ctrl" not "CommandOrControl", "↑" not "ArrowUp", "F12" verbatim).
  // On macOS we'd render ⌘ / ⌥ / ⇧, but the app ships Windows-first so the
  // word labels read more clearly to the average user.
  function labelForToken(token: string): string {
    switch (token) {
      case "CommandOrControl":
      case "Control":
        return "Ctrl";
      case "Command":
        return "Cmd";
      case "Alt":
        return "Alt";
      case "Shift":
        return "Shift";
      case "Space":
        return "Space";
      case "Enter":
        return "Enter";
      case "Backspace":
        return "Backspace";
      case "Escape":
        return "Esc";
      case "ArrowUp":
        return "↑";
      case "ArrowDown":
        return "↓";
      case "ArrowLeft":
        return "←";
      case "ArrowRight":
        return "→";
      case "BracketLeft":
        return "[";
      case "BracketRight":
        return "]";
      case "Comma":
        return ",";
      case "Period":
        return ".";
      case "Slash":
        return "/";
      case "Backslash":
        return "\\";
      case "Backquote":
        return "`";
      case "Minus":
        return "-";
      case "Equal":
        return "=";
      case "Semicolon":
        return ";";
      case "Quote":
        return "'";
      default:
        return token;
    }
  }

  // Render an accelerator string as an ordered list of display labels.
  // Reorders modifiers into the conventional Ctrl → Alt → Shift → key flow
  // so two bindings that differ only in input order display identically.
  function comboLabels(accel: string | null | undefined): string[] {
    if (!accel) return [];
    const parts = accel.split("+").map((p) => p.trim()).filter(Boolean);
    const order = (t: string) =>
      t === "CommandOrControl" || t === "Control" ? 0
      : t === "Command" ? 1
      : t === "Alt" ? 2
      : t === "Shift" ? 3
      : 4;
    const sorted = [...parts].sort((a, b) => order(a) - order(b));
    return sorted.map(labelForToken);
  }

  // The labels rendered inside the field — live during capture, persisted
  // binding otherwise. Recording with no modifier held shows a blinking
  // placeholder so the user knows the app is listening.
  const liveLabels = $derived(() => {
    if (!recording) {
      return comboLabels(store.hotkey || DEFAULT_HOTKEY);
    }
    if (pendingCombo) return comboLabels(pendingCombo);
    const mods = [...recordingMods];
    const order = (t: string) =>
      t === "CommandOrControl" ? 0 : t === "Alt" ? 1 : t === "Shift" ? 2 : 3;
    return mods.sort((a, b) => order(a) - order(b)).map(labelForToken);
  });

  async function commitCombo(combo: string) {
    saving = true;
    captureError = null;
    const prev = store.hotkey;
    try {
      await setHotkey(combo);
    } catch (e) {
      captureError = e instanceof Error ? e.message : String(e);
      if (store.hotkey !== prev) store.hotkey = prev;
    } finally {
      saving = false;
      stopRecording();
    }
  }

  function startRecording() {
    if (recording) return;
    recordingMods = new Set();
    pendingCombo = null;
    captureError = null;
    recording = true;
  }

  function stopRecording() {
    recording = false;
    recordingMods = new Set();
    pendingCombo = null;
  }

  function resetHotkey() {
    void commitCombo(DEFAULT_HOTKEY);
  }

  function handleWindowClick(e: MouseEvent) {
    if (!isOpen || !menuEl) return;
    // If the click landed inside our menu container OR on the trigger button,
    // leave the menu open. Use `mouseEvent.composedPath()` first because the
    // click target may already have been detached from the DOM if its onclick
    // re-rendered (e.g. a state transition removed the button). composedPath
    // captures the dispatch path at event-fire time, before any re-render.
    const path = e.composedPath?.() as Node[] | undefined;
    if (path && path.some((n) => n === menuEl)) return;
    if (menuEl.contains(e.target as Node)) return;
    if (recording) stopRecording();
    isOpen = false;
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (!isOpen) return;
    if (recording) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        stopRecording();
        return;
      }
      e.preventDefault();
      e.stopPropagation();
      recordingMods = new Set(modifierTokensFromEvent(e));
      const mainKey = keyTokenFromEvent(e);
      if (mainKey) {
        const mods = [...recordingMods];
        if (mods.length === 0) {
          captureError = "Hold Ctrl, Alt or Shift before pressing the key.";
          return;
        }
        // Stable canonical order — backend's normalize_hotkey re-orders too,
        // but emitting consistent strings keeps the round-trip identical.
        const order = (t: string) =>
          t === "CommandOrControl" ? 0 : t === "Alt" ? 1 : t === "Shift" ? 2 : 3;
        mods.sort((a, b) => order(a) - order(b));
        pendingCombo = [...mods, mainKey].join("+");
        void commitCombo(pendingCombo);
      }
      return;
    }
    if (e.key === "Escape") {
      isOpen = false;
      triggerEl?.focus();
    }
  }

  function handleWindowKeyup(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    // Mirror modifier release in the chips. We never commit on keyup — only
    // the main keydown locks the combo.
    recordingMods = new Set(modifierTokensFromEvent(e));
  }

  function openMenu() {
    isOpen = true;
  }

  function toggle(e: MouseEvent) {
    e.stopPropagation();
    if (isOpen) {
      if (recording) stopRecording();
      isOpen = false;
      return;
    }
    openMenu();
  }

  // Stop clicks INSIDE the dropdown from bubbling to the window-level
  // outside-click handler. Without this, any button click that re-renders
  // and removes its own DOM node makes window.click see a detached target
  // (menuEl.contains returns false) and the menu collapses immediately.
  function stopInsideClick(e: MouseEvent) {
    e.stopPropagation();
  }

  function selectTheme(themeId: ThemeId) {
    setTheme(themeId);
    isOpen = false;
  }

  function selectOverlayMode(mode: OverlayMode) {
    void setOverlayMode(mode);
  }

  async function addProjectRoot(e: SubmitEvent) {
    e.preventDefault();
    const candidate = projectPath.trim();
    if (!candidate || store.projectRootsSaving) return;

    const saved = await setProjectRoots([...store.projectRoots, candidate]);
    if (saved) projectPath = "";
  }

  function removeProjectRoot(root: string) {
    if (store.projectRootsSaving) return;
    void setProjectRoots(store.projectRoots.filter((candidate) => candidate !== root));
  }
</script>

<svelte:window
  onclick={handleWindowClick}
  onkeydown={handleWindowKeydown}
  onkeyup={handleWindowKeyup}
/>

<div class="relative" bind:this={menuEl}>
  <!-- Trigger: gear icon -->
  <button
    bind:this={triggerEl}
    class="btn-icon"
    onclick={toggle}
    aria-label="Settings"
    aria-expanded={isOpen}
    aria-haspopup="dialog"
  >
    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round"
        d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
      <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
    </svg>
  </button>

  <!-- Dropdown menu -->
  {#if isOpen}
    <div
      role="dialog"
      aria-label="Settings"
      tabindex="-1"
      onclick={stopInsideClick}
      onkeydown={(e) => { /* dropdown owns its own keys; window handler reads them */ if (e.key === "Tab") return; }}
      class="layer-popover popover-enter absolute right-0 top-full z-50 mt-1.5 w-[292px] py-1
        max-h-[calc(100vh-52px)] overflow-x-hidden overflow-y-auto overscroll-contain"
      style="--popover-origin: top right;"
    >
      <!-- Section label -->
      <div class="u-caps px-3 py-1.5">Theme</div>

      <div class="mx-1 border-t border-[var(--color-border)] mb-1"></div>

      {#each THEMES as theme (theme.id)}
        {@const isActive = themeStore.currentThemeId === theme.id}
        <button
          class="flex w-full items-center gap-2.5 rounded-[var(--radius-sm)] mx-1 px-2 py-2
            text-left
            transition-[background-color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
            hover:bg-[var(--color-surface-2)]"
          style="width: calc(100% - 8px);"
          onclick={() => selectTheme(theme.id)}
        >
          <!-- Checkmark / placeholder -->
          <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
            {#if isActive}
              <svg class="h-3.5 w-3.5 text-[var(--color-accent)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
              </svg>
            {/if}
          </span>

          <div class="min-w-0">
            <div class="text-sm font-medium
              {isActive ? 'text-[var(--color-text-primary)]' : 'text-[var(--color-text-secondary)]'}">
              {theme.name}
            </div>
            <div class="text-2xs text-[var(--color-text-muted)] leading-snug">
              {theme.description}
            </div>
          </div>

          <!-- Color-scheme indicator dot. These swatches must be theme-INVARIANT
               (a dark swatch stays dark even while the app is in light mode), so
               they cannot use --color-surface-* which flip with the active theme.
               They resolve the fixed --color-swatch-dark / --color-swatch-light
               tokens from app.css @theme (the sanctioned home for these values). -->
          <span
            class="instant-tooltip ml-auto h-2 w-2 shrink-0 rounded-full border border-[var(--color-border)]"
            style="background: {theme.id === 'system' ? 'linear-gradient(135deg, var(--color-swatch-dark) 50%, var(--color-swatch-light) 50%)' : theme.colorScheme === 'dark' ? 'var(--color-swatch-dark)' : 'var(--color-swatch-light)'};"
            data-tooltip="Color scheme: {theme.colorScheme}"
          ></span>
        </button>
      {/each}

      <div class="mx-1 mt-1 border-t border-[var(--color-border)]"></div>

      <div class="u-caps px-3 py-1.5">Window behavior</div>

      {#each ([
        { id: "pinned", label: "Pinned mode", desc: "Stays open until you hide it" },
        { id: "auto-hide", label: "Auto-hide mode", desc: "Hides when focus leaves window" }
      ] as const) as mode (mode.id)}
        {@const active = store.overlayMode === mode.id}
        <button
          class="flex w-full items-center gap-2.5 rounded-[var(--radius-sm)] mx-1 px-2 py-2
            text-left
            transition-[background-color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
            hover:bg-[var(--color-surface-2)]"
          style="width: calc(100% - 8px);"
          onclick={() => selectOverlayMode(mode.id)}
        >
          <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
            {#if active}
              <svg class="h-3.5 w-3.5 text-[var(--color-accent)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
              </svg>
            {/if}
          </span>

          <div class="min-w-0">
            <div class="text-sm font-medium
              {active ? 'text-[var(--color-text-primary)]' : 'text-[var(--color-text-secondary)]'}">
              {mode.label}
            </div>
            <div class="text-2xs text-[var(--color-text-muted)] leading-snug">
              {mode.desc}
            </div>
          </div>
        </button>
      {/each}

      <div class="mx-1 mt-1 border-t border-[var(--color-border)]"></div>

      <div class="u-caps px-3 py-1.5">Project locations</div>

      <div class="px-3 pb-3">
        <p class="text-2xs leading-snug text-[var(--color-text-muted)]">
          Paths stay on this device. Skill Deck checks known agent folders plus these projects, never an entire drive.
        </p>

        {#if store.projectRoots.length > 0}
          <ul class="mt-2 space-y-1" aria-label="Configured project locations">
            {#each store.projectRoots as root (root)}
              <li
                class="flex min-h-10 items-center gap-1 rounded-sm border border-[var(--color-border)]
                  bg-[var(--color-surface-2)] pl-2"
              >
                <span
                  class="min-w-0 flex-1 truncate font-mono text-2xs text-[var(--color-text-secondary)]"
                  title={root}
                >
                  {root}
                </span>
                <button
                  type="button"
                  class="min-h-10 shrink-0 rounded-r-sm px-2 text-2xs font-medium
                    text-[var(--color-text-muted)]
                    transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
                    hover:bg-[var(--color-surface-3)] hover:text-[var(--color-error)] active:scale-[0.96]
                    disabled:cursor-not-allowed disabled:opacity-50"
                  onclick={() => removeProjectRoot(root)}
                  disabled={store.projectRootsSaving}
                  aria-label={`Remove project location ${root}`}
                >
                  Remove
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="mt-2 text-2xs italic text-[var(--color-text-muted)]">
            No project folders added.
          </p>
        {/if}

        <form class="mt-2 flex items-stretch gap-1" onsubmit={addProjectRoot}>
          <label class="sr-only" for="project-root-path">Project folder path</label>
          <input
            id="project-root-path"
            type="text"
            class="min-h-10 min-w-0 flex-1 rounded-sm border border-[var(--color-border)]
              bg-[var(--color-surface-2)] px-2 font-mono text-2xs text-[var(--color-text-primary)]
              outline-none transition-[border-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
              placeholder:text-[var(--color-text-muted)]
              focus:border-[var(--color-accent)] focus:shadow-[0_0_0_3px_var(--color-accent-subtle)]
              disabled:cursor-not-allowed disabled:opacity-50"
            bind:value={projectPath}
            placeholder="Paste a project folder path"
            autocomplete="off"
            spellcheck="false"
            disabled={store.projectRootsSaving}
          />
          <button
            type="submit"
            class="min-h-10 shrink-0 rounded-sm border border-[var(--color-border)] px-2
              text-2xs font-semibold text-[var(--color-text-secondary)]
              transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
              hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-primary)] active:scale-[0.96]
              disabled:cursor-not-allowed disabled:opacity-50"
            disabled={store.projectRootsSaving || projectPath.trim().length === 0}
            aria-label="Add project location and rescan"
          >
            {store.projectRootsSaving ? "Saving…" : "Add"}
          </button>
        </form>
      </div>

      <div class="mx-1 mt-1 border-t border-[var(--color-border)]"></div>

      <div class="u-caps px-3 py-1.5">Shortcut</div>

      <div class="px-3 pb-3">
        <!-- Single inset field. Idle: shows the bound combo. Recording:
             renders live key chips as the user holds modifiers + presses
             the main key. Auto-saves on main key press; Esc cancels. -->
        <button
          type="button"
          class="group flex w-full items-center gap-1.5 rounded-sm border px-2 py-1.5
            text-left transition-[border-color,background-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
            {recording
              ? 'border-[var(--color-accent)] bg-[var(--color-accent-subtle)] shadow-[0_0_0_3px_var(--color-accent-subtle)]'
              : 'border-[var(--color-border)] bg-[var(--color-surface-2)] hover:border-[var(--color-border-active)] hover:bg-[var(--color-surface-3)]'}
            cursor-text min-h-[28px]"
          onclick={() => (recording ? stopRecording() : startRecording())}
          aria-label="Shortcut binding — click to record"
        >
          {#each liveLabels() as label, i (label + i)}
            {#if i > 0}
              <span class="text-2xs font-medium text-[var(--color-text-muted)] select-none">
                +
              </span>
            {/if}
            <span
              class="inline-flex items-center justify-center rounded-xs px-1.5 py-0.5
                text-2xs font-semibold tracking-wide
                border bg-[var(--color-surface-1)] tabular-nums
                {recording
                  ? 'border-[var(--color-accent)] text-[var(--color-accent-text)]'
                  : 'border-[var(--color-border)] text-[var(--color-text-primary)]'}"
            >
              {label}
            </span>
          {/each}
          {#if recording && liveLabels().length === 0}
            <span class="text-2xs italic text-[var(--color-accent-text)] u-pulse">
              Hold Ctrl, Alt or Shift…
            </span>
          {/if}
          {#if !recording}
            <span class="ml-auto text-2xs uppercase tracking-[0.06em] text-[var(--color-text-muted)] group-hover:text-[var(--color-text-secondary)]">
              edit
            </span>
          {/if}
        </button>

        <div class="mt-1.5 flex items-center justify-between gap-2">
          <p class="text-2xs text-[var(--color-text-muted)] leading-snug">
            {#if recording}
              Press the key to save · Esc to cancel
            {:else}
              Click the field, then press your keys
            {/if}
          </p>
          {#if !recording}
            <button
              type="button"
              class="rounded-xs border px-1.5 py-0.5 text-2xs uppercase tracking-[0.06em]
                border-[var(--color-border)] text-[var(--color-text-muted)]
                transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
                hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)] active:scale-[0.96]"
              onclick={resetHotkey}
              title="Reset to default (Ctrl + Shift + K)"
              disabled={saving}
            >
              Reset
            </button>
          {/if}
        </div>

        {#if captureError}
          <p class="mt-1.5 text-2xs text-[var(--color-error)] leading-snug">
            {captureError}
          </p>
        {/if}
      </div>
    </div>
  {/if}
</div>
