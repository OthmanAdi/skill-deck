<!--
  @agent-context: Main overlay container for unified dark or light themes.
  Solid surface, clean border, no glassmorphism.
  Layout: title bar → search → tabs → grouped skill list → context bar.
  Keyboard: Arrow keys navigate rows across groups, Enter expands, Escape closes.
-->
<script lang="ts">
  import { fly, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    collapseAllAgentGroups,
    expandAllAgentGroups,
    refreshSkills,
    setFinderOpen,
    setSkillSortMode,
    skillSortOptions,
    store,
  } from "$lib/stores/skills.svelte";
  import SearchBar from "./SearchBar.svelte";
  import FacetBar from "./FacetBar.svelte";
  import TabBar from "./TabBar.svelte";
  import AgentGroup from "./AgentGroup.svelte";
  import SkillTree from "./SkillTree.svelte";
  import ContextBar from "./ContextBar.svelte";
  import Toast from "./Toast.svelte";
  import ThemeMenu from "./ThemeMenu.svelte";
  import FullSkillModal from "./FullSkillModal.svelte";
  import DiffModal from "./DiffModal.svelte";
  import RegistryTab from "./RegistryTab.svelte";
  import EmojiPickerPopover from "./EmojiPickerPopover.svelte";
  import ChatShell from "./chat/ChatShell.svelte";
  import Spinner from "./ui/Spinner.svelte";
  import Kbd from "./ui/Kbd.svelte";
  import { harnessStore, openChat } from "$lib/stores/harness.svelte";

  let focusedIndex = $state(-1);
  let listEl: HTMLDivElement | undefined = $state();
  let finderOpen = $state(false);
  let searchBarApi: { focusInput: () => void } | undefined = $state();

  // Spotlight-style recall: this overlay subtree unmounts on hide and remounts
  // on show, so a naive entrance replays the full staggered list every hotkey
  // press. Capture whether THIS mount is the first-ever reveal — a const, stable
  // for the mount's lifetime, so marking the list revealed below never yanks the
  // class off a stagger already in flight. Later recalls open instantly.
  const playEntrance = !store.listRevealed;
  $effect(() => {
    if (!store.listRevealed) store.listRevealed = true;
  });

  // First-run finder hint. Shown once until dismissed; the choice persists in
  // localStorage so the permanent shortcut bar doesn't tax every later session.
  const FINDER_HINT_KEY = "sd-hint-finder-dismissed";
  function readHintDismissed(): boolean {
    try {
      return localStorage.getItem(FINDER_HINT_KEY) === "1";
    } catch {
      return false;
    }
  }
  let finderHintDismissed = $state(readHintDismissed());
  function dismissFinderHint() {
    finderHintDismissed = true;
    try {
      localStorage.setItem(FINDER_HINT_KEY, "1");
    } catch {
      // Non-fatal: hint just reappears next launch if storage is unavailable.
    }
  }

  const activeFinderSignals = $derived(
    (store.searchQuery.trim() ? 1 : 0)
      + store.selectedTags.length
      + store.selectedUseCases.length
      + store.selectedArtifactTypes.length
  );

  // Reset keyboard focus only when the visible set MEANINGFULLY changes (tab,
  // view, search, active filters, or result count) — not on every array
  // reassignment. filteredSkills returns a fresh array on each read, so the
  // 30s background rescan used to wipe the user's arrow-key position even when
  // it returned identical skills (UI-02). This signal is a value-compared
  // string: an unchanged result set produces an unchanged signal, so the
  // effect does not re-run.
  const focusResetSignal = $derived(
    [
      store.activeTab,
      store.viewMode,
      store.searchQuery.trim(),
      store.selectedTags.join(","),
      store.selectedUseCases.join(","),
      store.selectedArtifactTypes.join(","),
      store.filteredSkills.length,
    ].join("|"),
  );

  $effect(() => {
    focusResetSignal; // track the value-compared signal, not array identity
    focusedIndex = -1;
  });

  function getVisibleOptionIndices(): number[] {
    if (!listEl) return [];
    const options = Array.from(listEl.querySelectorAll<HTMLElement>('[role="option"]'));
    return options
      .map((el) => Number.parseInt(el.dataset.index ?? "-1", 10))
      .filter((n) => Number.isFinite(n) && n >= 0);
  }

  function isTextEntryTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName.toLowerCase();
    return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (store.fullSkillModalOpen) {
      return;
    }

    if (e.key === "Escape" && finderOpen) {
      e.preventDefault();
      e.stopPropagation();
      finderOpen = false;
      setFinderOpen(false);
      return;
    }

    if (isTextEntryTarget(e.target)) {
      return;
    }

    const finderAllowed = store.activeTab !== "registry";

    const isFindShortcut = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f";
    const isSlashShortcut =
      !e.ctrlKey
      && !e.metaKey
      && !e.altKey
      && e.key === "/";

    if (finderAllowed && (isFindShortcut || isSlashShortcut)) {
      e.preventDefault();
      finderOpen = true;
      setFinderOpen(true);
      requestAnimationFrame(() => searchBarApi?.focusInput());
      return;
    }

    if (store.activeTab === "registry") {
      return;
    }

    const visible = getVisibleOptionIndices();
    const total = visible.length;
    if (total === 0) return;

    const pos = visible.indexOf(focusedIndex);

    if (e.key === "ArrowDown") {
      e.preventDefault();
      const nextPos = pos < 0 ? 0 : (pos + 1) % total;
      focusedIndex = visible[nextPos];
      scrollToFocused();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      const nextPos = pos < 0 ? total - 1 : (pos - 1 + total) % total;
      focusedIndex = visible[nextPos];
      scrollToFocused();
    } else if (e.key === "Home") {
      e.preventDefault();
      focusedIndex = visible[0];
      scrollToFocused();
    } else if (e.key === "End") {
      e.preventDefault();
      focusedIndex = visible[total - 1];
      scrollToFocused();
    } else if (e.key === "Enter" || e.key === " ") {
      // Native controls and skill rows own their activation keys. Intercepting
      // them here prevents button default clicks (Settings was unreachable by
      // keyboard) and can make a row toggle twice after its own handler runs.
      if (
        e.target instanceof HTMLElement
        && e.target.closest('button, a[href], [role="option"]')
      ) return;
      e.preventDefault();
      const row = listEl?.querySelector<HTMLElement>(`[data-index="${focusedIndex}"]`);
      row?.click();
      scrollToFocused();
    }
  }

  function scrollToFocused() {
    requestAnimationFrame(() => {
      const row = listEl?.querySelector<HTMLElement>(`[data-index="${focusedIndex}"]`);
      // Instant scroll, not smooth: holding an arrow key used to queue
      // overlapping smooth-scroll animations, which read as jank (UI-04).
      row?.scrollIntoView({ block: "nearest", behavior: "auto" });
      row?.focus();
    });
  }

  const overlayTransition = {
    y: 12,
    duration: 230, // --motion-enter
    opacity: 0,
    easing: cubicOut,
  };

  function toggleFinderPanel() {
    const next = !finderOpen;
    setFinderOpen(next);
    if (next) {
      requestAnimationFrame(() => searchBarApi?.focusInput());
    }
  }

  $effect(() => {
    finderOpen = store.finderOpen;
  });

  $effect(() => {
    if (store.activeTab === "registry" && finderOpen) {
      finderOpen = false;
      setFinderOpen(false);
    }
  });

  $effect(() => {
    if (finderOpen) {
      requestAnimationFrame(() => searchBarApi?.focusInput());
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
{#if store.isVisible}
  <div
    class="flex h-screen w-screen flex-col overflow-hidden"
    style="
      background: var(--color-surface-0);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-lg);
      box-shadow: 0 24px 48px -12px var(--color-overlay-shadow);
    "
    transition:fly={overlayTransition}
    onkeydown={handleKeydown}
  >
    <!-- Title bar / drag region -->
    <div
      class="overlay-command-strip shrink-0 border-b border-[var(--color-border)]"
      data-tauri-drag-region
    >
      <!-- Left: fanned-deck mark + wordmark -->
      <div class="overlay-brand flex items-center gap-2" data-tauri-drag-region>
        <svg
          class="h-5 w-5 shrink-0"
          viewBox="0 0 24 24"
          aria-hidden="true"
          data-tauri-drag-region
        >
          <!-- Three fanned skill cards: accent silhouette, surface-0 seams -->
          <g style="fill: var(--color-accent); stroke: var(--color-surface-0); stroke-width: 1.4; stroke-linejoin: round;">
            <rect x="3.5" y="9" width="8.5" height="12.5" rx="2" transform="rotate(-16 7.75 15.25)" />
            <rect x="12" y="9" width="8.5" height="12.5" rx="2" transform="rotate(16 16.25 15.25)" />
            <rect x="7.75" y="6.5" width="8.5" height="12.5" rx="2" />
          </g>
        </svg>
        <span
          class="font-display text-xs font-medium uppercase tracking-[0.16em] text-[var(--color-text-secondary)]"
          data-tauri-drag-region
        >Skill Deck</span>
      </div>

      <!-- Right: loading indicator + skill count + controls -->
        <div class="overlay-command-actions">
          {#if store.isLoading || store.registryLoading}
            <Spinner size={12} label="Scanning skills" />
          {/if}
        <span class="command-count mr-0.5 text-2xs tabular-nums text-[var(--color-text-muted)]">
          {store.activeTab === "registry" ? store.registryResultCount : store.skills.length}
        </span>
        {#if store.activeTab !== "registry"}
          <!-- View mode toggle + collapse/expand, one consolidated segment -->
          <div class="overlay-priority-group flex items-center rounded-md border"
            style="border-color: var(--color-border); background: var(--color-surface-1);">
            <button
              class="command-mode-control instant-tooltip flex items-center gap-1.5 rounded-sm text-2xs font-medium
                transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]
                {store.viewMode === 'grouped'
                  ? 'text-[var(--color-accent-text)] bg-[var(--color-surface-3)]'
                  : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
              onclick={() => (store.viewMode = "grouped")}
              data-tooltip="Grouped by coding agent"
              aria-label="Grouped list view"
              aria-pressed={store.viewMode === "grouped"}
            >
              <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 7h16M4 12h12M4 17h9" />
              </svg>
              <span class="command-label">Agents</span>
            </button>

            <button
              class="command-mode-control instant-tooltip flex items-center gap-1.5 rounded-sm text-2xs font-medium
                transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]
                {store.viewMode === 'tree'
                  ? 'text-[var(--color-accent-text)] bg-[var(--color-surface-3)]'
                  : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
              onclick={() => (store.viewMode = "tree")}
              data-tooltip="Parent child relation cards"
              aria-label="Parent child card view"
              aria-pressed={store.viewMode === "tree"}
            >
              <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M7 6h10M7 12h5M7 18h3M4 6h.01M4 12h.01M4 18h.01" />
              </svg>
              <span class="command-label">Cards</span>
            </button>

            {#if store.viewMode === 'grouped' && store.activeTab === 'all'}
              <button
                class="command-mode-control instant-tooltip flex items-center justify-center rounded-sm
                  text-[var(--color-text-muted)]
                  transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]
                  hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={() => collapseAllAgentGroups(store.groupedSkills.map((group) => group.agentId))}
                data-tooltip="Collapse all coding agent categories"
                aria-label="Collapse all coding agent categories"
              >
                <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 9l6 6 6-6" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M7 5h10" />
                </svg>
              </button>

              <button
                class="command-mode-control instant-tooltip flex items-center justify-center rounded-sm
                  text-[var(--color-text-muted)]
                  transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]
                  hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={expandAllAgentGroups}
                data-tooltip="Expand all coding agent categories"
                aria-label="Expand all coding agent categories"
              >
                <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 15l6-6 6 6" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M7 19h10" />
                </svg>
              </button>
            {/if}
          </div>
        {/if}

        {#if store.activeTab !== "registry"}
          <button
            class="command-rail-control instant-tooltip flex items-center gap-1.5 rounded-sm border text-2xs font-medium
              transition-[background-color,border-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]"
            style={finderOpen
              ? "border-color: var(--color-border-active); background: var(--color-accent-subtle); color: var(--color-accent-text);"
              : "border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-secondary);"}
            onclick={toggleFinderPanel}
            data-tooltip="Open finder, Ctrl+F or slash"
            aria-label="Toggle finder panel"
            aria-expanded={finderOpen}
            aria-controls="finder-panel"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
            <span class="command-label">Find</span>
            {#if activeFinderSignals > 0}
              <span
                class="rounded-xs px-1 text-2xs font-semibold tabular-nums"
                style="background: var(--color-surface-3); color: var(--color-text-primary);"
              >
                {activeFinderSignals}
              </span>
            {/if}
          </button>
        {/if}

        <button
          class="command-rail-control instant-tooltip flex items-center gap-1.5 rounded-sm border text-2xs font-medium
            transition-[background-color,border-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)] active:scale-[0.96]"
          style={harnessStore.activeProvider
            ? "border-color: var(--color-border-active); background: var(--color-accent-subtle); color: var(--color-accent-text);"
            : "border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-secondary);"}
          onclick={() => openChat(harnessStore.activeProvider ? "chat" : "settings")}
          data-tooltip={harnessStore.activeProvider ? "Open agent chat" : "Configure agent"}
          aria-label="Open agent chat"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 00-2.456 2.456z" />
          </svg>
          <span class="command-label">Agent</span>
        </button>

        <ThemeMenu />
      </div>
    </div>

    {#if !finderHintDismissed && !finderOpen && store.activeTab !== "registry"}
      <div class="shrink-0 px-3 pt-1" transition:slide={{ duration: 150, easing: cubicOut }}>
        <div
          class="flex items-center gap-1.5 rounded-sm border px-2 py-1 text-2xs text-[var(--color-text-muted)]"
          style="background: var(--color-surface-1); border-color: var(--color-border);"
        >
          <span class="shrink-0">Finder</span>
          <Kbd keys={["Ctrl", "F"]} />
          <span class="opacity-60">or</span>
          <Kbd>/</Kbd>
          <button
            class="ml-auto flex h-5 w-5 items-center justify-center rounded-xs text-[var(--color-text-muted)]
              transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
              hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)] active:scale-[0.96]"
            onclick={dismissFinderHint}
            aria-label="Dismiss finder hint"
          >
            <svg class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>
    {/if}

    {#if finderOpen && store.activeTab !== "registry"}
      <div
        id="finder-panel"
        class="shrink-0 px-3 pt-2.5 pb-2"
        transition:slide={{ duration: 150, easing: cubicOut }}
      >
        <div class="space-y-2 rounded-md border p-2"
          style="background: var(--color-surface-1); border-color: var(--color-border);">
          <SearchBar bind:this={searchBarApi} />
          <FacetBar compact />
          <div class="flex items-center gap-2">
            <label class="u-caps shrink-0" for="skill-sort-mode">Sort</label>
            <select
              id="skill-sort-mode"
              class="field flex-1"
              value={store.skillSortMode}
              onchange={(event) => {
                const mode = (event.currentTarget as HTMLSelectElement).value as
                  | "default"
                  | "installed-newest"
                  | "installed-oldest";
                void setSkillSortMode(mode);
              }}
            >
              {#each skillSortOptions as option (option.id)}
                <option value={option.id}>{option.label}</option>
              {/each}
            </select>
          </div>
        </div>
      </div>
    {/if}

    <!-- Tabs -->
    <div class="shrink-0 px-3 pb-2 {(finderOpen && store.activeTab !== 'registry') ? '' : 'pt-2.5'}">
      <TabBar />
    </div>

    {#if store.activeTab !== "registry" && store.scanErrorCount > 0}
      <div class="scan-partial-warning mx-3 mb-2" role="status">
        <svg class="h-3 w-3 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v4m0 4h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
        </svg>
        <span>{store.scanErrorCount} path{store.scanErrorCount === 1 ? "" : "s"} skipped during scan</span>
      </div>
    {/if}

      <!-- Skill list — grouped by agent -->
      <div
        class="skill-list flex-1 overflow-y-auto px-2 pb-2"
      role="listbox"
      aria-label={store.activeTab === "registry" ? "Registry skills" : "Skills"}
      tabindex="-1"
        bind:this={listEl}
      >
      {#if store.activeTab === "registry"}
        <div class="h-full py-1">
          <RegistryTab />
        </div>
      {:else if store.isLoading && store.skills.length === 0}
        <!-- Skeleton loading state -->
        <div class="flex flex-col gap-1.5 px-1 py-2">
          {#each Array(5) as _, i}
            <div class="skeleton-card flex items-center gap-3 px-2" style="height: 34px; animation-delay: {i * 100}ms;">
              <div class="h-2.5 w-2.5 rounded-full bg-[var(--color-surface-3)]"></div>
              <div class="h-2.5 flex-1 max-w-[140px] rounded-xs bg-[var(--color-surface-3)]"></div>
              <div class="h-2 w-8 rounded-xs bg-[var(--color-surface-3)] opacity-50 ml-auto"></div>
            </div>
          {/each}
        </div>

      {:else if store.scanError && store.filteredSkills.length === 0}
        <!-- Scan failure — a broken scan is not the same as zero skills. -->
        <div class="flex flex-col items-center justify-center py-16 text-center">
          <div
            class="mb-4 flex h-14 w-14 items-center justify-center"
            style="background: var(--color-error-subtle); border: 1px solid var(--color-error); border-radius: var(--radius-lg);"
          >
            <svg class="h-6 w-6 text-[var(--color-error)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.6" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round"
                d="M12 9v4m0 4h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
            </svg>
          </div>
          <p class="font-display text-lg font-medium text-[var(--color-text-primary)]">Scan failed</p>
          <p class="mt-1 max-w-[240px] text-2xs leading-relaxed text-[var(--color-text-muted)]">
            {store.scanError}
          </p>
          <button class="btn-secondary btn-sm mt-3" onclick={() => void refreshSkills()}>
            <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
            </svg>
            Retry scan
          </button>
        </div>

      {:else if store.filteredSkills.length === 0}
        <!-- Empty state — honest per tab / search context. -->
        <div class="flex flex-col items-center justify-center py-16 text-center">
          <div
            class="mb-4 flex h-14 w-14 items-center justify-center"
            style="background: var(--color-surface-1); border: 1px solid var(--color-border); border-radius: var(--radius-lg);"
          >
            {#if store.searchQuery}
              <svg class="h-6 w-6 text-[var(--color-text-muted)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
            {:else if store.activeTab === "starred"}
              <svg class="h-6 w-6 text-[var(--color-text-muted)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M11.48 3.5a.56.56 0 0 1 1.04 0l2.08 4.71 5.12.53c.5.05.7.68.32 1.02l-3.84 3.42 1.1 5.04c.1.49-.42.87-.85.62L12 16.9l-4.55 2.46c-.43.25-.95-.13-.85-.62l1.1-5.04-3.84-3.42a.56.56 0 0 1 .32-1.02l5.12-.53 2.08-4.71z" />
              </svg>
            {:else}
              <svg class="h-6 w-6 text-[var(--color-text-muted)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
              </svg>
            {/if}
          </div>
          <p class="font-display text-lg font-medium text-[var(--color-text-primary)]">
            {#if store.searchQuery}
              No matching skills
            {:else if store.activeTab === "starred"}
              No starred skills yet
            {:else}
              No skills found
            {/if}
          </p>
          <p class="mt-1 max-w-[240px] text-2xs leading-relaxed text-[var(--color-text-muted)]">
            {#if store.searchQuery}
              Try a different search or clear active filters
            {:else if store.activeTab === "starred"}
              Star any skill to pin it here
            {:else}
              Scanned {store.installedAgentCount} agent{store.installedAgentCount === 1 ? "" : "s"} — none expose skills yet. Add skills to an agent's skills/ or commands/ folder, then rescan.
            {/if}
          </p>
        </div>

      {:else if store.viewMode === "tree"}
        <!-- Card view: parent/child hierarchy by file path -->
        <div class="py-1 px-1">
          <SkillTree skills={store.filteredSkills} {focusedIndex} enter={playEntrance} />
        </div>
      {:else}
        <!-- Grouped view: skills bucketed by agent -->
        <div class="flex flex-col gap-0.5 py-1">
          {#each store.groupedSkills as group (group.agentId)}
            <AgentGroup
              agentId={group.agentId}
              skills={group.skills}
              startIndex={group.startIndex}
              {focusedIndex}
              enter={playEntrance}
            />
          {/each}
        </div>
      {/if}
    </div>

    <!-- Context bar -->
    <ContextBar />

    <!-- Toast -->
    <Toast />

    <!-- Full skill content modal -->
    <FullSkillModal />

    <!-- Archive diff / view dialog -->
    <DiffModal />

    <!-- Speaking-in-Skills agent overlay -->
    <ChatShell />

    <!-- Singleton emoji picker — only ever one open at a time. -->
    <EmojiPickerPopover />
  </div>
{/if}
