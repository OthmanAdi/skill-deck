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

  let contentEl: HTMLDivElement | undefined = $state();

  // After the markdown HTML lands, decorate any code block tagged `prompt`
  // (the agent's suggested-next-prompts or quick-invocation snippets) with
  // a click-to-copy button so the user can grab them without a manual
  // select. We re-run on every html change because the assistant message
  // may stream and grow.
  $effect(() => {
    html; // dependency
    if (!contentEl) return;
    const blocks = contentEl.querySelectorAll<HTMLPreElement>(
      'pre.skill-code-block[data-language="prompt"]'
    );
    blocks.forEach((block) => {
      if (block.dataset.copyAttached === "1") return;
      block.dataset.copyAttached = "1";
      block.classList.add("ai-prompt-block");

      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "ai-prompt-copy";
      btn.textContent = "copy";
      btn.setAttribute("aria-label", "Copy this prompt to the clipboard");
      btn.onclick = (e) => {
        e.preventDefault();
        e.stopPropagation();
        const text = block.innerText.trim();
        void navigator.clipboard.writeText(text);
        const original = btn.textContent;
        btn.textContent = "copied";
        btn.classList.add("ai-prompt-copy-flash");
        window.setTimeout(() => {
          btn.textContent = original ?? "copy";
          btn.classList.remove("ai-prompt-copy-flash");
        }, 1200);
      };
      block.appendChild(btn);
    });
  });
</script>

<div class="ai-message {bubbleClass}">
  {#if message.role === "user"}
    <span class="ai-role-tag">you</span>
  {:else if message.role === "assistant"}
    <span class="ai-role-tag">agent</span>
  {/if}
  <div class="ai-content" bind:this={contentEl}>{@html html}</div>
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
  .ai-content :global(pre.ai-prompt-block) {
    position: relative;
    padding-right: 56px;
    border: 1px solid var(--color-accent-muted, var(--color-border));
  }
  .ai-content :global(.ai-prompt-copy) {
    position: absolute;
    top: 6px;
    right: 6px;
    height: 22px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-1);
    color: var(--color-text-primary);
    font-size: 10px;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .ai-content :global(.ai-prompt-copy:hover) {
    background: var(--color-accent);
    color: var(--color-surface-0);
    border-color: transparent;
  }
  .ai-content :global(.ai-prompt-copy-flash) {
    background: var(--color-accent) !important;
    color: var(--color-surface-0) !important;
    border-color: transparent !important;
  }
</style>
