<!--
  @agent-context: Compact agent badge with color dot and agent name.
  Used inside SkillCard expanded view.
-->
<script lang="ts">
  import { AGENT_COLORS, AGENT_NAMES } from "$lib/types";
  import type { AgentId } from "$lib/types";

  let { agentId }: { agentId: AgentId } = $props();

  const id    = $derived(typeof agentId === "string" ? agentId : "custom");
  const color = $derived(AGENT_COLORS[id] ?? "#7a7fad");
  const name  = $derived(AGENT_NAMES[id] ?? (typeof agentId === "object" ? agentId.custom : id));

  function hexToRgba(hex: string, alpha: number): string {
    const normalized = hex.replace("#", "").trim();
    if (normalized.length !== 6) return `rgba(122, 127, 173, ${alpha})`;
    const r = Number.parseInt(normalized.slice(0, 2), 16);
    const g = Number.parseInt(normalized.slice(2, 4), 16);
    const b = Number.parseInt(normalized.slice(4, 6), 16);
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
</script>

<span
  class="inline-flex items-center gap-1 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.05em]"
  style="
    background: {hexToRgba(color, 0.12)};
    color: {color};
    border: 1px solid {hexToRgba(color, 0.22)};
    border-radius: var(--radius-sm);
  "
>
  <span
    class="h-[5px] w-[5px] rounded-full"
    style="background: {color};"
  ></span>
  {name}
</span>
