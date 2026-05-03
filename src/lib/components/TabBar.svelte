<!--
  @agent-context: Tab bar — All / Starred.
  Sliding pill indicator with smooth 180ms transition.
-->
<script lang="ts">
  import { store } from "$lib/stores/skills.svelte";
  import type { TabView } from "$lib/types";

  const tabs: { id: TabView; label: string }[] = [
    { id: "all",     label: "All" },
    { id: "starred", label: "Starred" },
  ];

  function getCount(id: TabView): number {
    switch (id) {
      case "all":     return store.skills.length;
      case "starred": return store.starredCount;
    }
  }

  const activeIndex = $derived(tabs.findIndex((t) => t.id === store.activeTab));
</script>

<div
  class="relative flex gap-0.5 p-0.5"
  style="
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  "
>
  <!-- Sliding indicator -->
  <div
    class="absolute top-0.5 bottom-0.5 transition-[left,width] ease-in-out"
    style="
      left: calc({activeIndex} * (100% / {tabs.length}) + 2px);
      width: calc(100% / {tabs.length} - 4px);
      background: var(--color-surface-3);
      border: 1px solid var(--color-border-hover);
      border-radius: calc(var(--radius-md) - 2px);
      transition-duration: 180ms;
    "
  ></div>

  {#each tabs as tab}
    {@const isActive = store.activeTab === tab.id}
    {@const count = getCount(tab.id)}
    <button
      class="relative z-10 flex-1 px-2 py-1.5
        text-[11px] font-medium
        transition-colors duration-[120ms] ease-out
        {isActive ? 'text-[var(--color-text-primary)]' : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
      style="border-radius: calc(var(--radius-md) - 2px);"
      onclick={() => (store.activeTab = tab.id)}
    >
      {tab.label}
      <span class="ml-1 tabular-nums text-[9px]
        transition-colors duration-[120ms]
        {isActive ? 'text-[var(--color-accent)]' : 'opacity-35'}">
        {count}
      </span>
    </button>
  {/each}
</div>
