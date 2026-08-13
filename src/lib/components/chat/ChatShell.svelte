<!--
  @agent-context: Full-overlay harness shell. Header = identity titlebar (SKILL mark,
  wordmark, provider summary + memory pill) over a PHOSPHOR segmented control
  (Chat / Library Wizard / History / Settings). Esc closes (aborting any live
  turn via closeChat). History rows show relative time + message count + a
  ConfirmPopover-guarded delete. role=tablist/tab + aria-selected preserved.
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { onMount } from "svelte";
  import {
    closeChat,
    deleteSession,
    harnessStore,
    initHarness,
    loadSession,
    startNewSession,
  } from "$lib/stores/harness.svelte";
  import type { HarnessSession } from "$lib/types/harness";
  import MessageStream from "./MessageStream.svelte";
  import Composer from "./Composer.svelte";
  import Wizard from "./Wizard.svelte";
  import Settings from "./Settings.svelte";
  import ConfirmPopover from "../ConfirmPopover.svelte";

  onMount(() => {
    void initHarness();
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && harnessStore.open) {
      e.preventDefault();
      e.stopPropagation();
      closeChat();
    }
  }

  const TABS: { key: typeof harnessStore.view; label: string }[] = [
    { key: "chat", label: "Chat" },
    { key: "wizard", label: "Library Wizard" },
    { key: "history", label: "History" },
    { key: "settings", label: "Settings" },
  ];

  const providerSummary = $derived(
    harnessStore.activeProvider
      ? `${harnessStore.providers.find((p) => p.id === harnessStore.activeProvider)?.label ?? "?"} · ${harnessStore.activeModel ?? "(no model)"}`
      : "no provider",
  );

  // Memory-count pill so the user can SEE prior turns survive into the next
  // request. If this number stays at the same value across sends, memory is
  // actually working and the model just chose to ignore it.
  const memoryStats = $derived.by(() => {
    const msgs = harnessStore.activeSession?.memory.messages ?? [];
    const user = msgs.filter((m) => m.role === "user").length;
    const assistant = msgs.filter((m) => m.role === "assistant").length;
    return { user, assistant, total: msgs.length };
  });

  // Conversational message count for a history row (user + assistant, not tool
  // records). Guarded so an unpopulated list payload just shows nothing.
  function sessionMsgCount(s: HarnessSession): number {
    const msgs = s.memory?.messages ?? [];
    return msgs.filter((m) => m.role === "user" || m.role === "assistant").length;
  }

  // Relative, tabular timestamp for history rows. Backend epoch may be seconds
  // or ms — normalize both.
  function relativeTime(ts: number | null | undefined): string {
    if (!ts) return "";
    const ms = ts < 1e12 ? ts * 1000 : ts;
    const diff = Date.now() - ms;
    if (diff < 0) return "now";
    const s = Math.floor(diff / 1000);
    if (s < 45) return "just now";
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    const d = Math.floor(h / 24);
    if (d < 7) return `${d}d ago`;
    return new Date(ms).toLocaleDateString();
  }

  function absoluteTime(ts: number | null | undefined): string {
    if (!ts) return "";
    const ms = ts < 1e12 ? ts * 1000 : ts;
    return new Date(ms).toLocaleString();
  }

  function onRowKey(e: KeyboardEvent, id: string) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      void loadSession(id);
    }
  }

  // Delete confirmation (shared ConfirmPopover, anchored to the trash button).
  let deleteOpenFor = $state<string | null>(null);
  let deleteAnchorRect = $state<DOMRect | null>(null);

  function startDelete(e: MouseEvent, id: string) {
    e.stopPropagation();
    deleteAnchorRect = (e.currentTarget as HTMLElement)?.getBoundingClientRect() ?? null;
    deleteOpenFor = id;
  }

  async function confirmDelete() {
    const id = deleteOpenFor;
    if (!id) return;
    await deleteSession(id);
    deleteOpenFor = null;
    deleteAnchorRect = null;
  }

  function cancelDelete() {
    deleteOpenFor = null;
    deleteAnchorRect = null;
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if harnessStore.open}
  <div
    class="chat-overlay"
    transition:fly={{ y: 8, duration: 230, easing: cubicOut }}
    role="dialog"
    aria-modal="true"
    aria-label="Harness chat"
  >
    <header class="chat-header">
      <div class="titlebar">
        <span class="skill-mark" aria-hidden="true">SKILL</span>
        <div class="identity">
          <h2 class="wordmark">Speaking in Skills</h2>
          <p class="provider">
            <span class="provider-text">{providerSummary}</span>
            {#if memoryStats.total > 0}
              <span
                class="badge-meta mem-pill"
                title="Memory carried into this turn: {memoryStats.user} user · {memoryStats.assistant} assistant"
              >
                mem {memoryStats.user}u · {memoryStats.assistant}a
              </span>
            {/if}
          </p>
        </div>
        <button class="btn-icon hit-expand close" onclick={closeChat} aria-label="Close chat">
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
          </svg>
        </button>
      </div>

      <div class="tabbar" role="tablist" aria-label="Chat views">
        <div class="segmented">
          {#each TABS as tab (tab.key)}
            <button
              role="tab"
              aria-selected={harnessStore.view === tab.key}
              class="seg"
              class:active={harnessStore.view === tab.key}
              onclick={() => (harnessStore.view = tab.key)}
            >{tab.label}</button>
          {/each}
        </div>
      </div>
    </header>

    {#if harnessStore.view === "chat"}
      <MessageStream />
      <Composer />
    {:else if harnessStore.view === "wizard"}
      <Wizard />
    {:else if harnessStore.view === "history"}
      <div class="history-pane">
        <div class="action-row">
          <button class="btn-primary btn-sm" onclick={startNewSession}>New chat</button>
        </div>
        {#if harnessStore.sessions.length === 0}
          <div class="empty">No sessions yet. Send a message in Chat to start one.</div>
        {:else}
          {#each harnessStore.sessions as s (s.id)}
            <div
              class="session-row"
              class:active={harnessStore.activeSession?.id === s.id}
              role="button"
              tabindex="0"
              onclick={() => void loadSession(s.id)}
              onkeydown={(e) => onRowKey(e, s.id)}
            >
              <div class="row-main">
                <div class="title">{s.title || s.id}</div>
                <div class="meta">
                  <span class="meta-provider">{s.providerId} · {s.model}</span>
                  {#if sessionMsgCount(s) > 0}
                    <span class="dot" aria-hidden="true">·</span>
                    <span class="tnum">{sessionMsgCount(s)} msgs</span>
                  {/if}
                  {#if s.updatedAt}
                    <span class="dot" aria-hidden="true">·</span>
                    <time class="tnum" title={absoluteTime(s.updatedAt)}>{relativeTime(s.updatedAt)}</time>
                  {/if}
                </div>
              </div>
              <button
                class="btn-icon btn-icon-sm del"
                onclick={(e) => startDelete(e, s.id)}
                aria-label="Delete session"
              >
                <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                  <path
                    d="M3 4.5h10M6.5 4.5V3.6a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v.9M4.8 4.5l.45 8a1 1 0 0 0 1 .95h3.5a1 1 0 0 0 1-.95l.45-8"
                    stroke="currentColor"
                    stroke-width="1.3"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                  />
                </svg>
              </button>
            </div>
          {/each}
        {/if}

        <ConfirmPopover
          open={deleteOpenFor !== null}
          anchorRect={deleteAnchorRect}
          title="Delete this session?"
          message="The conversation and its memory are permanently removed."
          confirmLabel="Delete"
          tone="danger"
          onConfirm={confirmDelete}
          onCancel={cancelDelete}
        />
      </div>
    {:else}
      <Settings />
    {/if}
  </div>
{/if}

<style>
  /* The window frame is sacred chrome (DESIGN 10): radius-lg, hairline, one
   * modal-tier shadow. Components inside never draw a competing outer frame. */
  .chat-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    flex-direction: column;
    background: var(--color-surface-0);
    border-radius: var(--radius-lg);
    border: 1px solid var(--color-border);
    box-shadow: var(--shadow-modal);
    overflow: hidden;
  }

  /* === Header: identity titlebar over a segmented control ================ */
  .chat-header {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-1);
  }
  .titlebar {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .identity {
    flex: 1;
    min-width: 0;
  }
  .skill-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 20px;
    padding: 0 7px;
    border-radius: var(--radius-xs);
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.08em;
    color: var(--color-on-accent);
    background: var(--color-accent);
    flex-shrink: 0;
  }
  .wordmark {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-lg);
    line-height: var(--text-lg--line-height);
    font-weight: 600;
    color: var(--color-text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .provider {
    margin: 2px 0 0;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    color: var(--color-text-muted);
  }
  .provider-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .mem-pill {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }
  .close {
    flex-shrink: 0;
  }

  /* Segmented control (DESIGN 8) — radius-sm track, concentric radius-xs
   * segments, phosphor active state (raised surface-3 + accent underline).
   * NOT a 999px pill. */
  .tabbar {
    display: flex;
  }
  .segmented {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    max-width: 100%;
    overflow-x: auto;
  }
  .seg {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    padding: 4px 10px;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: 1;
    font-weight: 500;
    white-space: nowrap;
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition-property: background-color, color, box-shadow, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .seg:hover {
    color: var(--color-text-secondary);
  }
  .seg.active {
    background: var(--color-surface-3);
    color: var(--color-text-primary);
    box-shadow: inset 0 -2px 0 var(--color-accent);
  }
  .seg:active {
    scale: 0.96;
  }

  /* === History pane ====================================================== */
  .history-pane {
    flex: 1;
    overflow-y: auto;
    padding: 10px 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .action-row {
    margin-bottom: 2px;
  }
  .session-row {
    display: flex;
    align-items: center;
    gap: 8px;
    text-align: left;
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: 8px 8px 8px 10px;
    cursor: pointer;
    transition-property: background-color, border-color, box-shadow;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .session-row:hover {
    background: var(--color-surface-2);
    border-color: var(--color-border-hover);
  }
  /* Stronger active state: accent border + subtle fill + inset accent bar. */
  .session-row.active {
    border-color: var(--color-border-active);
    background: var(--color-accent-subtle);
    box-shadow: inset 2px 0 0 var(--color-accent);
  }
  .row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .session-row .title {
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    font-weight: 500;
    color: var(--color-text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .session-row .meta {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    color: var(--color-text-muted);
    min-width: 0;
  }
  .meta-provider {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .meta .dot {
    opacity: 0.5;
    flex-shrink: 0;
  }
  .tnum {
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  .btn-icon-sm {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
  }
  .del:hover {
    color: var(--color-error);
    background: var(--color-error-subtle);
  }
  .empty {
    color: var(--color-text-muted);
    text-align: center;
    padding: 24px 12px;
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
  }
</style>
