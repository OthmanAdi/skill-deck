<!--
  @agent-context: Session transcript renderer — a terminal-style operator log,
  NOT a chat-bubble clone. Every turn is a full-width block behind a mono role
  gutter (› user, ∎ assistant in phosphor, · tool). Markdown flows through
  renderChatMarkdown (bold / lists / headings / code / links) in both themes.
  Selection stays on via .selectable. Live region announces streaming to AT.
  Renders: anchored-summary pill, persisted + pending turns, the bespoke
  phosphor "thinking" cursor + elapsed seconds, the final envelope (suggested
  prompts + sources), a friendly + retryable error pill, and the first-run
  empty state (feature intro + example prompts + Library Wizard pointer).
-->
<script lang="ts">
  import {
    currentEnvelope,
    harnessStore,
    prefillComposer,
    retryLastTurn,
    sendMessage,
  } from "$lib/stores/harness.svelte";
  import ToolCard from "./ToolCard.svelte";
  import { renderChatMarkdown } from "$lib/utils/renderChatMarkdown";
  import { openUrl } from "@tauri-apps/plugin-opener";

  let scrollEl: HTMLDivElement | undefined = $state();
  let rawOpen = $state(false);

  const EXAMPLE_PROMPTS = [
    "What skills do I have for writing tests?",
    "Find a marketplace skill for changelogs",
    "Search the web for MCP server examples",
  ];

  const providerReady = $derived(
    Boolean(harnessStore.activeProvider && harnessStore.activeModel),
  );

  const showEmptyState = $derived(
    !harnessStore.activeSession && !harnessStore.pending && !harnessStore.lastError,
  );

  // Whether the collapsed "details" line adds anything beyond the friendly copy.
  const hasRawDetail = $derived(
    Boolean(
      harnessStore.lastErrorRaw &&
        harnessStore.lastErrorRaw.trim() &&
        harnessStore.lastErrorRaw !== harnessStore.lastError,
    ),
  );

  $effect(() => {
    if (!scrollEl) return;
    // Re-run on any change that could push the bottom further down.
    harnessStore.activeSession?.memory.messages.length;
    harnessStore.pending?.userText;
    harnessStore.pending?.text;
    harnessStore.pending?.toolCalls.length;
    harnessStore.lastError;
    // Double-rAF so the new DOM node is laid out before we scroll past it.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
      });
    });
  });

  // Bespoke "thinking" state: an honest elapsed counter (tabular) beside a
  // pulsing phosphor block cursor. Replaces the generic 3-dot bounce and reuses
  // the same truth-telling treatment ToolCard uses for slow sequential tools.
  let thinkingElapsed = $state(0);
  $effect(() => {
    if (harnessStore.isStreaming) {
      const start = performance.now();
      thinkingElapsed = 0;
      const id = window.setInterval(() => {
        thinkingElapsed = (performance.now() - start) / 1000;
      }, 100);
      return () => window.clearInterval(id);
    }
    return undefined;
  });

  // Dedupe: persisted tool record with the same callId wins over pending.
  const persistedCallIds = $derived(
    new Set(
      (harnessStore.activeSession?.memory.messages ?? [])
        .filter((m) => m.role === "tool" && m.toolCallId)
        .map((m) => m.toolCallId as string),
    ),
  );

  const pendingFresh = $derived(
    (harnessStore.pending?.toolCalls ?? []).filter((c) => !persistedCallIds.has(c.callId)),
  );

  function renderHtml(text: string): string {
    return renderChatMarkdown(text ?? "").html;
  }

  // Relative timestamp for a turn. Backend createdAt is epoch (seconds or ms —
  // normalize both); shown muted, revealed on hover so it adds no default noise.
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

  // Intercept clicks inside markdown turns: anchors open in the system browser
  // (Tauri opener plugin), copy buttons write their `data-copy` payload to the
  // clipboard. Both must short-circuit because the turn is locked in the webview.
  function onBubbleClick(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (!target) return;

    const copyBtn = target.closest("[data-copy]") as HTMLElement | null;
    if (copyBtn) {
      const payload = copyBtn.getAttribute("data-copy") ?? "";
      if (payload) {
        e.preventDefault();
        const writeText = navigator.clipboard?.writeText?.bind(navigator.clipboard);
        if (writeText) {
          void writeText(payload)
            .then(() => {
              copyBtn.classList.add("copied");
              const original = copyBtn.textContent;
              copyBtn.textContent = "copied";
              window.setTimeout(() => {
                copyBtn.classList.remove("copied");
                copyBtn.textContent = original ?? "copy";
              }, 1200);
            })
            .catch(() => {
              // Clipboard write failed — no user feedback needed for a
              // desktop Tauri app where this should always work.
            });
        }
        return;
      }
    }

    const anchor = target.closest("a") as HTMLAnchorElement | null;
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    if (!href) return;
    if (href.startsWith("#") || href.startsWith("javascript:")) return;
    e.preventDefault();
    void openUrl(href).catch(() => {
      // Fallback: do nothing rather than crash. The link survives in the DOM
      // so right-click → "copy link" still works.
    });
  }
