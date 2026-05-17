<!--
  @agent-context: Full-overlay AI chat panel.
  Mounted at the top of Overlay.svelte. Visibility driven by aiStore.open.
-->
<script lang="ts">
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import AIComposer from "./AIComposer.svelte";
  import AIDebug from "./AIDebug.svelte";
  import AIMessage from "./AIMessage.svelte";
  import AISessionList from "./AISessionList.svelte";
  import AISettings from "./AISettings.svelte";
  import AIToolCard from "./AIToolCard.svelte";
  import {
    aiStore,
    closeAiPanel,
    initAiPanel,
    refreshRegistry,
    refreshSessions,
  } from "$lib/stores/ai.svelte";
  import { onMount } from "svelte";

  let scrollEl: HTMLDivElement | undefined = $state();

  onMount(() => {
    void initAiPanel();
  });

  $effect(() => {
    if (aiStore.open && aiStore.view === "sessions") {
      void refreshSessions();
    }
    if (aiStore.open && aiStore.view === "settings") {
      void refreshRegistry();
    }
  });

  $effect(() => {
    if (!scrollEl) return;
    // Track these so the effect re-runs:
    aiStore.activeSession?.messages.length;
    aiStore.pending?.text;
    requestAnimationFrame(() => {
      if (scrollEl) {
        scrollEl.scrollTop = scrollEl.scrollHeight;
      }
    });
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && aiStore.open) {
      e.preventDefault();
      e.stopPropagation();
      closeAiPanel();
    }
  }

  type ChatItem =
    | { kind: "message"; key: string; message: import("$lib/types/ai").AiSessionMessage }
    | {
        kind: "toolCard";
        key: string;
        name: string;
        argumentsJson: string;
        status: "success" | "error";
        label?: string;
        data?: unknown;
        errorMessage?: string;
      };

  function safeParse(text: string): unknown {
    if (!text) return null;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }

  // Walk the persisted session into an ordered render list. Tool messages
  // are folded into the assistant turn that issued them so the user sees
  // every tool card with its full result, then the final assistant reply,
  // exactly in the order the agent produced them.
  function flattenSession(messages: import("$lib/types/ai").AiSessionMessage[]): ChatItem[] {
    const out: ChatItem[] = [];
    const toolByCallId = new Map<string, import("$lib/types/ai").AiSessionMessage>();
    for (const m of messages) {
      if (m.role === "tool" && m.toolCallId) {
        toolByCallId.set(m.toolCallId, m);
      }
    }
    for (const m of messages) {
      if (m.role === "user") {
        out.push({ kind: "message", key: m.id, message: m });
        continue;
      }
      if (m.role === "assistant") {
        if (m.content && m.content.trim().length > 0) {
          out.push({ kind: "message", key: m.id, message: m });
        }
        if (m.toolCalls && m.toolCalls.length > 0) {
          for (const call of m.toolCalls) {
            const toolMsg = toolByCallId.get(call.id);
            const parsed = toolMsg ? safeParse(toolMsg.content) : null;
            const isError =
              !!toolMsg?.toolError ||
              (parsed !== null &&
                typeof parsed === "object" &&
                parsed !== null &&
                "error" in (parsed as Record<string, unknown>));
            out.push({
              kind: "toolCard",
              key: `${m.id}:${call.id}`,
              name: call.name,
              argumentsJson: call.argumentsJson,
              status: isError ? "error" : "success",
              label: toolMsg?.toolLabel ?? undefined,
              data: parsed,
              errorMessage: toolMsg?.toolError ?? undefined,
            });
          }
        }
        continue;
      }
      // role === "tool" already consumed via lookup map.
    }
    return out;
  }

  const chatItems = $derived(flattenSession(aiStore.activeSession?.messages ?? []));

  const providerSummary = $derived(
    aiStore.activeProvider
      ? `${aiStore.providers.find((p) => p.id === aiStore.activeProvider)?.label ?? "?"} · ${aiStore.activeModel ?? "(no model)"}`
      : "Not configured",
  );
</script>

<svelte:window onkeydown={onKeydown} />

