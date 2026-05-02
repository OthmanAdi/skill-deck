<!--
  @agent-context: Main overlay container for unified dark or light themes.
  Solid surface, clean border, no glassmorphism.
  Layout: title bar → search → tabs → grouped skill list → context bar.
  Keyboard: Arrow keys navigate rows across groups, Enter expands, Escape closes.
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    collapseAllAgentGroups,
    expandAllAgentGroups,
    store,
    refreshSkills,
  } from "$lib/stores/skills.svelte";
  import SearchBar from "./SearchBar.svelte";
  import TabBar from "./TabBar.svelte";
  import AgentGroup from "./AgentGroup.svelte";
  import SkillTree from "./SkillTree.svelte";
  import SkillGraphView from "./SkillGraphView.svelte";
  import ContextBar from "./ContextBar.svelte";
  import Toast from "./Toast.svelte";
  import ThemeMenu from "./ThemeMenu.svelte";

  let focusedIndex = $state(-1);
  let listEl: HTMLDivElement | undefined = $state();

  // Initial scan on mount
  $effect(() => {
    refreshSkills();
  });

  // Reset focus index when filtered skills change
  $effect(() => {
    store.filteredSkills; // track dependency
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
    if (store.viewMode === "graph") {
      return;
    }

    if (isTextEntryTarget(e.target)) {
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
      e.preventDefault();
      const row = listEl?.querySelector<HTMLElement>(`[data-index="${focusedIndex}"]`);
      row?.click();
      scrollToFocused();
    }
  }

  function scrollToFocused() {
    requestAnimationFrame(() => {
      const row = listEl?.querySelector<HTMLElement>(`[data-index="${focusedIndex}"]`);
      row?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      row?.focus();
    });
  }

  const overlayTransition = {
    y: 12,
    duration: 220,
    opacity: 0,
    easing: cubicOut,
  };
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
      class="flex shrink-0 items-center justify-between px-3 py-2.5 border-b border-[var(--color-border)]"
      data-tauri-drag-region
    >
      <!-- Left: logo + name -->
      <div class="flex items-center gap-2" data-tauri-drag-region>
        <div
          class="flex h-5 w-5 shrink-0 items-center justify-center rounded"
          style="background: var(--color-accent); border-radius: var(--radius-sm);"
        >
          <svg class="h-3 w-3" viewBox="0 0 16 16" fill="currentColor"
            style="color: var(--color-surface-0);">
            <path d="M2 3a1 1 0 011-1h10a1 1 0 011 1v2a1 1 0 01-1 1H3a1 1 0 01-1-1V3zm0 5a1 1 0 011-1h6a1 1 0 011 1v2a1 1 0 01-1 1H3a1 1 0 01-1-1V8zm0 5a1 1 0 011-1h8a1 1 0 011 1v1a1 1 0 01-1 1H3a1 1 0 01-1-1v-1z"/>
          </svg>
        </div>
        <span
          class="text-[11px] font-semibold tracking-[0.07em] uppercase text-[var(--color-text-muted)]"
          data-tauri-drag-region
        >Skill Deck</span>
      </div>

      <!-- Right: loading indicator + skill count + theme menu -->
      <div class="flex items-center gap-2.5">
        {#if store.isLoading}
          <span class="h-3 w-3 rounded-full border-[1.5px] border-[var(--color-accent)] border-t-transparent spin"></span>
        {/if}
        <span class="text-[10px] tabular-nums text-[var(--color-text-muted)]">
          {store.skills.length}
        </span>
        <!-- View mode toggle: grouped / tree / graph -->
        <div class="flex items-center rounded-md border p-0.5"
          style="border-color: var(--color-border); background: var(--color-surface-1);">
          <button
            class="instant-tooltip flex h-6 items-center gap-1.5 rounded px-2 text-[10px] font-medium transition-all duration-150
              {store.viewMode === 'grouped'
                ? 'text-[var(--color-accent)] bg-[var(--color-surface-3)]'
                : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
            onclick={() => (store.viewMode = "grouped")}
            data-tooltip="Grouped by coding agent"
            aria-label="Grouped list view"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 7h16M4 12h12M4 17h9" />
            </svg>
            <span>Agents</span>
          </button>

          <button
            class="instant-tooltip flex h-6 items-center gap-1.5 rounded px-2 text-[10px] font-medium transition-all duration-150
              {store.viewMode === 'tree'
                ? 'text-[var(--color-accent)] bg-[var(--color-surface-3)]'
                : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
            onclick={() => (store.viewMode = "tree")}
            data-tooltip="Parent child relation tree"
            aria-label="Parent child tree view"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M7 6h10M7 12h5M7 18h3M4 6h.01M4 12h.01M4 18h.01" />
            </svg>
            <span>Tree</span>
          </button>

          <button
            class="instant-tooltip flex h-6 items-center gap-1.5 rounded px-2 text-[10px] font-medium transition-all duration-150
              {store.viewMode === 'graph'
                ? 'text-[var(--color-accent)] bg-[var(--color-surface-3)]'
                : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
            onclick={() => (store.viewMode = "graph")}
            data-tooltip="Graph relationship view"
            aria-label="Graph view"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <circle cx="5" cy="8" r="2"></circle>
              <circle cx="12" cy="5" r="2"></circle>
              <circle cx="18" cy="10" r="2"></circle>
              <circle cx="9" cy="17" r="2"></circle>
              <path stroke-linecap="round" stroke-linejoin="round" d="M6.8 7l3.4-1M13.8 5.8l2.4 2.6M16.4 11.5l-6 4" />
            </svg>
            <span>Graph</span>
          </button>

          {#if store.viewMode === 'grouped' && store.activeTab === 'all'}
            <span class="mx-0.5 h-4 w-px bg-[var(--color-border)] opacity-80"></span>

            <button
              class="instant-tooltip flex h-6 items-center gap-1.5 rounded px-2 text-[10px] font-medium
                text-[var(--color-text-muted)] transition-all duration-150 hover:text-[var(--color-text-secondary)]"
              onclick={() => collapseAllAgentGroups(store.groupedSkills.map((group) => group.agentId))}
              data-tooltip="Collapse all coding agent categories"
              aria-label="Collapse all coding agent categories"
            >
              <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M6 9l6 6 6-6" />
                <path stroke-linecap="round" stroke-linejoin="round" d="M7 5h10" />
              </svg>
              <span>Collapse</span>
            </button>

            <button
              class="instant-tooltip flex h-6 items-center gap-1.5 rounded px-2 text-[10px] font-medium
                text-[var(--color-text-muted)] transition-all duration-150 hover:text-[var(--color-text-secondary)]"
              onclick={expandAllAgentGroups}
              data-tooltip="Expand all coding agent categories"
              aria-label="Expand all coding agent categories"
            >
              <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M6 15l6-6 6 6" />
                <path stroke-linecap="round" stroke-linejoin="round" d="M7 19h10" />
              </svg>
              <span>Expand</span>
            </button>
          {/if}
        </div>
        <ThemeMenu />
      </div>
    </div>

    <!-- Search -->
    <div class="shrink-0 px-3 pt-2.5 pb-2">
      <SearchBar />
    </div>

    <!-- Tabs -->
    <div class="shrink-0 px-3 pb-2">
      <TabBar />
    </div>

    <!-- Skill list — grouped by agent -->
    <div
      class="skill-list flex-1 overflow-y-auto px-2 pb-2"
      role="listbox"
      aria-label="Skills"
      tabindex="-1"
      bind:this={listEl}
      class:overflow-hidden={store.viewMode === "graph"}
    >
      {#if store.isLoading && store.skills.length === 0}
        <!-- Skeleton loading state -->
        <div class="flex flex-col gap-1.5 px-1 py-2">
          {#each Array(5) as _, i}
            <div class="skeleton-card flex items-center gap-3 px-2" style="height: 34px; animation-delay: {i * 100}ms;">
              <div class="h-2.5 w-2.5 rounded-full bg-[var(--color-surface-3)]"></div>
              <div class="h-2.5 flex-1 max-w-[140px] rounded bg-[var(--color-surface-3)]"></div>
              <div class="h-2 w-8 rounded bg-[var(--color-surface-3)] opacity-50 ml-auto"></div>
            </div>
          {/each}
        </div>

      {:else if store.filteredSkills.length === 0}
        <!-- Empty state -->
        <div class="flex flex-col items-center justify-center py-20 text-center">
          <div
            class="mb-4 flex h-14 w-14 items-center justify-center"
            style="background: var(--color-surface-1); border: 1px solid var(--color-border); border-radius: var(--radius-lg);"
          >
            {#if store.searchQuery}
              <svg class="h-6 w-6 text-[var(--color-text-muted)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
            {:else}
              <svg class="h-6 w-6 text-[var(--color-text-muted)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
              </svg>
            {/if}
          </div>
          <p class="text-[12px] font-medium text-[var(--color-text-secondary)]">
            {store.searchQuery ? "No matching skills" : "No skills found"}
          </p>
          <p class="mt-1 max-w-[220px] text-[10px] leading-relaxed text-[var(--color-text-muted)]">
            {store.searchQuery
              ? "Try a different search or clear the filter"
              : "Install a coding agent or add skills to ~/.claude/skills/"}
          </p>
        </div>

      {:else if store.viewMode === "tree"}
        <!-- Tree view: parent/child hierarchy by file path -->
        <div class="py-1 px-1">
          <SkillTree skills={store.filteredSkills} {focusedIndex} />
        </div>
      {:else if store.viewMode === "graph"}
        <!-- Graph view: relationship network -->
        <div class="h-full py-1 px-1">
          <SkillGraphView skills={store.filteredSkills} />
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
            />
          {/each}
        </div>
      {/if}
    </div>

    <!-- Drag-over-terminal banner -->
    {#if store.dragOverTerminal}
      <div
        class="shrink-0 flex items-center justify-center gap-2 py-1.5 text-[10px] font-medium"
        style="background: var(--color-accent-subtle); border-top: 1px solid var(--color-accent-muted); color: var(--color-accent);"
      >
        <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M8 9l4-4 4 4m0 6l-4 4-4-4" />
        </svg>
        Drop to inject into terminal
      </div>
    {/if}

    <!-- Context bar -->
    <ContextBar />

    <!-- Toast -->
    <Toast />
  </div>
{/if}