</script>

<div
  class="stream selectable"
  bind:this={scrollEl}
  role="log"
  aria-live="polite"
  aria-relevant="additions text"
  aria-label="Conversation transcript"
>
  {#if showEmptyState}
    <div class="empty">
      <div class="empty-mark" aria-hidden="true">∎</div>
      <h2 class="empty-title">Speaking in Skills</h2>
      <p class="empty-sub">
        Ask in plain language. The agent searches your local skills, the
        marketplaces, and the open web, then answers in skills.
      </p>
      {#if providerReady}
        <div class="empty-prompts">
          {#each EXAMPLE_PROMPTS as p (p)}
            <button class="example-chip" onclick={() => prefillComposer(p)}>{p}</button>
          {/each}
        </div>
        <button class="wizard-link" onclick={() => (harnessStore.view = "wizard")}>
          Building a team skill library? Open the Library Wizard →
        </button>
      {:else}
        <div class="empty-noprovider">
          <p class="noprovider-line">No model provider connected yet.</p>
          <button class="btn-primary" onclick={() => (harnessStore.view = "settings")}>
            Set up a provider
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if harnessStore.activeSession?.memory.runningSummary}
    <div class="sys-pill" title="Anchored summary persisted across compactions">
      <span class="sys-caps">anchored summary</span>
      <span class="sys-text"
        >{harnessStore.activeSession.memory.runningSummary.slice(0, 240)}{harnessStore
          .activeSession.memory.runningSummary.length > 240
          ? "…"
          : ""}</span
      >
    </div>
  {/if}

  {#if harnessStore.activeSession}
    {#each harnessStore.activeSession.memory.messages as msg (msg.id)}
      {#if msg.role === "user"}
        <div class="turn turn-user">
          <span class="gutter" aria-hidden="true">›</span>
          <div class="body chat-md" onclick={onBubbleClick} role="presentation">
            {@html renderHtml(msg.text)}
          </div>
          <time class="turn-time" title={absoluteTime(msg.createdAt)}>{relativeTime(msg.createdAt)}</time>
        </div>
      {:else if msg.role === "assistant"}
        <div class="turn turn-assistant">
          <span class="gutter gutter-accent" aria-hidden="true">∎</span>
          <div class="body chat-md" onclick={onBubbleClick} role="presentation">
            {@html renderHtml(msg.text)}
          </div>
          <time class="turn-time" title={absoluteTime(msg.createdAt)}>{relativeTime(msg.createdAt)}</time>
        </div>
      {:else if msg.role === "tool"}
        {#if msg.cleared}
          <div class="turn turn-tool">
            <span class="gutter gutter-dim" aria-hidden="true">·</span>
            <div class="tool-note">tool result cleared by compaction</div>
          </div>
        {:else if msg.spilledPath}
          <div class="turn turn-tool">
            <span class="gutter gutter-dim" aria-hidden="true">·</span>
            <div class="tool-note">tool result spilled to disk: {msg.spilledPath}</div>
          </div>
        {:else}
          <div class="turn turn-tool">
            <span class="gutter gutter-dim" aria-hidden="true">·</span>
            <div class="tool-body">
              <ToolCard
                cardKey={`persist:${msg.id}`}
                name={msg.name ?? "tool"}
                status={msg.toolError ? "error" : "success"}
                result={msg.toolResult ?? undefined}
                errorMessage={msg.toolError ?? undefined}
                argumentsJson={msg.argsJson ?? undefined}
              />
            </div>
          </div>
        {/if}
      {/if}
    {/each}
  {/if}

  {#if harnessStore.pending}
    {#if harnessStore.pending.userText}
      <div class="turn turn-user" class:turn-failed={harnessStore.pending.failed}>
        <span class="gutter" aria-hidden="true">›</span>
        <div class="body chat-md" onclick={onBubbleClick} role="presentation">
          {@html renderHtml(harnessStore.pending.userText)}
        </div>
        {#if harnessStore.pending.failed}
          <span class="badge-signal is-error turn-failed-badge">not sent</span>
        {/if}
      </div>
    {/if}
    {#each pendingFresh as call (call.callId)}
      <div class="turn turn-tool">
        <span class="gutter gutter-dim" aria-hidden="true">·</span>
        <div class="tool-body">
          <ToolCard
            cardKey={`${harnessStore.pending.id}:${call.callId}`}
            name={call.name}
            status={call.status}
            result={call.result}
            errorMessage={call.errorMessage}
            argumentsJson={call.argumentsJson}
          />
        </div>
      </div>
    {/each}
    {#if harnessStore.pending.text.length > 0}
      <div class="turn turn-assistant">
        <span class="gutter gutter-accent" aria-hidden="true">∎</span>
        <div class="body chat-md" onclick={onBubbleClick} role="presentation">
          {@html renderHtml(harnessStore.pending.text)}{#if harnessStore.isStreaming}<span
              class="cursor-block u-pulse"
              aria-hidden="true"
            ></span>{/if}
        </div>
      </div>
    {:else if harnessStore.isStreaming}
      <div class="turn turn-assistant">
        <span class="gutter gutter-accent" aria-hidden="true">∎</span>
        <div class="thinking">
          <span class="cursor-block u-pulse" aria-hidden="true"></span>
          <span class="thinking-text">thinking</span>
          <span class="thinking-elapsed">{thinkingElapsed.toFixed(1)}s</span>
        </div>
      </div>
    {/if}
    {#if harnessStore.pending.compactionEvents > 0}
      <div class="sys-pill">
        <span class="sys-caps">compacted</span>
        <span class="sys-text tnum">{harnessStore.pending.compactionEvents}×</span>
        <span class="sys-text">this turn</span>
      </div>
    {/if}
  {/if}

  {#if !harnessStore.isStreaming && currentEnvelope()}
    {@const env = currentEnvelope()!}
    {#if env.suggestedNextPrompts.length > 0 || env.sources.length > 0}
      <div class="envelope">
        {#if env.suggestedNextPrompts.length > 0}
          <div class="envelope-section">
            <div class="envelope-label">Suggested next prompts</div>
            <div class="envelope-chips">
              {#each env.suggestedNextPrompts as p (p)}
                <button class="suggest-chip" onclick={() => void sendMessage(p)} title={p}>
                  {p}
                </button>
              {/each}
            </div>
          </div>
        {/if}
        {#if env.sources.length > 0}
          <div class="envelope-section">
            <div class="envelope-label">Sources <span class="tnum">({env.sources.length})</span></div>
            <div class="envelope-sources">
              {#each env.sources.slice(0, 8) as src (src.url)}
                <a class="source-link" href={src.url} onclick={onBubbleClick} title={src.url}>
                  <span class="source-kind">{src.kind}</span>
                  <span class="source-title">{src.title}</span>
                </a>
              {/each}
              {#if env.sources.length > 8}
                <span class="source-more tnum">+ {env.sources.length - 8} more</span>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  {#if harnessStore.lastError}
    <div class="error-pill" role="alert">
      <span class="error-icon" aria-hidden="true">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
          <path d="M8 1.5 15 14H1L8 1.5Z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" />
          <path d="M8 6.5v3.2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          <circle cx="8" cy="11.8" r="0.85" fill="currentColor" />
        </svg>
      </span>
      <div class="error-body">
        <span class="error-msg">{harnessStore.lastError}</span>
        {#if hasRawDetail}
          <button class="error-details" onclick={() => (rawOpen = !rawOpen)} aria-expanded={rawOpen}>
            {rawOpen ? "hide details" : "details"}
          </button>
          {#if rawOpen}
            <pre class="error-raw">{harnessStore.lastErrorRaw}</pre>
          {/if}
        {/if}
      </div>
      {#if harnessStore.lastFailedUserText}
        <button
          class="btn-secondary btn-sm error-retry"
          onclick={() => void retryLastTurn()}
          disabled={harnessStore.isStreaming}
        >
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
            <path d="M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
          Retry
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .stream {
    flex: 1;
    overflow-y: auto;
    padding: 14px 14px 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-family: var(--font-ui);
  }

  /* === Turn anatomy: mono role gutter + full-width body =================
   * No bubbles, no left/right alignment. A terminal session, not a DM. */
  .turn {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr);
    gap: 8px;
    position: relative;
  }
  /* Hairline separates each exchange (a new user query opens one). */
  .turn-user:not(:first-child) {
    margin-top: 4px;
    padding-top: 10px;
    border-top: 1px solid var(--color-border);
  }
  .gutter {
    font-family: var(--font-ui);
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    color: var(--color-text-muted);
    user-select: none;
    text-align: center;
  }
  .gutter-accent {
    color: var(--color-accent-text);
    font-weight: 600;
  }
  .gutter-dim {
    color: var(--color-text-muted);
    opacity: 0.5;
  }

  .body {
    min-width: 0;
    font-family: var(--font-ui);
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    word-break: break-word;
  }
  .turn-user .body {
    color: var(--color-text-secondary);
  }
  .turn-assistant .body {
    color: var(--color-text-primary);
  }
  .turn-failed .body {
    opacity: 0.7;
  }
  .turn-failed .gutter {
    color: var(--color-error);
  }
  .turn-failed-badge {
    position: absolute;
    top: 0;
    right: 0;
  }

  /* Per-turn timestamp — muted, tabular, revealed on hover only. */
  .turn-time {
    position: absolute;
    top: 2px;
    right: 0;
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-standard);
    pointer-events: none;
    user-select: none;
  }
  .turn:hover .turn-time {
    opacity: 0.7;
  }

  .tool-body {
    min-width: 0;
  }
  .tool-note {
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    color: var(--color-text-muted);
    font-style: italic;
    align-self: center;
  }

  /* === Thinking state: pulsing phosphor block cursor + honest elapsed ==== */
  .thinking {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    color: var(--color-text-muted);
  }
  .thinking-text {
    color: var(--color-text-secondary);
  }
  .thinking-elapsed {
    font-variant-numeric: tabular-nums;
    color: var(--color-text-muted);
    font-size: var(--text-xs);
  }
  .cursor-block {
    display: inline-block;
    width: 7px;
    height: 14px;
    vertical-align: -2px;
    background: var(--color-accent);
    border-radius: 1px;
  }

  /* === First-run / new-chat empty state ================================= */
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: 10px;
    padding: 24px 20px;
  }
  .empty > * {
    animation: empty-in var(--motion-enter) var(--ease-emphasized) both;
  }
  .empty-mark {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    line-height: 1;
    color: var(--color-accent-text);
    animation-delay: 0ms;
  }
  .empty-title {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--text-xl);
    line-height: var(--text-xl--line-height);
    font-weight: 600;
    color: var(--color-text-primary);
    text-wrap: balance;
    animation-delay: 40ms;
  }
  .empty-sub {
    margin: 0;
    max-width: 340px;
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    color: var(--color-text-muted);
    text-wrap: pretty;
    animation-delay: 80ms;
  }
  .empty-prompts {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    max-width: 340px;
    margin-top: 4px;
    animation-delay: 120ms;
  }
  .example-chip {
    width: 100%;
    text-align: left;
    padding: 8px 11px;
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition-property: background-color, border-color, color, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .example-chip::before {
    content: "› ";
    color: var(--color-text-muted);
  }
  .example-chip:hover {
    background: var(--color-surface-2);
    border-color: var(--color-border-hover);
    color: var(--color-text-primary);
  }
  .example-chip:active {
    scale: 0.96;
  }
  .wizard-link {
    margin-top: 6px;
    background: transparent;
    border: none;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    color: var(--color-accent-text);
    cursor: pointer;
    animation-delay: 160ms;
  }
  .wizard-link:hover {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .empty-noprovider {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    animation-delay: 120ms;
  }
  .noprovider-line {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }
  @keyframes empty-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  /* === Chat markdown overrides (renderSkillContent .skill-* output) ======
   * Tighten spacing + typography for transcript density and re-skin to
   * PHOSPHOR. All color via tokens; accent-as-text uses --color-accent-text. */
  .chat-md :global(.skill-line) {
    padding: 0;
  }
  .chat-md :global(.skill-paragraph) {
    margin: 2px 0;
  }
  .chat-md :global(.skill-list-item) {
    margin: 1px 0;
    align-items: baseline;
  }
  .chat-md :global(.skill-list) {
    margin: 3px 0;
  }
  .chat-md :global(.skill-list-item .skill-paragraph) {
    margin: 0;
  }
  .chat-md :global(.skill-heading) {
    margin: 12px 0 4px;
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .chat-md :global(.skill-heading-1) {
    font-size: var(--text-lg);
    font-family: var(--font-display);
  }
  .chat-md :global(.skill-heading-2) {
    font-size: var(--text-base);
    font-family: var(--font-display);
  }
  /* Section headers (### Local / Marketplace / Web): uppercase, tracked,
   * accent-TEXT (AA), hairline underline so scanning a long answer lands. */
  .chat-md :global(.skill-heading-3) {
    font-size: var(--text-xs);
    font-family: var(--font-display);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: 600;
    color: var(--color-accent-text);
    margin: 12px 0 4px;
    padding-bottom: 3px;
    border-bottom: 1px solid var(--color-border);
  }
  .chat-md :global(.skill-bullet-symbol),
  .chat-md :global(.skill-ordered-index) {
    color: var(--color-accent-text);
    font-weight: 600;
    flex-shrink: 0;
  }
  .chat-md :global(.skill-quote) {
    border-left: 2px solid var(--color-border-hover);
    background: var(--color-surface-1);
    padding: 4px 8px 4px 10px;
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    margin: 4px 0;
  }
  .chat-md :global(.skill-rule) {
    border-top-color: var(--color-border);
    margin: 6px 0;
  }
  .chat-md :global(.skill-strong) {
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .chat-md :global(.skill-em) {
    font-style: italic;
    color: var(--color-text-secondary);
  }
  .chat-md :global(.skill-md-link) {
    color: var(--color-accent-text);
    text-decoration: none;
    border-bottom: 1px dashed transparent;
    transition: border-color var(--motion-fast) var(--ease-standard);
  }
  .chat-md :global(.skill-md-link:hover) {
    border-bottom-color: var(--color-accent-text);
  }
  .chat-md :global(.skill-inline-code) {
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    background: var(--color-surface-2);
    color: var(--color-text-primary);
    padding: 1px 5px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--color-border);
  }
  .chat-md :global(.skill-code-block) {
    margin: 8px 0;
    padding: 9px 11px;
    border-radius: var(--radius-md);
    background: var(--color-code-bg);
    border: 1px solid var(--color-code-border);
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    line-height: 1.5;
    overflow-x: auto;
    max-width: 100%;
  }
  .chat-md :global(.skill-code-block code) {
    background: transparent;
    border: none;
    padding: 0;
    font-family: inherit;
    font-size: inherit;
    color: var(--color-text-primary);
  }

  /* === Chat-only decorations from renderChatMarkdown.decorate() ========= */
  .chat-md :global(.chat-pill) {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    margin: 0 2px;
    font-size: var(--text-2xs);
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    border-radius: var(--radius-xs);
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    vertical-align: 1px;
  }
  .chat-md :global(.chat-pill-score) {
    border-color: color-mix(in srgb, var(--color-accent) 45%, transparent);
    background: var(--color-accent-subtle);
    color: var(--color-accent-text);
    font-variant-numeric: tabular-nums;
  }
  .chat-md :global(.chat-pill-muted) {
    opacity: 0.85;
  }
  .chat-md :global(.chat-artifact-badge) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    margin: 0 3px;
    cursor: help;
    user-select: none;
    vertical-align: 0;
  }
  .chat-md :global(.chat-artifact-badge:hover) {
    background: var(--color-surface-3);
    color: var(--color-accent-text);
    border-color: var(--color-accent);
  }

  /* Install / scaffold command block: mono pill + inline copy button. */
  .chat-md :global(.chat-cmd) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px 2px 8px;
    margin: 1px 2px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    vertical-align: -2px;
    max-width: 100%;
  }
  .chat-md :global(.chat-cmd-code) {
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    color: var(--color-text-primary);
    background: transparent;
    border: 0;
    padding: 0;
    overflow-wrap: anywhere;
  }
  .chat-md :global(.chat-cmd-copy) {
    appearance: none;
    border: 1px solid var(--color-border);
    background: var(--color-surface-1);
    color: var(--color-text-muted);
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition-property: background-color, color, border-color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .chat-md :global(.chat-cmd-copy:hover),
  .chat-md :global(.chat-cmd-copy.copied) {
    background: var(--color-accent);
    color: var(--color-on-accent);
    border-color: var(--color-accent);
  }

  /* Source-section tags — a categorical taxonomy (Local / Marketplace / Web)
   * kept deliberately OFF the status palette so it can't be read as
   * ok/warn/error. Marketplace = accent (the installable/actionable source);
   * Local + Web differ by ink shade only. All tokened. */
  .chat-md :global(.chat-source-tag) {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    margin-right: 6px;
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    border-radius: var(--radius-xs);
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
  }
  .chat-md :global(.chat-source-local) {
    color: var(--color-text-secondary);
  }
  .chat-md :global(.chat-source-marketplace) {
    color: var(--color-accent-text);
    background: var(--color-accent-subtle);
    border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
  }
  .chat-md :global(.chat-source-web) {
    color: var(--color-text-muted);
  }
  .chat-md :global(.chat-source-mini) {
    display: inline-flex;
    align-items: center;
    height: 14px;
    padding: 0 5px;
    margin: 0 2px;
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: lowercase;
    border-radius: var(--radius-xs);
    vertical-align: 1px;
    font-family: var(--font-ui);
  }

  /* === Final envelope (suggested next prompts + sources) =============== */
  .envelope {
    align-self: stretch;
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 6px;
    padding: 11px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-1);
  }
  .envelope-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .envelope-label {
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
    font-weight: 600;
  }
  .envelope-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .suggest-chip {
    background: var(--color-surface-2);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: 1.2;
    cursor: pointer;
    transition-property: background-color, border-color, color, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .suggest-chip:hover {
    background: var(--color-surface-3);
    border-color: var(--color-border-active);
    color: var(--color-text-primary);
  }
  .suggest-chip:active {
    scale: 0.96;
  }
  .envelope-sources {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .source-link {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
    text-decoration: none;
    font-size: var(--text-xs);
    overflow: hidden;
    transition: border-color var(--motion-fast) var(--ease-standard);
  }
  .source-link:hover {
    border-color: var(--color-border-active);
    color: var(--color-text-primary);
  }
  .source-kind {
    background: var(--color-surface-3);
    color: var(--color-text-muted);
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
    flex-shrink: 0;
  }
  .source-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .source-more {
    font-size: var(--text-2xs);
    color: var(--color-text-muted);
    padding: 0 4px;
  }

  /* === System annotation pills (summary / compaction) ================== */
  .sys-pill {
    align-self: flex-start;
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    padding: 4px 9px;
    border-radius: var(--radius-sm);
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    max-width: 100%;
  }
  .sys-caps {
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-weight: 600;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }
  .sys-text {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .tnum {
    font-variant-numeric: tabular-nums;
  }

  /* === Error pill: friendly copy + Retry + collapsed raw ================ */
  .error-pill {
    align-self: stretch;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-top: 6px;
    padding: 9px 11px;
    border-radius: var(--radius-md);
    background: var(--color-error-subtle);
    border: 1px solid color-mix(in srgb, var(--color-error) 45%, transparent);
    color: var(--color-error);
  }
  .error-icon {
    flex-shrink: 0;
    margin-top: 1px;
    color: var(--color-error);
  }
  .error-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .error-msg {
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    color: var(--color-text-primary);
    word-break: break-word;
  }
  .error-details {
    align-self: flex-start;
    background: transparent;
    border: none;
    padding: 0;
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-muted);
    cursor: pointer;
  }
  .error-details:hover {
    color: var(--color-text-secondary);
  }
  .error-raw {
    margin: 2px 0 0;
    padding: 7px 9px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    line-height: 1.5;
    color: var(--color-text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 140px;
    overflow-y: auto;
  }
  .error-retry {
    flex-shrink: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    .empty > * {
      animation: none;
    }
  }
</style>
