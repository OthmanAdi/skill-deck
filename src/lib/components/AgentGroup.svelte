<!--
  @agent-context: Collapsible agent section for the grouped skill list.
  Header: chevron + flat agent-color dot + agent name + skill count.
  Body: tree-line left border + SkillRow list.
  Click header to collapse/expand. The `enter` prop gates the one-time list
  entrance stagger: rows only stagger on the first process reveal, so
  re-expanding a group slides open without a second competing animation.
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
    enter = true,
  }: {
    agentId: string;
    skills: Skill[];
    startIndex?: number;
    focusedIndex?: number;
    /** Play the one-time row-entrance stagger (first reveal only). */
    enter?: boolean;
  } = $props();

  const isCollapsed = $derived(store.collapsedAgents.has(agentId));
  const color = $derived(store.getAgentColor(agentId));
  const name = $derived(store.getAgentDisplayName(agentId));
</script>

<div class="group/agent">
  <!-- Header row -->
  <button
    class="flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-1.5 text-left
      transition-[background-color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
      hover:bg-[var(--color-surface-2)]"
    style="min-height: 32px;"
    onclick={() => toggleAgentCollapse(agentId)}
    aria-expanded={!isCollapsed}
    aria-controls={`agent-group-body-${agentId}`}
  >
    <!-- Chevron -->
    <svg
      class="h-3 w-3 shrink-0 text-[var(--color-text-muted)]
        transition-transform duration-[var(--motion-base)] ease-[var(--ease-standard)]
        {isCollapsed ? '' : 'rotate-90'}"
      fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
      aria-hidden="true"
    >
      <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
    </svg>

    <!-- Flat agent color dot (identity accent, not a glow) -->
    <span
      class="h-[7px] w-[7px] shrink-0 rounded-full"
      style="background: {color};"
    ></span>

    <!-- Agent name -->
    <span class="flex-1 truncate text-xs font-semibold tracking-[0.01em]
      text-[var(--color-text-secondary)]">
      {name}
    </span>

    <!-- Skill count -->
    <span class="shrink-0 tabular-nums text-2xs text-[var(--color-text-muted)]">
      {skills.length}
    </span>
  </button>

  <!-- Skills list (collapsible) -->
  {#if !isCollapsed}
    <div
      id={`agent-group-body-${agentId}`}
      role="region"
      aria-label={`${name} skills`}
      class="ml-[8px] border-l border-[var(--color-border)] pl-2 pb-0.5"
      in:slide={{ duration: 200, easing: cubicOut }}
      out:slide={{ duration: 130, easing: cubicOut }}
    >
      {#each skills as skill, i (skill.id)}
        <SkillRow
          {skill}
          index={startIndex + i}
          delayIndex={i}
          isFocused={focusedIndex === startIndex + i}
          {enter}
        />
      {/each}
    </div>
  {/if}
</div>
