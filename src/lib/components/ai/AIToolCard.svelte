<!--
  @agent-context: Renders one tool call. The header has a single-line plain-
  English summary (e.g. "filters: installed-since, tag=rust · 24 skills");
  expanding reveals a structured preview (top-N skill list, group counts,
  generated prompt body) plus a raw JSON view as a last resort. Highlights
  use highlightCode(...).html — the previous version stringified the whole
  result object and rendered "[object Object]".
-->
<script lang="ts">
  import { formatToolCallArguments } from "$lib/stores/ai.svelte";
  import { highlightCode } from "$lib/utils/highlight";

  let {
    name,
    argumentsJson,
    status,
    label,
    data,
    errorMessage,
    initiallyExpanded = false,
  }: {
    name: string;
    argumentsJson: string;
    status: "running" | "success" | "error";
    label?: string;
    data?: unknown;
    errorMessage?: string;
    initiallyExpanded?: boolean;
  } = $props();

  // svelte-ignore state_referenced_locally
  let expanded = $state(initiallyExpanded);
  let showRaw = $state(false);

  const argsObject = $derived.by(() => {
    if (!argumentsJson) return null;
    try {
      return JSON.parse(argumentsJson);
    } catch {
      return null;
    }
  });

  const parsedData = $derived.by(() => {
    if (data === undefined || data === null) return null;
    if (typeof data === "string") {
      try {
        return JSON.parse(data);
      } catch {
        return data;
      }
    }
    return data;
  });

  const formattedArgs = $derived(formatToolCallArguments(argumentsJson));
  const formattedData = $derived(
    parsedData === null
      ? ""
      : typeof parsedData === "string"
        ? parsedData
        : JSON.stringify(parsedData, null, 2)
  );

  const argsHtml = $derived(highlightCode(formattedArgs, "json").html);
  const dataHtml = $derived(highlightCode(formattedData, "json").html);

  // Plain-English summary line for the header so the user reads what the
  // tool received without expanding the card.
  const argsSummary = $derived.by(() => {
    if (!argsObject || typeof argsObject !== "object") return "";
    const obj = argsObject as Record<string, unknown>;
    const parts: string[] = [];
    for (const [k, v] of Object.entries(obj)) {
      if (v === null || v === undefined || v === "" || v === false) continue;
      if (Array.isArray(v)) {
        parts.push(`${k}=[${v.length}]`);
      } else if (typeof v === "object") {
        parts.push(`${k}={…}`);
      } else if (typeof v === "string" && v.length > 36) {
        parts.push(`${k}="${v.slice(0, 33)}…"`);
      } else {
        parts.push(`${k}=${v}`);
      }
    }
    return parts.join(" · ");
  });

  // Build the structured preview of the result so users see a table or
  // bullet list instead of raw JSON.
  type ResultPreview =
    | { kind: "empty" }
    | { kind: "string"; value: string }
    | { kind: "error"; message: string }
    | {
        kind: "skills";
        count: number;
        filters: string[];
        skills: Array<{
          id?: string;
          name?: string;
          description?: string;
          agentId?: string;
          tags?: string[];
        }>;
      }
    | {
        kind: "stats";
        groupBy: string;
        total: number;
        groups: Array<{ key: string; count: number }>;
      }
    | { kind: "prompt"; target_agent?: string; skill_count?: number; prompt?: string }
    | { kind: "workflow"; intent?: string; steps: unknown[] }
    | { kind: "detail"; skill?: unknown; metadata?: unknown; body?: string }
    | { kind: "json"; value: unknown };

  const resultPreview: ResultPreview = $derived.by(() => {
    if (status === "error") return { kind: "error", message: errorMessage ?? "(no detail)" };
    if (parsedData === null) return { kind: "empty" };
    if (typeof parsedData === "string") return { kind: "string", value: parsedData };
    if (typeof parsedData !== "object") return { kind: "json", value: parsedData };
    const obj = parsedData as Record<string, unknown>;

    if (Array.isArray(obj["skills"])) {
      return {
        kind: "skills",
        count: typeof obj["count"] === "number" ? (obj["count"] as number) : (obj["skills"] as unknown[]).length,
        filters: Array.isArray(obj["filters_applied"]) ? (obj["filters_applied"] as string[]) : [],
        skills: (obj["skills"] as Array<Record<string, unknown>>).map((s) => ({
          id: typeof s.id === "string" ? s.id : undefined,
          name: typeof s.name === "string" ? s.name : undefined,
          description: typeof s.description === "string" ? s.description : undefined,
          agentId: typeof s.agentId === "string" ? s.agentId : undefined,
          tags: Array.isArray(s.tags) ? (s.tags as string[]) : undefined,
        })),
      };
    }

    if (typeof obj["group_by"] === "string" && Array.isArray(obj["groups"])) {
      return {
        kind: "stats",
        groupBy: obj["group_by"] as string,
        total: typeof obj["total_count"] === "number" ? (obj["total_count"] as number) : 0,
        groups: (obj["groups"] as Array<Record<string, unknown>>).map((g) => ({
          key: String(g["key"] ?? ""),
          count: typeof g["count"] === "number" ? (g["count"] as number) : 0,
        })),
      };
    }

    if (typeof obj["prompt"] === "string") {
      return {
        kind: "prompt",
        target_agent: typeof obj["target_agent"] === "string" ? (obj["target_agent"] as string) : undefined,
        skill_count: typeof obj["skill_count"] === "number" ? (obj["skill_count"] as number) : undefined,
        prompt: obj["prompt"] as string,
      };
    }

    if (Array.isArray(obj["steps"])) {
      return {
        kind: "workflow",
        intent: typeof obj["intent"] === "string" ? (obj["intent"] as string) : undefined,
        steps: obj["steps"] as unknown[],
      };
    }

    if (obj["skill"] && typeof obj["body"] === "string") {
      return {
        kind: "detail",
        skill: obj["skill"],
        metadata: obj["metadata"],
        body: obj["body"] as string,
      };
    }

    return { kind: "json", value: parsedData };
  });

  const accent = $derived(
    status === "error"
      ? "var(--color-danger, #d97373)"
      : status === "running"
        ? "var(--color-accent-muted, #7a7fad)"
        : "var(--color-accent)"
  );

  function copyToClipboard(text: string) {
    void navigator.clipboard.writeText(text);
  }
