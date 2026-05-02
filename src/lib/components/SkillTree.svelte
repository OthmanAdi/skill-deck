<!--
  @agent-context: Tree view renderer for parent/child skill hierarchies.
  Groups the flat skill list by parentId and renders collapsible parent nodes
  with indented children.

  The backend sends all skills flat with parentId set on children.
  This component builds the visual tree client-side — no separate API call needed.
-->
<script lang="ts">
  import type { Skill } from "$lib/types";
  import { setCollapsedTreeNodes, store, toggleTreeNodeCollapse } from "$lib/stores/skills.svelte";
  import SkillCard from "./SkillCard.svelte";

  let {
    skills,
    focusedIndex = -1,
  }: {
    skills: Skill[];
    focusedIndex?: number;
  } = $props();

  const collapsed = $derived(store.collapsedTreeNodes);

  const idSet = $derived(new Set(skills.map((s) => s.id)));

  /** Top-level visible roots: true roots + filtered orphans whose parent is not visible */
  const roots = $derived(
    skills.filter((s) => !s.parentId || !idSet.has(s.parentId))
  );

  function flattenVisible(
    nodes: Skill[],
    collapsedSet: Set<string>,
    childrenLookup: Map<string, Skill[]>
  ): Skill[] {
    const result: Skill[] = [];
    const visit = (node: Skill) => {
      result.push(node);
      if (collapsedSet.has(node.id)) return;
      const kids = childrenLookup.get(node.id) ?? [];
      for (const kid of kids) {
        visit(kid);
      }
    };

    for (const node of nodes) {
      visit(node);
    }
    return result;
  }

  const childrenMap = $derived.by(() => {
    const map = new Map<string, Skill[]>();
    for (const skill of skills) {
      if (!skill.parentId) continue;
      const existing = map.get(skill.parentId) ?? [];
      existing.push(skill);
      map.set(skill.parentId, existing);
    }
    return map;
  });

  const visibleOrder = $derived(
    flattenVisible(roots, collapsed, childrenMap)
  );

  const collapsibleNodeIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const skill of skills) {
      if ((childrenMap.get(skill.id) ?? []).length > 0) {
        ids.add(skill.id);
      }
    }
    return ids;
  });

  function collapseAll() {
    setCollapsedTreeNodes(new Set(collapsibleNodeIds));
  }

  function expandAll() {
    setCollapsedTreeNodes(new Set());
  }

  function visibleIndex(skillId: string): number {
    return visibleOrder.findIndex((s) => s.id === skillId);
  }

  const indentById = $derived.by(() => {
    const map = new Map<string, number>();
    const walk = (node: Skill, depth: number) => {
      map.set(node.id, depth);
      if (collapsed.has(node.id)) return;
      const kids = childrenMap.get(node.id) ?? [];
      for (const kid of kids) {
        walk(kid, depth + 1);
      }
    };
    for (const root of roots) {
      walk(root, 0);
    }
    return map;
  });

  function hasChildren(skillId: string): boolean {
    return (childrenMap.get(skillId) ?? []).length > 0;
  }

  function childrenOfId(skillId: string): Skill[] {
    return childrenMap.get(skillId) ?? [];
  }
</script>

<div class="mb-2 flex items-center justify-end gap-1.5 px-1">
  <button
    class="rounded-md border px-2 py-1 text-[10px] font-medium text-[var(--color-text-secondary)]
      transition-colors hover:bg-[var(--color-surface-2)]"
    style="border-color: var(--color-border);"
    onclick={collapseAll}
    title="Collapse all sections"
  >
    Collapse all
  </button>
  <button
    class="rounded-md border px-2 py-1 text-[10px] font-medium text-[var(--color-text-secondary)]
      transition-colors hover:bg-[var(--color-surface-2)]"
    style="border-color: var(--color-border);"
    onclick={expandAll}
    title="Expand all sections"
  >
    Expand all
  </button>
</div>

<div class="flex flex-col gap-1">
  {#each visibleOrder as skill (skill.id)}
    {@const idx = visibleIndex(skill.id)}
    {@const depth = indentById.get(skill.id) ?? 0}
    {@const isCollapsed = collapsed.has(skill.id)}
    {@const hasKids = hasChildren(skill.id)}

    <div class="flex items-start gap-1" style="margin-left: {depth * 16}px;">
      <div class="flex-1 min-w-0">
        <SkillCard skill={skill} index={idx} isFocused={focusedIndex === idx} />
      </div>

      {#if hasKids}
        <button
          class="mt-2 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg
            text-[var(--color-text-muted)] transition-all duration-150
            hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
          onclick={() => toggleTreeNodeCollapse(skill.id)}
          title="{isCollapsed ? 'Expand' : 'Collapse'} {childrenOfId(skill.id).length} sub-skill{childrenOfId(skill.id).length !== 1 ? 's' : ''}"
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
  {/each}
</div>
