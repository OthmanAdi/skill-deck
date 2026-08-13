<!--
  @agent-context: Agent identity mark (DESIGN 3.5 / 8). Renders the bracketed
  mono form — [claude], [cursor] — via the shared .badge-agent primitive from
  app.css: transparent background, hairline border, tinted text, [ ] brackets
  drawn in CSS. Flagship agents resolve to the desaturated, AA-tuned
  --color-agent-* tokens (theme-aware: light values hold >=4.5:1 as text on
  surface-1) — the SAME KNOWN_AGENTS set ui/Badge.svelte uses, so the two
  .badge-agent renderers never diverge; keep the sets in sync. The non-flagship
  long tail (~50 agents) falls back to the backend-assigned hue
  (AgentInfo.color) to preserve identity; it is a runtime store value, never a
  literal.
  Used in SkillCard's metadata row; rows are already grouped by agent so they
  carry no badge.
  NOTE (see needs_from_integrator): fallback backend hues are single-value
  across themes and not AA-tuned for light — the fix belongs in registry.rs
  (light variants) or a documented agentId -> --color-agent-* mapping, not in
  this leaf.
-->
<script lang="ts">
  import type { AgentId } from "$lib/types";
  import { DEFAULT_AGENT_COLOR } from "$lib/types";
  import { store } from "$lib/stores/skills.svelte";

  let { agentId }: { agentId: AgentId } = $props();

  // Flagship agents with desaturated --color-agent-* tokens (DESIGN 3.5).
  // Mirrors ui/Badge.svelte's KNOWN_AGENTS — keep both sets in sync.
  const KNOWN_AGENTS = new Set([
    "claude",
    "cursor",
    "copilot",
    "codex",
    "windsurf",
    "gemini",
    "cline",
    "aider",
  ]);

  const id = $derived(typeof agentId === "string" ? agentId : "custom");
  const hue = $derived(
    KNOWN_AGENTS.has(id)
      ? `var(--color-agent-${id})`
      : (store.getAgentInfo(id)?.color ?? DEFAULT_AGENT_COLOR)
  );
  const name = $derived(
    store.getAgentInfo(id)?.displayName ??
      (typeof agentId === "object" ? agentId.custom : id)
  );
</script>

<span class="badge-agent" style="--badge-hue: {hue};" title={name}>{name}</span>