{#if aiStore.open}
  <div
    class="ai-overlay"
    transition:fly={{ y: 8, duration: 180, easing: cubicOut }}
    role="dialog"
    aria-modal="true"
    aria-label="AI chat"
  >
    <header class="ai-header">
      <div class="left">
        <span class="badge" style="background: var(--color-accent);">AI</span>
        <div class="title-block">
          <h2>Skill Deck AI</h2>
          <p>{providerSummary}</p>
        </div>
      </div>
      <div class="tabs">
        <button
          class="tab {aiStore.view === 'chat' ? 'active' : ''}"
          onclick={() => (aiStore.view = "chat")}
        >Chat</button>
        <button
          class="tab {aiStore.view === 'sessions' ? 'active' : ''}"
          onclick={() => (aiStore.view = "sessions")}
        >History</button>
        <button
          class="tab {aiStore.view === 'debug' ? 'active' : ''}"
          onclick={() => (aiStore.view = "debug")}
        >Debug</button>
        <button
          class="tab {aiStore.view === 'settings' ? 'active' : ''}"
          onclick={() => (aiStore.view = "settings")}
        >Settings</button>
      </div>
      <button class="close-btn" onclick={closeAiPanel} aria-label="Close">×</button>
    </header>

    {#if aiStore.view === "chat"}
      <div class="ai-scroll" bind:this={scrollEl}>
        {#if chatItems.length === 0 && !aiStore.pending}
          <div class="empty">
            <h3>Talk to your skills</h3>
            <p>
              Ask what skills you have, brainstorm a workflow, or build a perfect
              prompt for your coding agent — backed by the skills already on
              your machine.
            </p>
            <ul>
              <li>“What skills do I have for testing rust?”</li>
              <li>“Make me a prompt for Claude Code to migrate this API using rust-testing and code-review-quality.”</li>
              <li>“Combine humanizer and pr-perfect into a release-notes workflow.”</li>
            </ul>
          </div>
        {/if}

        {#each chatItems as item (item.key)}
          {#if item.kind === "message"}
            <AIMessage message={item.message} />
          {:else}
            <AIToolCard
              cardKey={item.key}
              name={item.name}
              argumentsJson={item.argumentsJson}
              status={item.status}
              label={item.label}
              data={item.data}
              errorMessage={item.errorMessage}
              initiallyExpanded={true}
            />
          {/if}
        {/each}

        {#if aiStore.pending}
          <div class="pending">
            {#each aiStore.pending.toolCalls as call (call.callId)}
              <AIToolCard
                cardKey={`${aiStore.pending.id}:${call.callId}`}
                name={call.name}
                argumentsJson={call.argumentsJson}
                status={call.status}
                label={call.label}
                data={call.data}
                errorMessage={call.errorMessage}
                initiallyExpanded={true}
              />
            {/each}
            {#if aiStore.pending.text.length > 0}
              <AIMessage
                message={{
                  id: aiStore.pending.id,
                  role: "assistant",
                  content: aiStore.pending.text,
                  createdAt: Math.floor(Date.now() / 1000),
                }}
              />
            {:else if aiStore.isStreaming}
              <div class="thinking">
                <span class="dot"></span><span class="dot"></span><span class="dot"></span>
                <span>thinking…</span>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="ai-composer-wrap">
        <AIComposer />
      </div>
    {:else if aiStore.view === "sessions"}
      <div class="ai-scroll">
        <AISessionList />
      </div>
    {:else if aiStore.view === "debug"}
      <div class="ai-scroll">
        <AIDebug />
      </div>
    {:else}
      <div class="ai-scroll">
        <AISettings />
      </div>
    {/if}
  </div>
{/if}

<style>
  .ai-overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    display: flex;
    flex-direction: column;
    background: var(--color-surface-0);
    border-radius: var(--radius-lg);
    border: 1px solid var(--color-border);
    box-shadow: 0 24px 48px -12px var(--color-overlay-shadow);
    overflow: hidden;
  }
  .ai-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-1);
  }
  .ai-header .left {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: 1;
    min-width: 0;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 22px;
    min-width: 28px;
    border-radius: 6px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: var(--color-surface-0);
  }
  .title-block {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .title-block h2 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .title-block p {
    margin: 0;
    font-size: 10.5px;
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tabs {
    display: flex;
    gap: 4px;
    background: var(--color-surface-2);
    padding: 3px;
    border-radius: 999px;
  }
  .tab {
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    padding: 5px 12px;
    font-size: 10.5px;
    font-weight: 600;
    border-radius: 999px;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .tab:hover { color: var(--color-text-secondary); }
  .tab.active {
    background: var(--color-surface-0);
    color: var(--color-text-primary);
  }
  .close-btn {
    height: 26px;
    width: 26px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
  }
  .close-btn:hover {
    background: var(--color-surface-2);
    color: var(--color-text-primary);
  }
  .ai-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .ai-composer-wrap {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-0);
  }
  .empty {
    margin: auto;
    max-width: 360px;
    padding: 20px;
    text-align: center;
    color: var(--color-text-secondary);
  }
  .empty h3 {
    margin: 0 0 6px 0;
    font-size: 14px;
    color: var(--color-text-primary);
  }
  .empty p {
    margin: 0 0 8px 0;
    font-size: 12px;
    line-height: 1.5;
  }
  .empty ul {
    text-align: left;
    margin: 8px auto 0;
    padding-left: 18px;
    font-size: 11.5px;
    color: var(--color-text-muted);
  }
  .empty li { margin: 4px 0; }
  .pending {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .thinking {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: 12px;
    background: var(--color-surface-1);
    color: var(--color-text-muted);
    font-size: 11px;
    align-self: flex-start;
  }
  .dot {
    height: 4px;
    width: 4px;
    border-radius: 50%;
    background: var(--color-text-muted);
    animation: dot-pulse 1.1s ease-in-out infinite;
  }
  .dot:nth-child(1) { animation-delay: 0ms; }
  .dot:nth-child(2) { animation-delay: 150ms; }
  .dot:nth-child(3) { animation-delay: 300ms; }
  @keyframes dot-pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.8); }
    50% { opacity: 1; transform: scale(1.3); }
  }
</style>
