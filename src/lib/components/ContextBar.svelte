<!--
  @agent-context: Status bar at bottom — CWD, scan stats, refresh button.
-->
<script lang="ts">
  import { store } from "$lib/stores/skills.svelte";

  const timeSinceScan = $derived.by(() => {
    if (!store.lastScanTime) return "";
    const elapsed = Math.round((Date.now() - store.lastScanTime) / 1000);
    if (elapsed < 5)  return "just now";
    if (elapsed < 60) return `${elapsed}s ago`;
    return `${Math.round(elapsed / 60)}m ago`;
  });
</script>

<div
  class="flex shrink-0 items-center justify-end gap-1.5 px-3 py-1.5 border-t border-[var(--color-border)]"
  style="background: var(--color-surface-2);"
>
  <!-- Stats -->
  <div class="flex shrink-0 items-center gap-1.5 text-[9.5px] tabular-nums text-[var(--color-text-secondary)]">
    <span class="opacity-80">{store.scanDurationMs}ms</span>
    {#if timeSinceScan}
      <span class="opacity-50">·</span>
      <span class="opacity-75">{timeSinceScan}</span>
    {/if}
  </div>
</div>
