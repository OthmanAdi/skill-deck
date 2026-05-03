<!--
  @agent-context: Discovery facet filters for use-cases and tags.
  Helps users answer when to use skills.
-->
<script lang="ts">
  import {
    clearDiscoveryFilters,
    store,
    toggleTagFilter,
    toggleUseCaseFilter,
  } from "$lib/stores/skills.svelte";

  const topUseCases = $derived(store.availableUseCases.slice(0, 10));
  const topTags = $derived(store.availableTags.slice(0, 14));

  const hasFilters = $derived(
    store.selectedTags.length > 0 || store.selectedUseCases.length > 0
  );

  function present(label: string): string {
    return label
      .split("-")
      .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : part))
      .join(" ");
  }
</script>

<div
  class="shrink-0 rounded-md border px-2 py-2"
  style="background: var(--color-surface-1); border-color: var(--color-border);"
>
  <div class="mb-1.5 flex items-center justify-between gap-2">
    <span class="text-[10px] font-semibold uppercase tracking-[0.06em] text-[var(--color-text-muted)]">
      Find by intent
    </span>

    {#if hasFilters}
      <button
        class="rounded px-1.5 py-0.5 text-[9px] font-medium text-[var(--color-text-muted)]
          hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]"
        onclick={clearDiscoveryFilters}
      >clear</button>
    {/if}
  </div>

  <div class="mb-2 flex flex-wrap gap-1">
    {#each topUseCases as item (item.label)}
      {@const active = store.selectedUseCases.includes(item.label)}
      <button
        class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium transition-colors duration-120"
        style={active
          ? "background: var(--color-accent-subtle); color: var(--color-accent); border-color: var(--color-border-active);"
          : "background: var(--color-surface-2); color: var(--color-text-secondary); border-color: var(--color-border);"}
        onclick={() => toggleUseCaseFilter(item.label)}
        aria-pressed={active}
      >
        {present(item.label)}
        <span class="ml-1 tabular-nums opacity-70">{item.count}</span>
      </button>
    {/each}
  </div>

  <div class="flex flex-wrap gap-1">
    {#each topTags as item (item.label)}
      {@const active = store.selectedTags.includes(item.label)}
      <button
        class="rounded-md border px-1.5 py-0.5 text-[9px] transition-colors duration-120"
        style={active
          ? "background: var(--color-surface-3); color: var(--color-text-primary); border-color: var(--color-border-active);"
          : "background: transparent; color: var(--color-text-muted); border-color: var(--color-border);"}
        onclick={() => toggleTagFilter(item.label)}
        aria-pressed={active}
      >
        #{item.label}
      </button>
    {/each}
  </div>
</div>
