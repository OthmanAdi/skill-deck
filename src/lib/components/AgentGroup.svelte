<!--
  @agent-context: Collapsible agent section for the grouped skill list.
  Header: chevron + agent color dot + agent name + skill count.
  Body: tree-line left border + SkillRow list.
  Click header to collapse/expand.
-->
<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { Skill } from "$lib/types";
  import { toggleAgentCollapse, store } from "$lib/stores/skills.svelte";
  import SkillRow from "./SkillRow.svelte";

  let {
    agentId,
    skills,
    startIndex = 0,
    focusedIndex = -1,
  }: {
    agentId: string;
    skills: Skill[];
    startIndex?: number;
    focusedIndex?: number;
  } = $props();

  const isCollapsed = $derived(store.collapsedAgents.has(agentId));
  const color = $derived(store.getAgentColor(agentId));
  const name = $derived(store.getAgentDisplayName(agentId));

  function hexToRgba(hex: string, alpha: number): string {
    const normalized = hex.replace("#", "").trim();
    if (normalized.length !== 6) return `rgba(122, 127, 173, ${alpha})`;
    const r = Number.parseInt(normalized.slice(0, 2), 16);
    const g = Number.parseInt(normalized.slice(2, 4), 16);
    const b = Number.parseInt(normalized.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
</script>

<div class="group/agent">
  <!-- Header row: 30px height -->
  <button
    class="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-1.5 py-1 text-left
      transition-[background-color] duration-[120ms] ease-out
      hover:bg-[var(--color-surface-2)]"
    style="min-height: 30px;"
    onclick={() => toggleAgentCollapse(agentId)}
    aria-expanded={!isCollapsed}
  >
    <!-- Chevron -->
    <svg
      class="h-3 w-3 shrink-0 text-[var(--color-text-muted)]
        transition-transform duration-200 ease-in-out
        {isCollapsed ? '' : 'rotate-90'}"
      fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
    >
      <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
    </svg>

    <!-- Agent color dot -->
    <span
      class="h-[7px] w-[7px] shrink-0 rounded-full"
      style="background: {color}; box-shadow: 0 0 5px {hexToRgba(color, 0.5)};"
    ></span>

    <!-- Agent name -->
    <span class="flex-1 truncate text-[11px] font-semibold tracking-[0.01em]
      text-[var(--color-text-secondary)]">
      {name}
    </span>

    <!-- Skill count -->
    <span class="shrink-0 tabular-nums text-[10px] text-[var(--color-text-muted)]">
      {skills.length}
    </span>
  </button>

  <!-- Skills list (collapsible) -->
  {#if !isCollapsed}
    <div
      class="ml-[8px] border-l border-[var(--color-border)] pl-2 pb-0.5"
      transition:slide={{ duration: 200, easing: cubicOut }}
    >
      {#each skills as skill, i (skill.id)}
        <SkillRow
          {skill}
          index={startIndex + i}
          delayIndex={i}
          isFocused={focusedIndex === startIndex + i}
        />
      {/each}
    </div>
  {/if}
</div>
