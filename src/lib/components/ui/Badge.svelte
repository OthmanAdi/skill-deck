<!--
  @agent-context: The Badge primitive (DESIGN 8). One element, three variants:
  - agent  : bracketed mono identity mark ([claude]). Pass `agent` (an AgentId)
             to tint text + hairline with a desaturated, AA-checked --color-agent-*
             hue on a transparent background. Brackets come from CSS.
  - meta   : plain muted taxonomy chip (surface-2, radius-xs) for neutral facts.
  - signal : the ONE filled emphasis per surface. `tone` = accent (default) /
             ok / warn / error. on-accent text passes AA on every fill, both themes.
  All visuals live in app.css (.badge-agent / .badge-meta / .badge-signal); this
  component only maps props -> class + the --badge-hue token. No color literals.
  A skill row/card shows at most: one agent badge + 2 meta chips + 1 signal.
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  type Variant = "agent" | "meta" | "signal";
  type Tone = "accent" | "ok" | "warn" | "error";

  let {
    variant = "meta",
    agent = null,
    tone = "accent",
    bracket = true,
    title = undefined,
    class: klass = "",
    children,
  }: {
    variant?: Variant;
    /** AgentId — maps to a --color-agent-<id> token. Only used by variant="agent". */
    agent?: string | null;
    /** Fill color for variant="signal". */
    tone?: Tone;
    /** variant="agent" only — render the [ ] brackets (default true). */
    bracket?: boolean;
    title?: string | undefined;
    class?: string;
    children?: Snippet;
  } = $props();

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

  // Tint token for the agent hue. Unknown ids fall back to secondary ink via
  // the CSS default, so the badge stays legible instead of breaking.
  const hueVar = $derived(
    variant === "agent" && agent && KNOWN_AGENTS.has(agent)
      ? `var(--color-agent-${agent})`
      : undefined,
  );

  const cls = $derived(
    [
      variant === "agent"
        ? "badge-agent"
        : variant === "signal"
          ? "badge-signal"
          : "badge-meta",
      variant === "agent" && !bracket ? "no-bracket" : "",
      variant === "signal" && tone !== "accent" ? `is-${tone}` : "",
      klass,
    ]
      .filter(Boolean)
      .join(" "),
  );
</script>

<span class={cls} style={hueVar ? `--badge-hue: ${hueVar};` : undefined} {title}
  >{@render children?.()}</span
>
