<!--
  @agent-context: Search input. Focused on clean styling, no blur effects.
  Auto-focuses on overlay open. X button clears query.
-->
<script lang="ts">
  import { store } from "$lib/stores/skills.svelte";

  let inputEl: HTMLInputElement | undefined = $state();
  let isFocused = $state(false);

  export function focusInput() {
    inputEl?.focus();
  }

  $effect(() => {
    focusInput();
  });
</script>

<div class="relative">
  <!-- Search icon -->
  <svg
    class="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2
      transition-colors duration-[120ms]
      {isFocused ? 'text-[var(--color-accent)]' : 'text-[var(--color-text-muted)]'}"
    fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
  >
    <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
  </svg>

  <input
    bind:this={inputEl}
    bind:value={store.searchQuery}
    onfocus={() => (isFocused = true)}
    onblur={() => (isFocused = false)}
    type="text"
    placeholder="Search skills…"
    class="w-full py-2 pl-8 pr-7 text-[12px]
      text-[var(--color-text-primary)] placeholder-[var(--color-text-muted)]
      transition-[border-color,box-shadow] duration-[120ms] focus:outline-none"
    style="
      background: var(--color-surface-1);
      border: 1px solid {isFocused ? 'var(--color-border-active)' : 'var(--color-border)'};
      border-radius: var(--radius-md);
      box-shadow: {isFocused ? '0 0 0 3px var(--color-search-glow)' : 'none'};
    "
  />

  <!-- Clear button -->
  {#if store.searchQuery}
    <button
      class="absolute right-2 top-1/2 -translate-y-1/2 flex items-center justify-center
        h-4 w-4 rounded text-[var(--color-text-muted)]
        hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]
        transition-all duration-[120ms]"
      onclick={() => { store.searchQuery = ""; inputEl?.focus(); }}
      aria-label="Clear search"
    >
      <svg class="h-2.5 w-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
      </svg>
    </button>
  {/if}
</div>
