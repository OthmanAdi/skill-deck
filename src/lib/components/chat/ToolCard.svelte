<!--
  @agent-context: Compact tool-result instrument. Always shows kind + title +
  snippet. Status reads off the semantic trio (DESIGN 3.4): running = accent
  (dot pulses via shared .u-pulse), success = --color-ok, vetoed = --color-warn
  (blocked, not failed), error = --color-error + subtle tint. Collapsed by
  default; click toggles a mono args/data payload. The live elapsed counter for
  running tools is tabular and honest (rig 0.35 dispatches tools sequentially,
  so a slow tool can block visible activity for seconds).
-->
<script lang="ts">
  import { getToolCardExpanded, setToolCardExpanded } from "$lib/stores/harness.svelte";
  import type { ToolResult } from "$lib/types/harness";

  let {
    cardKey,
    name,
    status,
    result,
    errorMessage,
    argumentsJson,
  }: {
    cardKey: string;
    name: string;
    status: "running" | "success" | "error" | "vetoed";
    result?: ToolResult;
    errorMessage?: string;
    argumentsJson?: string;
  } = $props();

  const expanded = $derived(getToolCardExpanded(cardKey, false));
  const kindBadge = $derived(result?.kind ?? name);
  const title = $derived(result?.title ?? name);
  const itemsCount = $derived(result?.itemsCount ?? null);
  const elapsedMs = $derived(result?.elapsedMs ?? null);

  // Live elapsed counter for running tools — rig 0.35 runs tool dispatch
  // sequentially inside the stream so a slow tool can block visible activity
  // for several seconds. Updating a few times a second gives the user proof the
  // backend is doing work instead of a frozen "running…" snippet.
  let liveStart = $state<number | null>(null);
  let liveElapsed = $state(0);
  $effect(() => {
    if (status === "running") {
      liveStart = performance.now();
      liveElapsed = 0;
      const id = window.setInterval(() => {
        if (liveStart === null) return;
        liveElapsed = Math.floor(performance.now() - liveStart);
      }, 200);
      return () => {
        window.clearInterval(id);
        liveStart = null;
      };
    }
    return undefined;
  });
  const runningLabel = $derived(
    status === "running" ? `running… ${(liveElapsed / 1000).toFixed(1)}s` : "",
  );
  const snippet = $derived(
    result?.snippet
      ?? (status === "error" ? (errorMessage ?? "error") : status === "running" ? runningLabel : ""),
  );

  function toggle() {
    setToolCardExpanded(cardKey, !expanded);
  }

  function prettyArgs(): string {
    if (!argumentsJson) return "";
    try {
      return JSON.stringify(JSON.parse(argumentsJson), null, 2);
    } catch {
      return argumentsJson;
    }
  }

  function prettyData(): string {
    if (!result) return "";
    try {
      return JSON.stringify(result.data, null, 2);
    } catch {
      return String(result.data);
    }
  }
</script>

<button
  class="tool-card status-{status}"
  type="button"
  onclick={toggle}
  aria-expanded={expanded}
>
  <div class="card-header">
    <span class="kind-badge">{kindBadge}</span>
    <span class="title">{title}</span>
    <span class="meta">
      {#if itemsCount !== null}<span class="pill">{itemsCount}</span>{/if}
      {#if elapsedMs !== null}<span class="elapsed">{elapsedMs} ms</span>{/if}
      <span class="status-dot status-dot-{status}" class:u-pulse={status === "running"}></span>
    </span>
  </div>
  {#if !expanded}
    <div class="snippet" class:snippet-running={status === "running"}>{snippet}</div>
  {:else}
    <div class="expanded">
      {#if argumentsJson}
        <div class="section-label">args</div>
        <pre class="pre">{prettyArgs()}</pre>
      {/if}
      {#if result}
        <div class="section-label">data</div>
        <pre class="pre">{prettyData()}</pre>
      {:else if errorMessage}
        <div class="section-label">error</div>
        <pre class="pre">{errorMessage}</pre>
      {/if}
    </div>
  {/if}
</button>

<style>
  .tool-card {
    display: block;
    width: 100%;
    text-align: left;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-1);
    color: var(--color-text-primary);
    padding: 8px 10px;
    cursor: pointer;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    margin: 4px 0;
    transition-property: background-color, border-color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .tool-card:hover {
    border-color: var(--color-border-hover);
  }
  /* Keep the fault tint on hover for error/vetoed; neutral cards lift to surface-2. */
  .tool-card:not(.status-error):not(.status-vetoed):hover {
    background: var(--color-surface-2);
  }
  /* Status trio (DESIGN 3.4) — running = accent, error = error + subtle tint,
   * vetoed = warn + subtle tint (blocked, not failed), success = neutral. */
  .tool-card.status-running {
    border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
  }
  .tool-card.status-error {
    border-color: color-mix(in srgb, var(--color-error) 50%, transparent);
    background: var(--color-error-subtle);
  }
  .tool-card.status-vetoed {
    border-color: color-mix(in srgb, var(--color-warn) 50%, transparent);
    background: var(--color-warn-subtle);
  }
  .card-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .kind-badge {
    background: var(--color-surface-2);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 600;
    flex-shrink: 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-weight: 500;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    flex-shrink: 0;
  }
  .pill {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
    padding: 0 5px;
    border-radius: var(--radius-xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .elapsed {
    font-variant-numeric: tabular-nums;
  }
  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-text-muted);
    flex-shrink: 0;
  }
  .status-dot-running {
    background: var(--color-accent);
  }
  .status-dot-success {
    background: var(--color-ok);
  }
  .status-dot-error {
    background: var(--color-error);
  }
  .status-dot-vetoed {
    background: var(--color-warn);
  }
  .snippet {
    margin-top: 5px;
    color: var(--color-text-secondary);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .snippet-running {
    color: var(--color-accent-text);
    font-variant-numeric: tabular-nums;
  }
  .expanded {
    margin-top: 6px;
  }
  .section-label {
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: 600;
    color: var(--color-text-muted);
    margin: 6px 0 2px;
  }
  .pre {
    background: var(--color-code-bg);
    color: var(--color-text-primary);
    border: 1px solid var(--color-code-border);
    border-radius: var(--radius-sm);
    padding: 7px 9px;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: 1.5;
    overflow-x: auto;
    max-height: 220px;
    overflow-y: auto;
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
