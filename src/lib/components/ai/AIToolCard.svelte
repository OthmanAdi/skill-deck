<!--
  @agent-context: Renders a single tool call (running, success, or error)
  as a collapsible card inside an assistant message.
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

  const formattedArgs = $derived(formatToolCallArguments(argumentsJson));
  const formattedData = $derived(
    data === undefined || data === null
      ? ""
      : typeof data === "string"
        ? data
        : JSON.stringify(data, null, 2)
  );

  const argsHtml = $derived(highlightCode(formattedArgs, "json"));
  const dataHtml = $derived(highlightCode(formattedData, "json"));

  const accent = $derived(
    status === "error"
      ? "var(--color-danger, #d97373)"
      : status === "running"
        ? "var(--color-accent-muted, #7a7fad)"
        : "var(--color-accent)"
  );
</script>

<div
  class="tool-card mt-1.5 overflow-hidden rounded-md border text-[10.5px]"
  style="border-color: var(--color-border); background: var(--color-surface-1);"
>
  <button
    class="flex w-full items-center justify-between gap-2 px-2 py-1.5 text-left transition-colors hover:bg-[var(--color-surface-2)]"
    onclick={() => (expanded = !expanded)}
  >
    <span class="flex items-center gap-1.5">
      <span
        class="h-1.5 w-1.5 rounded-full {status === 'running' ? 'tool-pulse' : ''}"
        style="background: {accent};"
      ></span>
      <span class="font-medium text-[var(--color-text-primary)]">{name}</span>
      {#if label}
        <span class="text-[var(--color-text-muted)]">— {label}</span>
      {/if}
    </span>
    <span class="text-[9px] uppercase tracking-wide text-[var(--color-text-muted)]">
      {status === "running" ? "running" : status === "error" ? "error" : "done"}
      <span class="ml-1">{expanded ? "−" : "+"}</span>
    </span>
  </button>

  {#if expanded}
    <div class="border-t px-2 py-2" style="border-color: var(--color-border);">
      <div class="mb-1 text-[9px] uppercase tracking-wide text-[var(--color-text-muted)]">arguments</div>
      <pre class="tool-pre"><!-- eslint-disable-line --
        -->{@html argsHtml}</pre>

      {#if status === "error"}
        <div class="mt-2 mb-1 text-[9px] uppercase tracking-wide text-[var(--color-text-muted)]">error</div>
        <div class="text-[10.5px]" style="color: var(--color-danger, #d97373);">{errorMessage ?? "(no detail)"}</div>
      {:else if formattedData.length > 0}
        <div class="mt-2 mb-1 text-[9px] uppercase tracking-wide text-[var(--color-text-muted)]">result</div>
        <pre class="tool-pre"><!-- eslint-disable-line --
          -->{@html dataHtml}</pre>
      {/if}
    </div>
  {/if}
</div>

<style>
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
  .tool-pulse {
    animation: tool-pulse 1.1s ease-in-out infinite;
  }
  @keyframes tool-pulse {
    0%, 100% {
      opacity: 0.4;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.15);
    }
  }
</style>
