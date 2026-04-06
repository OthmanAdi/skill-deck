<!--
  @agent-context: Tree view renderer for parent/child skill hierarchies.
  Groups the flat skill list by parentId and renders collapsible parent nodes
  with indented children.

  The backend sends all skills flat with parentId set on children.
  This component builds the visual tree client-side — no separate API call needed.
-->
<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { Skill } from "$lib/types";
  import SkillCard from "./SkillCard.svelte";

  let {
    skills,
    focusedIndex = -1,
  }: {
    skills: Skill[];
    focusedIndex?: number;
  } = $props();

  /** Track which parent nodes are collapsed */
  let collapsed = $state<Set<string>>(new Set());

  function toggleCollapse(id: string) {
    const next = new Set(collapsed);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    collapsed = next;
  }

  /** Top-level skills (no parent) */
  const roots = $derived(skills.filter((s) => !s.parentId));

  /** Get direct children of a given parent ID */
  function childrenOf(parentId: string): Skill[] {
    return skills.filter((s) => s.parentId === parentId);
  }

  /** Global index tracker for keyboard navigation across the tree */
  let globalIdx = 0;
  function nextIdx(): number {
    return globalIdx++;
  }

  $effect(() => {
    // Reset index on each skills update so focusedIndex aligns correctly
    globalIdx = 0;
  });
</script>

<div class="flex flex-col gap-1">
  {#each roots as root (root.id)}
    {@const kids = childrenOf(root.id)}
    {@const isCollapsed = collapsed.has(root.id)}

    <div>
      <!-- Parent row: skill card + collapse toggle if it has children -->
      <div class="flex items-start gap-1">
        <div class="flex-1 min-w-0">
          <SkillCard skill={root} index={nextIdx()} isFocused={focusedIndex === nextIdx() - 1} />
        </div>

        {#if kids.length > 0}
          <button
            class="mt-2 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg
              text-[var(--color-text-muted)] transition-all duration-150
              hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
            onclick={() => toggleCollapse(root.id)}
            title="{isCollapsed ? 'Expand' : 'Collapse'} {kids.length} sub-skill{kids.length !== 1 ? 's' : ''}"
            aria-label="{isCollapsed ? 'Expand' : 'Collapse'} children"
          >
            <svg
              class="h-3 w-3 transition-transform duration-200"
              style="transform: rotate({isCollapsed ? '-90deg' : '0deg'});"
              fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        {/if}
      </div>

      <!-- Children: indented, shown when not collapsed -->
      {#if kids.length > 0 && !isCollapsed}
        <div
          class="ml-4 mt-1 flex flex-col gap-1 border-l pl-3"
          style="border-color: var(--color-border);"
          transition:slide={{ duration: 160, easing: cubicOut }}
        >
          {#each kids as child (child.id)}
            {@const grandkids = childrenOf(child.id)}
            {@const childCollapsed = collapsed.has(child.id)}

            <div>
              <div class="flex items-start gap-1">
                <div class="flex-1 min-w-0">
                  <SkillCard skill={child} index={nextIdx()} isFocused={focusedIndex === nextIdx() - 1} />
                </div>

                {#if grandkids.length > 0}
                  <button
                    class="mt-2 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg
                      text-[var(--color-text-muted)] transition-all duration-150
                      hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                    onclick={() => toggleCollapse(child.id)}
                    title="{childCollapsed ? 'Expand' : 'Collapse'} {grandkids.length} sub-skill{grandkids.length !== 1 ? 's' : ''}"
                    aria-label="{childCollapsed ? 'Expand' : 'Collapse'} children"
                  >
                    <svg
                      class="h-3 w-3 transition-transform duration-200"
                      style="transform: rotate({childCollapsed ? '-90deg' : '0deg'});"
                      fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
                    >
                      <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
                    </svg>
                  </button>
                {/if}
              </div>

              {#if grandkids.length > 0 && !childCollapsed}
                <div
                  class="ml-4 mt-1 flex flex-col gap-1 border-l pl-3"
                  style="border-color: var(--color-border);"
                  transition:slide={{ duration: 160, easing: cubicOut }}
                >
                  {#each grandkids as grandchild (grandchild.id)}
                    <SkillCard skill={grandchild} index={nextIdx()} isFocused={focusedIndex === nextIdx() - 1} />
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>
