<!--
  @agent-context: Discovery facet filters for artifact types, use-cases, and
  tags. Helps users answer when to use skills. All three rows share ONE toggle
  treatment (inactive surface-2 chip / active accent-subtle + accent-text +
  border-active) so meaning reads from state, not from per-row color. Each chip
  keeps aria-pressed for the active state. When no facets are detected the panel
  states so honestly instead of rendering three empty rows.
-->
<script lang="ts">
  import {
    clearDiscoveryFilters,
    store,
    toggleArtifactTypeFilter,
    toggleTagFilter,
    toggleUseCaseFilter,
  } from "$lib/stores/skills.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  const topUseCases = $derived(store.availableUseCases.slice(0, 10));
  const topTags = $derived(store.availableTags.slice(0, 14));
  const topTypes = $derived(store.availableArtifactTypes);

  const hasFilters = $derived(
    store.selectedTags.length > 0
      || store.selectedUseCases.length > 0
      || store.selectedArtifactTypes.length > 0
  );

  const hasFacets = $derived(
    topTypes.length > 0 || topUseCases.length > 0 || topTags.length > 0
  );

  function present(label: string): string {
    return label
      .split("-")
      .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : part))
      .join(" ");
  }

  const typeLabel: Record<string, string> = {
    skill: "Skill",
    command: "Command",
    hook: "Hook",
    rule: "Rule",
    workflow: "Workflow",
    prompt: "Prompt",
    config: "Config",
    other: "Other",
  };

  // ONE chip treatment shared by all three rows. Active = phosphor.
  const chipBase =
    "rounded-sm border px-1.5 py-0.5 text-2xs font-medium "
    + "transition-[background-color,color,border-color,scale] duration-[var(--motion-fast)] "
    + "ease-[var(--ease-standard)] active:scale-[0.96]";

  function chipStyle(active: boolean): string {
    return active
      ? "background: var(--color-accent-subtle); color: var(--color-accent-text); border-color: var(--color-border-active);"
      : "background: var(--color-surface-2); color: var(--color-text-secondary); border-color: var(--color-border);";
  }
</script>

<div
  class="shrink-0 {compact ? '' : 'rounded-md border px-2 py-2'}"
  style={compact ? "" : "background: var(--color-surface-1); border-color: var(--color-border);"}
>
  <div class="mb-1.5 flex items-center justify-between gap-2">
    <span class="u-caps">Find by intent</span>

    {#if hasFilters}
      <button
        class="rounded-sm px-1.5 py-0.5 text-2xs font-medium text-[var(--color-text-muted)]
          transition-[background-color,color,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
          hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)] active:scale-[0.96]"
        onclick={clearDiscoveryFilters}
      >clear</button>
    {/if}
  </div>

  {#if !hasFacets}
    <p class="text-2xs text-[var(--color-text-muted)]">No discovery tags detected yet</p>
  {:else}
    {#if topTypes.length > 0}
      <div class="mb-2 flex flex-wrap gap-1">
        {#each topTypes as item (item.label)}
          {@const active = store.selectedArtifactTypes.includes(item.label)}
          <button
            class={chipBase}
            style={chipStyle(active)}
            onclick={() => toggleArtifactTypeFilter(item.label)}
            aria-pressed={active}
          >
            {typeLabel[item.label] ?? present(item.label)}
            <span class="ml-1 tabular-nums opacity-70">{item.count}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if topUseCases.length > 0}
      <div class="mb-2 flex flex-wrap gap-1">
        {#each topUseCases as item (item.label)}
          {@const active = store.selectedUseCases.includes(item.label)}
          <button
            class={chipBase}
            style={chipStyle(active)}
            onclick={() => toggleUseCaseFilter(item.label)}
            aria-pressed={active}
          >
            {present(item.label)}
            <span class="ml-1 tabular-nums opacity-70">{item.count}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if topTags.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each topTags as item (item.label)}
          {@const active = store.selectedTags.includes(item.label)}
          <button
            class={chipBase}
            style={chipStyle(active)}
            onclick={() => toggleTagFilter(item.label)}
            aria-pressed={active}
          >
            #{item.label}
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>
