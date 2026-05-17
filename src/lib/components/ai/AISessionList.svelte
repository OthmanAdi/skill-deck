<!--
  @agent-context: List of persisted AI chat sessions. Selecting one loads it
  into the chat view; trash icon deletes.
-->
<script lang="ts">
  import {
    aiStore,
    deleteSession,
    loadSession,
    startNewSession,
  } from "$lib/stores/ai.svelte";

  function formatDate(unix: number) {
    if (!unix) return "";
    return new Date(unix * 1000).toLocaleString();
  }

  async function open(sessionId: string) {
    await loadSession(sessionId);
    aiStore.view = "chat";
  }

  async function remove(sessionId: string, e: MouseEvent) {
    e.stopPropagation();
    await deleteSession(sessionId);
  }

  function newChat() {
    startNewSession();
    aiStore.view = "chat";
  }
</script>

<div class="session-list">
  <div class="header">
    <button class="ghost-btn primary" onclick={newChat}>New chat</button>
    <span class="count">{aiStore.sessions.length} saved</span>
  </div>

  {#if aiStore.sessions.length === 0}
    <div class="empty">No chats yet. Start one in the Chat tab.</div>
  {/if}

  {#each aiStore.sessions as s (s.id)}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="session-row {aiStore.activeSession?.id === s.id ? 'active' : ''}"
      role="button"
      tabindex="0"
      onclick={() => open(s.id)}
      onkeydown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          void open(s.id);
        }
      }}
    >
      <div class="title">{s.title || "Untitled chat"}</div>
      <div class="meta">
        <span>{s.providerId} · {s.model}</span>
        <span>{formatDate(s.updatedAt)}</span>
      </div>
      <button
        class="trash"
        aria-label="Delete chat"
        onclick={(e) => remove(s.id, e)}
      >×</button>
    </div>
  {/each}
</div>

<style>
  .session-list {
    padding: 12px;
    font-size: 12px;
    color: var(--color-text-primary);
  }
  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .count {
    font-size: 10.5px;
    color: var(--color-text-muted);
  }
  .empty {
    padding: 24px 0;
    text-align: center;
    color: var(--color-text-muted);
    font-size: 11.5px;
  }
  .session-row {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 30px 8px 10px;
    border: 1px solid var(--color-border);
    border-radius: 10px;
    background: var(--color-surface-1);
    text-align: left;
    cursor: pointer;
    margin-bottom: 6px;
    transition: background-color 150ms ease;
    width: 100%;
  }
  .session-row:hover {
    background: var(--color-surface-2);
  }
  .session-row.active {
    border-color: var(--color-accent);
  }
  .title {
    font-size: 12px;
    font-weight: 600;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--color-text-muted);
  }
  .trash {
    position: absolute;
    right: 6px;
    top: 6px;
    height: 22px;
    width: 22px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 14px;
  }
  .trash:hover {
    background: var(--color-surface-3);
    color: var(--color-danger, #d97373);
  }
  .ghost-btn {
    height: 26px;
    padding: 0 12px;
    border-radius: 999px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-text-primary);
    font-size: 11px;
    cursor: pointer;
  }
  .ghost-btn.primary {
    background: var(--color-accent);
    color: var(--color-surface-0);
    border-color: transparent;
  }
</style>
