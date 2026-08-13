<!--
  @agent-context: Tab bar — All / Starred / Registry. Sliding pill indicator
  with a smooth motion-base transition. Real WAI-ARIA tablist: role=tablist on
  the track, role=tab + aria-selected on each control, roving tabindex (active
  tab is the only tab stop), and Left/Right/Home/End arrow movement with
  automatic activation (focus follows selection). Focus is the global phosphor
  :focus-visible ring.
-->
<script lang="ts">
  import { setActiveTab, store } from "$lib/stores/skills.svelte";
  import type { TabView } from "$lib/types";

  const tabs: { id: TabView; label: string }[] = [
    { id: "all",     label: "All" },
    { id: "starred", label: "Starred" },
    { id: "registry", label: "Registry" },
  ];

  function getCount(id: TabView): number {
    switch (id) {
      case "all":     return store.skills.length;
      case "starred": return store.starredCount;
      case "registry": return store.registryResultCount;
      default: return 0;
    }
  }

  const activeIndex = $derived(tabs.findIndex((t) => t.id === store.activeTab));

  let tabEls = $state<(HTMLButtonElement | undefined)[]>([]);

  function activate(i: number) {
    const tab = tabs[i];
    if (!tab) return;
    void setActiveTab(tab.id);
    tabEls[i]?.focus();
  }

  // Automatic-activation tablist: arrows move selection and focus together.
  // Bound on each tab (the focusable, interactive elements) rather than the
  // tablist container, so a non-interactive <div> never carries a key handler.
  function onTabKeydown(e: KeyboardEvent) {
    const last = tabs.length - 1;
    let next = -1;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") {
      next = activeIndex >= last ? 0 : activeIndex + 1;
    } else if (e.key === "ArrowLeft" || e.key === "ArrowUp") {
      next = activeIndex <= 0 ? last : activeIndex - 1;
    } else if (e.key === "Home") {
      next = 0;
    } else if (e.key === "End") {
      next = last;
    } else {
      return;
    }
    e.preventDefault();
    activate(next);
  }
</script>

<div
  role="tablist"
  aria-label="Skill list scope"
  class="relative flex gap-0.5 p-0.5"
  style="
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  "
>
  <!-- Sliding indicator -->
  <div
    class="absolute top-0.5 bottom-0.5 transition-[left,width] ease-[var(--ease-standard)]"
    style="
      left: calc({activeIndex} * (100% / {tabs.length}) + 2px);
      width: calc(100% / {tabs.length} - 4px);
      background: var(--color-surface-3);
      border: 1px solid var(--color-border-hover);
      border-radius: calc(var(--radius-md) - 2px);
      transition-duration: var(--motion-base);
    "
    aria-hidden="true"
  ></div>

  {#each tabs as tab, i (tab.id)}
    {@const isActive = store.activeTab === tab.id}
    {@const count = getCount(tab.id)}
    <button
      bind:this={tabEls[i]}
      role="tab"
      aria-selected={isActive}
      tabindex={isActive ? 0 : -1}
      class="relative z-10 flex-1 px-2 py-1.5
        text-xs font-medium
        transition-[color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
        {isActive ? 'text-[var(--color-text-primary)]' : 'text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]'}"
      style="border-radius: calc(var(--radius-md) - 2px);"
      onclick={() => activate(i)}
      onkeydown={onTabKeydown}
    >
      {tab.label}
      <span class="ml-1 tabular-nums text-2xs
        transition-[color] duration-[var(--motion-fast)]
        {isActive ? 'text-[var(--color-accent-text)]' : 'opacity-40'}">
        {count}
      </span>
    </button>
  {/each}
</div>
