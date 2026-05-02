<!--
  @agent-context: Main overlay container for unified dark or light themes.
  Solid surface, clean border, no glassmorphism.
  Layout: title bar → search → tabs → grouped skill list → context bar.
  Keyboard: Arrow keys navigate rows across groups, Enter expands, Escape closes.
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { store, refreshSkills } from "$lib/stores/skills.svelte";
  import SearchBar from "./SearchBar.svelte";
  import TabBar from "./TabBar.svelte";
  import AgentGroup from "./AgentGroup.svelte";
  import SkillTree from "./SkillTree.svelte";
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
        <!-- Tree mode toggle -->
        <button
          class="flex h-5 w-5 items-center justify-center rounded transition-all duration-150
            {store.treeMode
              ? 'text-[var(--color-accent)]'
              : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
          onclick={() => (store.treeMode = !store.treeMode)}
          title="{store.treeMode ? 'Switch to grouped view' : 'Switch to tree view'}"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round"
              d="M3 6h18M3 12h12M3 18h6" />
          </svg>
        </button>
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

      {:else if store.treeMode}
        <!-- Tree view: parent/child hierarchy by file path -->
        <div class="py-1 px-1">
          <SkillTree skills={store.filteredSkills} {focusedIndex} />
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
