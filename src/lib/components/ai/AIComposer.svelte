<!--
  @agent-context: Multiline input for the AI panel.
  Enter sends, Shift+Enter newline, Esc closes panel.
-->
<script lang="ts">
  import { aiStore, cancelCurrentTurn, sendMessage } from "$lib/stores/ai.svelte";

  let value = $state("");
  let textarea: HTMLTextAreaElement | undefined = $state();

  function autosize() {
    if (!textarea) return;
    textarea.style.height = "auto";
    const max = 160;
    textarea.style.height = `${Math.min(max, textarea.scrollHeight)}px`;
  }

  $effect(() => {
    value;
    autosize();
  });

  async function submit() {
    const text = value.trim();
    if (!text || aiStore.isStreaming) return;
    value = "";
    autosize();
    await sendMessage(text);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  }

  const canSend = $derived(value.trim().length > 0 && !aiStore.isStreaming);
  const placeholder = $derived(
    !aiStore.activeProvider
      ? "Choose a provider in Settings →"
      : aiStore.isStreaming
        ? "Agent is working…"
        : "Ask about your skills, brainstorm, or build a prompt"
  );
</script>

<div
  class="composer"
  style="border-color: var(--color-border); background: var(--color-surface-1);"
>
  <textarea
    bind:this={textarea}
    bind:value
    onkeydown={onKeydown}
    rows="1"
    placeholder={placeholder}
    disabled={!aiStore.activeProvider}
  ></textarea>
  {#if aiStore.isStreaming}
    <button
      type="button"
      class="instant-tooltip send-btn"
      style="background: var(--color-danger, #d97373); color: var(--color-surface-0);"
      onclick={cancelCurrentTurn}
      data-tooltip="Stop the agent and keep whatever has streamed so far"
      aria-label="Stop the agent"
    >
      Stop
    </button>
  {:else}
    <button
      onclick={submit}
      disabled={!canSend}
      class="instant-tooltip send-btn"
      style="background: {canSend ? 'var(--color-accent)' : 'var(--color-surface-3)'}; color: {canSend ? 'var(--color-surface-0)' : 'var(--color-text-muted)'};"
      data-tooltip="Send the message to the agent (Enter)"
      aria-label="Send"
    >
      Send
    </button>
  {/if}
</div>
{#if aiStore.lastError}
  <div class="composer-error">{aiStore.lastError}</div>
{/if}

<style>
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 8px 10px;
    border: 1px solid;
    border-radius: 12px;
  }
  .composer textarea {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--color-text-primary);
    resize: none;
    font-size: 12px;
    line-height: 1.5;
    outline: none;
    min-height: 22px;
    max-height: 160px;
    overflow-y: auto;
    font-family: inherit;
  }
  .composer textarea::placeholder {
    color: var(--color-text-muted);
  }
  .send-btn {
    height: 26px;
    min-width: 52px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    border: none;
    transition: background-color 150ms ease;
    cursor: pointer;
  }
  .send-btn:disabled {
    cursor: not-allowed;
  }
  .composer-error {
    margin-top: 6px;
    font-size: 10.5px;
    color: var(--color-danger, #d97373);
  }
</style>
