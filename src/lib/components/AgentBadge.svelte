<!--
  @agent-context: Compact agent badge with color dot and agent name.
  Used inside SkillCard expanded view.
-->
<script lang="ts">
  import { AGENT_COLORS, AGENT_NAMES } from "$lib/types";
  import type { AgentId } from "$lib/types";

  let { agentId }: { agentId: AgentId } = $props();

  const id    = $derived(typeof agentId === "string" ? agentId : "custom");
  const color = $derived(AGENT_COLORS[id] ?? "oklch(0.70 0.08 260)");
  const name  = $derived(AGENT_NAMES[id] ?? (typeof agentId === "object" ? agentId.custom : id));
</script>

<span
  class="inline-flex items-center gap-1 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.05em]"
  style="
    background: color-mix(in oklch, {color} 10%, transparent);
    color: {color};
    border: 1px solid color-mix(in oklch, {color} 15%, transparent);
    border-radius: var(--radius-sm);
  "
>
  <span
    class="h-[5px] w-[5px] rounded-full"
    style="background: {color};"
  ></span>
  {name}
</span>
