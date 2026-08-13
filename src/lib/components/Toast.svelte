<!--
  @agent-context: Toast notification — auto-dismisses after 2.5s. Reads
  store.toastKind ("ok" | "error") to branch icon + color: "ok" is the phosphor
  checkmark on a hairline surface, "error" is the error-toned alert with an
  error border. A live region (role=status, aria-live=polite) announces the
  message to screen readers. Single message, not a queue (see needs_from_integrator).
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { store } from "$lib/stores/skills.svelte";

  const isError = $derived(store.toastKind === "error");
</script>

<div
  class="pointer-events-none fixed bottom-10 left-1/2 z-50 -translate-x-1/2"
  role="status"
  aria-live="polite"
  aria-atomic="true"
>
  {#if store.toastMessage}
    <div
      class="pointer-events-auto flex items-center gap-2 rounded-[var(--radius-md)] px-3 py-2
        text-xs font-medium text-[var(--color-text-primary)]"
      style="
        background: var(--color-surface-2);
        border: 1px solid {isError ? 'var(--color-error)' : 'var(--color-border-active)'};
        box-shadow: var(--shadow-popover);
      "
      transition:fly={{ y: 6, duration: 160, easing: cubicOut }}
    >
      {#if isError}
        <!-- Alert triangle — failure -->
        <svg
          class="h-3.5 w-3.5 shrink-0 text-[var(--color-error)]"
          fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.2"
          aria-hidden="true"
        >
          <path stroke-linecap="round" stroke-linejoin="round"
            d="M12 9v4m0 4h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
        </svg>
      {:else}
        <!-- Checkmark — success -->
        <svg
          class="h-3.5 w-3.5 shrink-0 text-[var(--color-accent-text)]"
          fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
          aria-hidden="true"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
        </svg>
      {/if}
      <span class="max-w-[260px] truncate">{store.toastMessage}</span>
    </div>
  {/if}
</div>
