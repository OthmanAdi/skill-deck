<!--
  @agent-context: Diagnostics for the most recent agent turn — visible
  proof of what the agent did, why, how many tool calls fired, and how
  long each took. Solves "the agent looks silent / lazy / dumb" by
  exposing the actual loop.
-->
<script lang="ts">
  import { aiStore } from "$lib/stores/ai.svelte";

  function ms(value: number): string {
    if (!Number.isFinite(value) || value < 0) return "—";
    if (value < 1000) return `${Math.round(value)} ms`;
    return `${(value / 1000).toFixed(2)} s`;
  }

  const tel = $derived(aiStore.activeSession?.lastTelemetry ?? null);
</script>

<div class="debug-pane">
  {#if !aiStore.activeSession}
    <div class="empty">No active session. Run a chat turn first.</div>
  {:else if !tel}
    <div class="empty">No telemetry recorded yet — send a message in the Chat tab.</div>
  {:else}
    <section class="card">
      <header>
        <h3>Last turn</h3>
        <span class="badge">{tel.iterations} iter · {tel.toolDispatches.length} tool call(s)</span>
      </header>
      <div class="user-quote">
        “{tel.userText}”
      </div>
      <dl>
        <div><dt>Total duration</dt><dd>{ms(tel.durationMs)}</dd></div>
        <div><dt>Iterations</dt><dd>{tel.iterations}</dd></div>
        <div><dt>Stream chunks</dt><dd>{tel.totalChunks}</dd></div>
        <div><dt>Assistant chars</dt><dd>{tel.totalAssistantChars}</dd></div>
        <div><dt>Finish reason</dt><dd>{tel.finishReason ?? "—"}</dd></div>
        {#if tel.error}
          <div class="error-row"><dt>Error</dt><dd>{tel.error}</dd></div>
        {/if}
      </dl>
    </section>

    <section class="card">
      <header>
        <h3>Tool dispatches</h3>
        <span class="badge">{tel.toolDispatches.length}</span>
      </header>
      {#if tel.toolDispatches.length === 0}
        <div class="empty inset">
          The model did not call any tool this turn. This is usually a sign
          the model decided the question was conversational — or it
          hallucinated an answer. Re-ask with a phrasing like
          “search my skills for X” to nudge it.
        </div>
      {:else}
        <ol class="dispatches">
          {#each tel.toolDispatches as d, i (i)}
            <li class={d.success ? "ok" : "fail"}>
              <span class="dot"></span>
              <span class="name">{d.name}</span>
              {#if d.label}
                <span class="label">{d.label}</span>
              {/if}
              <span class="dur">{ms(d.durationMs)}</span>
              {#if !d.success && d.error}
                <div class="err">{d.error}</div>
              {/if}
            </li>
          {/each}
        </ol>
      {/if}
    </section>

    <section class="card">
      <header>
        <h3>Session</h3>
        <span class="badge">{aiStore.activeSession.messages.length} message(s)</span>
      </header>
      <dl>
        <div><dt>Session id</dt><dd class="mono">{aiStore.activeSession.id || "(pending)"}</dd></div>
        <div><dt>Provider</dt><dd>{aiStore.activeSession.providerId}</dd></div>
        <div><dt>Model</dt><dd>{aiStore.activeSession.model}</dd></div>
      </dl>
    </section>
  {/if}
</div>

<style>
  .debug-pane {
    padding: 12px;
    color: var(--color-text-primary);
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .empty {
    padding: 16px;
    text-align: center;
    color: var(--color-text-muted);
    font-size: 11.5px;
    border: 1px dashed var(--color-border);
    border-radius: 10px;
  }
  .empty.inset {
    border: none;
    padding: 8px 0;
    text-align: left;
  }
  .card {
    border: 1px solid var(--color-border);
    border-radius: 12px;
    background: var(--color-surface-1);
    overflow: hidden;
  }
  .card header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
  }
  .card h3 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
  }
  .badge {
    font-size: 10px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--color-surface-0);
    color: var(--color-text-muted);
  }
  .user-quote {
    padding: 8px 12px;
    border-bottom: 1px solid var(--color-border);
    font-style: italic;
    color: var(--color-text-secondary);
  }
  dl {
    margin: 0;
    padding: 8px 12px;
    display: grid;
    grid-template-columns: 1fr;
    gap: 4px;
  }
  dl > div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 11px;
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0;
    color: var(--color-text-primary);
    text-align: right;
  }
  dd.mono {
    font-family: ui-monospace, SFMono-Regular, "JetBrains Mono", Consolas, monospace;
    font-size: 10.5px;
  }
  .error-row dd {
    color: var(--color-danger, #d97373);
  }
  .dispatches {
    margin: 0;
    padding: 6px 12px 10px 28px;
    counter-reset: dispatch;
  }
  .dispatches li {
    display: grid;
    grid-template-columns: 12px 1fr auto;
    align-items: center;
    column-gap: 8px;
    padding: 6px 0;
    border-bottom: 1px dashed var(--color-border);
    font-size: 11.5px;
  }
  .dispatches li:last-child {
    border-bottom: none;
  }
  .dot {
    height: 6px;
    width: 6px;
    border-radius: 50%;
    background: var(--color-accent);
  }
  .dispatches li.fail .dot {
    background: var(--color-danger, #d97373);
  }
  .name {
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .label {
    grid-column: 2 / 3;
    grid-row: 2;
    color: var(--color-text-muted);
    font-size: 10.5px;
    margin-top: 2px;
  }
  .dur {
    color: var(--color-text-muted);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .err {
    grid-column: 1 / -1;
    margin-top: 2px;
    color: var(--color-danger, #d97373);
    font-size: 10.5px;
  }
</style>
