<!--
  @agent-context: Multiline composer. Enter sends, Shift+Enter newline.
  Send stays guarded while streaming; the textarea itself remains live so
  the user can draft the next message while the agent is mid-turn.
  Send is optimistic (clears immediately so the composer is free for the
  next draft — the sent text is mirrored as a user bubble by the store's
  pending turn). sendMessage() never throws: it returns false on failure and
  keeps the failed turn + Retry in the transcript. On false we RESTORE the
  typed text so nothing is lost (this also covers the no-provider case, which
  has no transcript Retry) — chat-send-failure. Also adopts one-shot
  composerPrefill values (empty-state example prompts) and clears the channel.
-->
<script lang="ts">
  import { tick } from "svelte";
  import { cancelCurrentTurn, harnessStore, openChat, sendMessage } from "$lib/stores/harness.svelte";

  let value = $state("");
  let textareaEl: HTMLTextAreaElement | undefined = $state();

  // One-shot prefill channel: the empty-state example prompts call
  // prefillComposer(), which sets harnessStore.composerPrefill. Adopt it into
  // the textarea, then clear the channel so it fires exactly once.
  $effect(() => {
    const prefill = harnessStore.composerPrefill;
    if (prefill === null || prefill === undefined) return;
    value = prefill;
    harnessStore.composerPrefill = null;
    void tick().then(() => {
      autoresize();
      textareaEl?.focus();
    });
  });

  async function send() {
    if (harnessStore.isStreaming) return;
    const text = value.trim();
    if (!text) return;
    if (opensWizard(text)) {
      value = "";
      autoresize();
      openChat("wizard");
      return;
    }
    // Optimistic clear keeps the composer free for mid-turn drafting.
    value = "";
    autoresize();
    const ok = await sendMessage(text);
    if (!ok) {
      // Restore so a failed send never destroys the draft — the user can edit
      // and resend without retyping.
      value = text;
      await tick();
      autoresize();
      textareaEl?.focus();
    }
  }

  function opensWizard(text: string): boolean {
    const normalized = text.trim().toLowerCase();
    return (
      normalized === "/wizard" ||
      normalized === "/library" ||
      normalized === "/skill-library" ||
      normalized === "/skill library" ||
      normalized === "wizard" ||
      normalized === "skill library" ||
      normalized.includes("skill library wizard")
    );
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (harnessStore.isStreaming) return;
      void send();
    }
  }

  function autoresize() {
    if (!textareaEl) return;
    textareaEl.style.height = "0";
    textareaEl.style.height = `${Math.min(textareaEl.scrollHeight, 220)}px`;
  }
</script>

<div class="composer">
  <div class="textarea-wrap selectable">
    <textarea
      bind:this={textareaEl}
      bind:value
      onkeydown={onKey}
      oninput={autoresize}
      placeholder="Ask the agent, or type /wizard to design a team skill library"
      rows="2"
    ></textarea>
  </div>
  <div class="row">
    <span class="meta">{harnessStore.activeProvider ?? "no provider"} · {harnessStore.activeModel ?? "no model"}</span>
    {#if harnessStore.isStreaming}
      <button class="btn-secondary btn-sm" onclick={() => void cancelCurrentTurn()} title="Stop the agent">
        <span class="btn-glyph" aria-hidden="true">■</span>
        <span>Stop</span>
      </button>
    {:else}
      <button class="btn-primary btn-sm" disabled={!value.trim()} onclick={() => void send()} title="Send (Enter)">
        <span>Send</span>
        <span class="btn-glyph" aria-hidden="true">
          <svg width="13" height="13" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
            <path d="M1.5 8L14.5 1.5L9.5 14.5L8 9L1.5 8Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
          </svg>
        </span>
      </button>
    {/if}
  </div>
</div>

<style>
  /* Flat carbon footer — no gradients (DESIGN 2/6), hairline separates it from
   * the transcript above. */
  .composer {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-1);
  }
  /* surface-2 = control at rest (DESIGN 3.1). radius-md container; the inner
   * textarea radius is concentric (md - padding). Accent focus ring matches
   * the shared .field treatment. */
  .textarea-wrap {
    position: relative;
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    transition-property: border-color, box-shadow;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .textarea-wrap:hover {
    border-color: var(--color-border-hover);
  }
  .textarea-wrap:focus-within {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-subtle);
  }
  textarea {
    width: 100%;
    resize: none;
    background: transparent;
    color: var(--color-text-primary);
    border: none;
    border-radius: var(--radius-sm);
    padding: 8px 10px;
    font-family: var(--font-ui);
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    overflow-y: auto;
    max-height: 220px;
    user-select: text;
    -webkit-user-select: text;
  }
  textarea:focus {
    outline: none;
  }
  textarea::placeholder {
    color: var(--color-text-muted);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 8px;
    gap: 12px;
  }
  .meta {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    font-family: var(--font-ui);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .btn-glyph {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 1;
  }
</style>
