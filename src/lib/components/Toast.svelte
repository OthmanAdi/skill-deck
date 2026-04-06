<!--
  @agent-context: Toast notification — auto-dismisses after 2.5s.
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { store } from "$lib/stores/skills.svelte";
</script>

{#if store.toastMessage}
  <div
    class="pointer-events-none fixed bottom-12 left-1/2 z-50"
    transition:fly={{ y: 8, duration: 180, easing: cubicOut }}
    style="transform: translateX(-50%);"
  >
    <div
      class="pointer-events-auto flex items-center gap-2 px-3 py-2
        text-[11px] font-medium text-[var(--color-text-primary)]"
      style="
        background: var(--color-surface-2);
        border: 1px solid var(--color-border-active);
        border-radius: var(--radius-md);
        box-shadow: 0 8px 20px -4px var(--color-overlay-shadow);
      "
    >
      <svg class="h-3.5 w-3.5 shrink-0 text-[var(--color-accent)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
      </svg>
      <span class="max-w-[230px] truncate">{store.toastMessage}</span>
    </div>
  </div>
{/if}