</script>

<div
  class="tool-card mt-1.5 overflow-hidden rounded-md border text-[10.5px]"
  style="border-color: var(--color-border); background: var(--color-surface-1);"
>
  <button
    class="tool-card-header"
    onclick={() => (expanded = !expanded)}
  >
    <span class="left">
      <span
        class="h-1.5 w-1.5 rounded-full {status === 'running' ? 'tool-pulse' : ''}"
        style="background: {accent};"
      ></span>
      <span class="name">{name}</span>
      {#if argsSummary}
        <span class="muted">— {argsSummary}</span>
      {/if}
    </span>
    <span class="right">
      {#if label}
        <span class="label-tag">{label}</span>
      {/if}
      <span class="status-tag">
        {status === "running" ? "running" : status === "error" ? "error" : "done"}
      </span>
      <span class="chev">{expanded ? "−" : "+"}</span>
    </span>
  </button>

  {#if expanded}
    <div class="tool-body">
      {#if resultPreview.kind === "error"}
        <div class="section-label">error</div>
        <div class="err">{resultPreview.message}</div>
      {:else if resultPreview.kind === "skills"}
        <div class="result-head">
          <span><strong>{resultPreview.count}</strong> skill(s)</span>
          {#if resultPreview.filters.length > 0}
            <span class="muted">filters: {resultPreview.filters.join(", ")}</span>
          {/if}
        </div>
        <ul class="skill-list">
          {#each resultPreview.skills.slice(0, 12) as s, i (i)}
            <li>
              <span class="row-name">{s.name ?? "(unnamed)"}</span>
              {#if s.agentId}
                <span class="row-agent">{s.agentId}</span>
              {/if}
              {#if s.description}
                <span class="row-desc">{s.description.slice(0, 140)}{s.description.length > 140 ? "…" : ""}</span>
              {/if}
              {#if s.tags && s.tags.length > 0}
                <span class="row-tags">
                  {#each s.tags.slice(0, 6) as t (t)}
                    <span class="tag">{t}</span>
                  {/each}
                  {#if s.tags.length > 6}
                    <span class="tag muted-tag">+{s.tags.length - 6}</span>
                  {/if}
                </span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if resultPreview.skills.length > 12}
          <div class="muted more">+{resultPreview.skills.length - 12} more skill(s) in raw view</div>
        {/if}
      {:else if resultPreview.kind === "stats"}
        <div class="result-head">
          <span>group_by <strong>{resultPreview.groupBy}</strong> · total {resultPreview.total}</span>
        </div>
        <ol class="stats-list">
          {#each resultPreview.groups as g, i (i)}
            <li>
              <span class="row-name">{g.key}</span>
              <span class="row-count">{g.count}</span>
            </li>
          {/each}
        </ol>
      {:else if resultPreview.kind === "prompt"}
        <div class="result-head">
          <span>target: <strong>{resultPreview.target_agent ?? "?"}</strong></span>
          {#if resultPreview.skill_count !== undefined}
            <span class="muted">{resultPreview.skill_count} skill(s)</span>
          {/if}
          <button class="copy-btn" onclick={() => copyToClipboard(resultPreview.prompt ?? "")}>copy</button>
        </div>
        <pre class="prompt-block">{resultPreview.prompt ?? ""}</pre>
      {:else if resultPreview.kind === "workflow"}
        <div class="result-head">
          {#if resultPreview.intent}
            <span>intent: <em>“{resultPreview.intent}”</em></span>
          {/if}
        </div>
        <ol class="workflow-list">
          {#each resultPreview.steps as step, i (i)}
            <li>
              <pre class="step-json">{JSON.stringify(step, null, 2)}</pre>
            </li>
          {/each}
        </ol>
      {:else if resultPreview.kind === "detail"}
        <pre class="detail-meta">{JSON.stringify(resultPreview.metadata ?? {}, null, 2)}</pre>
        <div class="section-label">body</div>
        <pre class="detail-body">{resultPreview.body}</pre>
      {:else if resultPreview.kind === "string"}
        <pre class="prompt-block">{resultPreview.value}</pre>
      {:else if resultPreview.kind === "empty"}
        <div class="muted">(no result yet)</div>
      {:else}
        <pre class="tool-pre">{@html dataHtml}</pre>
      {/if}

      <div class="raw-toggle">
        <button class="raw-btn" onclick={() => (showRaw = !showRaw)}>
          {showRaw ? "hide raw JSON" : "show raw JSON"}
        </button>
      </div>

      {#if showRaw}
        <div class="section-label">arguments</div>
        <pre class="tool-pre">{@html argsHtml}</pre>
        {#if formattedData.length > 0 && resultPreview.kind !== "error"}
          <div class="section-label">result</div>
          <pre class="tool-pre">{@html dataHtml}</pre>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .tool-card-header {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    text-align: left;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease;
  }
  .tool-card-header:hover {
    background: var(--color-surface-2);
  }
  .left {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .name {
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .muted {
    color: var(--color-text-muted);
    font-size: 10px;
  }
  .label-tag {
    font-size: 9.5px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }
  .status-tag {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
  }
  .chev {
    font-size: 14px;
    color: var(--color-text-muted);
    margin-left: 2px;
  }
  .tool-body {
    border-top: 1px solid var(--color-border);
    padding: 8px 10px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .section-label {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
  }
  .result-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    font-size: 11px;
    color: var(--color-text-primary);
  }
  .skill-list,
  .stats-list,
  .workflow-list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .skill-list li {
    display: grid;
    grid-template-columns: max-content max-content 1fr;
    gap: 6px 10px;
    align-items: start;
    padding: 6px 8px;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background: var(--color-surface-0);
    font-size: 11px;
  }
  .row-name {
    grid-column: 1;
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .row-agent {
    grid-column: 2;
    font-size: 9.5px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--color-surface-2);
    color: var(--color-text-secondary);
    align-self: start;
  }
  .row-desc {
    grid-column: 1 / -1;
    color: var(--color-text-secondary);
    font-size: 10.5px;
    line-height: 1.45;
  }
  .row-tags {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag {
    font-size: 9.5px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--color-surface-2);
    color: var(--color-text-secondary);
  }
  .muted-tag {
    color: var(--color-text-muted);
  }
  .stats-list li {
    display: flex;
    justify-content: space-between;
    padding: 4px 8px;
    background: var(--color-surface-0);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 11px;
  }
  .row-count {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-muted);
  }
  .copy-btn {
    margin-left: auto;
    height: 22px;
    padding: 0 8px;
    border-radius: 999px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-text-primary);
    font-size: 10px;
    cursor: pointer;
  }
  .copy-btn:hover { background: var(--color-surface-3); }
  .prompt-block,
  .detail-body,
  .detail-meta,
  .step-json {
    margin: 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--color-surface-0);
    border: 1px solid var(--color-border);
    font-family: ui-monospace, SFMono-Regular, "JetBrains Mono", Consolas, monospace;
    font-size: 11px;
    line-height: 1.5;
    color: var(--color-text-primary);
    white-space: pre-wrap;
    overflow: auto;
    max-height: 360px;
  }
  .step-json {
    max-height: 200px;
  }
  .raw-toggle {
    display: flex;
    justify-content: flex-end;
  }
  .raw-btn {
    height: 20px;
    padding: 0 8px;
    border-radius: 999px;
    border: 1px dashed var(--color-border);
    background: transparent;
    color: var(--color-text-muted);
    font-size: 10px;
    cursor: pointer;
  }
  .raw-btn:hover { color: var(--color-text-secondary); }
  .tool-pre {
    margin: 0;
    max-height: 240px;
    overflow: auto;
    white-space: pre;
    font-family: ui-monospace, SFMono-Regular, "JetBrains Mono", Consolas, monospace;
    font-size: 10.5px;
    line-height: 1.5;
    color: var(--color-text-primary);
  }
  .err {
    color: var(--color-danger, #d97373);
    font-size: 11px;
  }
  .tool-pulse {
    animation: tool-pulse 1.1s ease-in-out infinite;
  }
  .more {
    font-size: 10px;
  }
  @keyframes tool-pulse {
    0%, 100% { opacity: 0.4; transform: scale(0.85); }
    50% { opacity: 1; transform: scale(1.15); }
  }
</style>
