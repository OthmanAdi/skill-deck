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
</script>

<div class="flex flex-col gap-1">
  {#each visibleOrder as skill (skill.id)}
    {@const idx = visibleIndex(skill.id)}
    {@const depth = indentById.get(skill.id) ?? 0}
    {@const isCollapsed = collapsed.has(skill.id)}
    {@const hasKids = hasChildren(skill.id)}
    {@const childCount = childrenOfId(skill.id).length}

    <div style="margin-left: {depth * 16}px;">
      <SkillCard
        {skill}
        index={idx}
        isFocused={focusedIndex === idx}
        hasChildren={hasKids}
        childrenCollapsed={isCollapsed}
        childrenCount={childCount}
        onToggleChildren={() => toggleTreeNodeCollapse(skill.id)}
      />
    </div>
  {/each}
</div>
