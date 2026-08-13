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
      transition-[color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
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
    class="w-full py-2 pl-8 pr-8 text-sm
      text-[var(--color-text-primary)] placeholder-[var(--color-text-muted)]
      transition-[border-color,box-shadow] duration-[var(--motion-fast)] ease-[var(--ease-standard)] focus:outline-none"
    style="
      background: var(--color-surface-2);
      border: 1px solid {isFocused ? 'var(--color-border-active)' : 'var(--color-border)'};
      border-radius: var(--radius-sm);
      box-shadow: {isFocused ? '0 0 0 3px var(--color-search-glow)' : 'none'};
    "
  />

  <!-- Clear button -->
  {#if store.searchQuery}
    <button
      class="absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center justify-center
        h-6 w-6 rounded-sm text-[var(--color-text-muted)]
        hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]
        transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
        active:scale-[0.96]"
      onclick={() => { store.searchQuery = ""; inputEl?.focus(); }}
      aria-label="Clear search"
    >
      <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
      </svg>
    </button>
  {/if}
</div>
