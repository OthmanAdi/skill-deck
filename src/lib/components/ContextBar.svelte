<!--
  @agent-context: Status bar at bottom — CWD, scan stats, refresh button.
-->
<script lang="ts">
  import { store, refreshSkills } from "$lib/stores/skills.svelte";

  let isRefreshing = $state(false);

  function shortenPath(path: string): string {
    const match = path.match(/^(\/home\/\w+|\/Users\/\w+|[A-Z]:\\Users\\\w+)/)?.[0];
    return match ? path.replace(match, "~") : path;
  }

  async function handleRefresh() {
    isRefreshing = true;
    await refreshSkills();
    isRefreshing = false;
  }

  const timeSinceScan = $derived.by(() => {
    if (!store.lastScanTime) return "";
    const elapsed = Math.round((Date.now() - store.lastScanTime) / 1000);
    if (elapsed < 5)  return "just now";
    if (elapsed < 60) return `${elapsed}s ago`;
    return `${Math.round(elapsed / 60)}m ago`;
  });
</script>

<div
  class="flex shrink-0 items-center justify-between gap-2 px-3 py-1.5 border-t border-[var(--color-border)]"
  style="background: var(--color-surface-2);"
>
  <!-- CWD -->
  <div class="flex items-center gap-1.5 truncate">
    {#if store.terminalContext.cwd}
      <span class="relative flex h-[6px] w-[6px] shrink-0">
        <span class="absolute inset-0 rounded-full bg-[var(--color-accent)] opacity-30"
          style="animation: skeleton-shimmer 2s ease-in-out infinite;"></span>
        <span class="relative h-[6px] w-[6px] rounded-full bg-[var(--color-accent)]"></span>
      </span>
      <span
        class="truncate font-mono text-[9.5px] text-[var(--color-text-secondary)] opacity-90"
        title={store.terminalContext.cwd}
      >
        {shortenPath(store.terminalContext.cwd)}
      </span>
    {:else}
      <span class="h-[6px] w-[6px] shrink-0 rounded-full bg-[var(--color-text-muted)] opacity-40"></span>
      <span class="text-[9.5px] text-[var(--color-text-secondary)] opacity-90">No terminal</span>
    {/if}
  </div>

  <!-- Stats + refresh -->
  <div class="flex shrink-0 items-center gap-1.5 text-[9.5px] tabular-nums text-[var(--color-text-secondary)]">
    <span class="opacity-80">{store.scanDurationMs}ms</span>
    {#if timeSinceScan}
      <span class="opacity-50">·</span>
      <span class="opacity-75">{timeSinceScan}</span>
    {/if}
    <button
      class="ml-0.5 flex h-5 w-5 items-center justify-center border border-transparent
        text-[var(--color-text-secondary)] opacity-80
        transition-all duration-[120ms] ease-out
        hover:border-[var(--color-border)] hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-primary)] hover:opacity-100
        active:scale-90"
      style="border-radius: var(--radius-sm);"
      onclick={handleRefresh}
      title="Refresh (re-detect CWD)"
      aria-label="Refresh skills"
    >
      <svg
        class="h-3 w-3 {isRefreshing ? 'spin' : ''}"
        fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
      >
        <path stroke-linecap="round" stroke-linejoin="round"
          d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
      </svg>
    </button>
  </div>
</div>
