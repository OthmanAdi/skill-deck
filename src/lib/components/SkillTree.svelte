<!--
  @agent-context: Card view renderer for parent/child skill hierarchies.
  Groups the flat skill list by parentId and renders collapsible parent nodes
  with indented children.

  The backend sends all skills flat with parentId set on children.
  This component builds the visual tree client-side — no separate API call needed.
-->
<script lang="ts">
  import type { Skill } from "$lib/types";
  import { store, toggleTreeNodeCollapse } from "$lib/stores/skills.svelte";
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

  /**
   * Build parent/child map but only keep links where the candidate parent and
   * child share the SAME agent AND the SAME artifact type. The backend assigns
   * parent_id by filesystem prefix alone, which creates spurious links such as
   * `~/.gemini/GEMINI.md` becoming parent of every Gemini SKILL.md, or
   * `CLAUDE.md` becoming parent of every Claude command. Filtering here keeps
   * the tree faithful to real plugin / sub-skill relationships.
   */
  const skillById = $derived.by(() => {
    const map = new Map<string, Skill>();
    for (const skill of skills) map.set(skill.id, skill);
    return map;
  });

  function sameAgent(a: Skill, b: Skill): boolean {
    const aid = typeof a.agentId === "string" ? a.agentId : "custom";
    const bid = typeof b.agentId === "string" ? b.agentId : "custom";
    return aid === bid;
  }

  const childrenMap = $derived.by(() => {
    const map = new Map<string, Skill[]>();
    for (const skill of skills) {
      if (!skill.parentId) continue;
      const parent = skillById.get(skill.parentId);
      if (!parent) continue;
      if (!sameAgent(parent, skill)) continue;
      if (parent.artifactType !== skill.artifactType) continue;
      const existing = map.get(skill.parentId) ?? [];
      existing.push(skill);
      map.set(skill.parentId, existing);
    }
    return map;
  });

  /**
   * Top-level roots: skills with no real (filtered) parent in the visible set.
   * Includes orphans whose backend-assigned parent is missing OR was filtered
   * out by the same-agent / same-type rule above.
   */
  const roots = $derived(
    skills.filter((s) => {
      if (!s.parentId) return true;
      if (!idSet.has(s.parentId)) return true;
      const parent = skillById.get(s.parentId);
      if (!parent) return true;
      if (!sameAgent(parent, s)) return true;
      if (parent.artifactType !== s.artifactType) return true;
      return false;
    })
  );

  const visibleOrder = $derived(
    flattenVisible(roots, collapsed, childrenMap)
  );

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

  /**
   * For a given root, return its visible descendants in DFS order (respecting
   * per node collapsed state). Used to render a root and all its visible
   * sub skills inside a single glass container so the visual grouping is
   * unambiguous.
   */
  function collectVisibleDescendants(root: Skill): Skill[] {
    const result: Skill[] = [];
    const visit = (node: Skill) => {
      if (collapsed.has(node.id)) return;
      const kids = childrenMap.get(node.id) ?? [];
      for (const kid of kids) {
        result.push(kid);
        visit(kid);
      }
    };
    visit(root);
    return result;
  }
</script>

<div class="flex flex-col gap-1">
  {#each roots as root (root.id)}
    {@const rootIdx = visibleIndex(root.id)}
    {@const rootHasKids = hasChildren(root.id)}
    {@const rootChildCount = childrenOfId(root.id).length}
    {@const rootCollapsed = collapsed.has(root.id)}
    {@const descendants = collectVisibleDescendants(root)}
    {@const groupHasVisibleKids = rootHasKids && descendants.length > 0}

    {#if groupHasVisibleKids}
      <!-- Glass group container: visually binds the root with its visible sub skills -->
      <div class="skill-group-glass rounded-2xl p-1.5">
        <div class="flex flex-col gap-1">
          <SkillCard
            skill={root}
            index={rootIdx}
            isFocused={focusedIndex === rootIdx}
            hasChildren={rootHasKids}
            childrenCollapsed={rootCollapsed}
            childrenCount={rootChildCount}
            onToggleChildren={() => toggleTreeNodeCollapse(root.id)}
          />

          {#each descendants as desc (desc.id)}
            {@const dIdx = visibleIndex(desc.id)}
            {@const dDepth = indentById.get(desc.id) ?? 0}
            {@const dHasKids = hasChildren(desc.id)}
            {@const dCollapsed = collapsed.has(desc.id)}
            {@const dCount = childrenOfId(desc.id).length}

            <div style="margin-left: {dDepth * 16}px;">
              <SkillCard
                skill={desc}
                index={dIdx}
                isFocused={focusedIndex === dIdx}
                hasChildren={dHasKids}
                childrenCollapsed={dCollapsed}
                childrenCount={dCount}
                onToggleChildren={() => toggleTreeNodeCollapse(desc.id)}
              />
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <SkillCard
        skill={root}
        index={rootIdx}
        isFocused={focusedIndex === rootIdx}
        hasChildren={rootHasKids}
        childrenCollapsed={rootCollapsed}
        childrenCount={rootChildCount}
        onToggleChildren={() => toggleTreeNodeCollapse(root.id)}
      />
    {/if}
  {/each}
</div>
