<!--
  @agent-context: Renders one message bubble.
  - user/assistant: markdown via renderSkillContent (no truncation here).
  - tool: collapsed JSON payload — these only show up in raw mode; the
    AIPanel hides tool messages and shows the AIToolCard inline with the
    parent assistant turn instead.
-->
<script lang="ts">
  import { renderSkillContent } from "$lib/utils/renderSkillContent";
  import type { AiSessionMessage } from "$lib/types/ai";

  let { message }: { message: AiSessionMessage } = $props();

  const html = $derived(
    renderSkillContent(message.content, { maxLines: 9999 }).html
  );

  const bubbleClass = $derived(
    message.role === "user"
      ? "ai-bubble-user"
      : message.role === "assistant"
        ? "ai-bubble-assistant"
        : "ai-bubble-tool"
  );
</script>

<div class="ai-message {bubbleClass}">
  {#if message.role === "user"}
    <span class="ai-role-tag">you</span>
  {:else if message.role === "assistant"}
    <span class="ai-role-tag">agent</span>
  {/if}
  <div class="ai-content">{@html html}</div>
</div>

<style>
  .ai-message {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-1);
    line-height: 1.5;
    font-size: 12px;
    color: var(--color-text-primary);
  }
  .ai-bubble-user {
    background: var(--color-accent-subtle, var(--color-surface-2));
    border-color: var(--color-border-active, var(--color-border));
  }
  .ai-bubble-assistant {
    background: var(--color-surface-1);
  }
  .ai-bubble-tool {
    background: var(--color-surface-2);
    font-size: 10.5px;
  }
  .ai-role-tag {
    align-self: flex-start;
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--color-text-muted);
  }
  .ai-content :global(p) {
    margin: 0 0 6px 0;
  }
  .ai-content :global(p:last-child) {
    margin-bottom: 0;
  }
  .ai-content :global(pre) {
    margin: 6px 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--color-surface-2);
    overflow: auto;
    font-size: 11px;
  }
  .ai-content :global(code) {
    font-family: ui-monospace, SFMono-Regular, "JetBrains Mono", Consolas, monospace;
  }
  .ai-content :global(ul),
  .ai-content :global(ol) {
    margin: 4px 0 6px 18px;
    padding: 0;
  }
  .ai-content :global(li) {
    margin: 2px 0;
  }
  .ai-content :global(h1),
  .ai-content :global(h2),
  .ai-content :global(h3) {
    margin: 8px 0 4px 0;
    font-weight: 600;
    color: var(--color-text-primary);
  }
  .ai-content :global(h1) { font-size: 14px; }
  .ai-content :global(h2) { font-size: 13px; }
  .ai-content :global(h3) { font-size: 12.5px; }
</style>
