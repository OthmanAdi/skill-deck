<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { renderSkillContent } from "$lib/utils/renderSkillContent";
  import AgentBadge from "./AgentBadge.svelte";
  import {
    closeFullSkillModal,
    copySkillReference,
    showToast,
    store,
  } from "$lib/stores/skills.svelte";

  let closeButtonEl: HTMLButtonElement | undefined = $state();
  let panelEl: HTMLDivElement | undefined = $state();
  let previousFocusEl: HTMLElement | null = null;

  const skill = $derived(store.fullSkillModalSkill);
  const content = $derived(store.fullSkillModalContent);

  const rendered = $derived.by(() => {
    if (!content) return null;
    return renderSkillContent(content, {
      maxLines: Number.MAX_SAFE_INTEGER,
      filePath: skill?.filePath ?? null,
    });
  });

  $effect(() => {
    if (store.fullSkillModalOpen) {
      previousFocusEl = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      requestAnimationFrame(() => closeButtonEl?.focus());
      return;
    }

    if (previousFocusEl) {
      previousFocusEl.focus();
      previousFocusEl = null;
    }
  });

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      closeFullSkillModal();
    }
  }

  function handleBackdropKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeFullSkillModal();
    }
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (!store.fullSkillModalOpen) return;

    if (e.key === "Tab") {
      const focusable = panelEl
        ? Array.from(
            panelEl.querySelectorAll<HTMLElement>(
              'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
            )
          ).filter((el) => !el.hasAttribute("disabled") && el.offsetParent !== null)
        : [];

      if (focusable.length > 0) {
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        const active = document.activeElement as HTMLElement | null;
        const activeInside = !!active && !!panelEl?.contains(active);

        if (!activeInside) {
          e.preventDefault();
          first.focus();
          return;
        }

        if (e.shiftKey && active === first) {
          e.preventDefault();
          last.focus();
          return;
        }

        if (!e.shiftKey && active === last) {
          e.preventDefault();
          first.focus();
          return;
        }
      }
    }

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeFullSkillModal();
    }
  }

  async function handleCopyFullContent() {
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      showToast("Copied full skill content");
    } catch {
      showToast("Could not copy full content");
    }
  }

  async function handleCopyReference() {
    if (!skill) return;
    await copySkillReference(skill);
  }

  const lineCountLabel = $derived.by(() => {
    const count = rendered?.totalLineCount ?? 0;
    return `${count} line${count === 1 ? "" : "s"}`;
  });
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if store.fullSkillModalOpen}
  <div
    class="fixed inset-0 z-[220] flex items-center justify-center p-4"
    style="background: rgba(4, 7, 12, 0.58); backdrop-filter: blur(2px);"
    transition:fade={{ duration: 140 }}
    onclick={handleBackdropClick}
    onkeydown={handleBackdropKeydown}
    role="dialog"
    aria-modal="true"
    aria-label={skill ? `Full skill view: ${skill.name}` : "Full skill view"}
    tabindex="-1"
  >
    <div
      bind:this={panelEl}
      class="full-skill-modal-panel flex w-full max-w-[980px] flex-col overflow-hidden"
      style="max-height: min(88vh, 900px);"
      transition:fly={{ y: 10, duration: 180, easing: cubicOut }}
      role="document"
      tabindex="-1"
    >
      <header class="flex shrink-0 items-start justify-between gap-3 border-b px-4 py-3"
        style="border-color: var(--color-border); background: var(--color-surface-1);"
      >
        <div class="min-w-0 flex-1 space-y-1">
          <div class="flex flex-wrap items-center gap-2">
            {#if skill}
              <AgentBadge agentId={skill.agentId} />
              <span
                class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium"
                style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-muted);"
              >
                {skill.artifactType}
              </span>
            {/if}
            <span class="text-[10px] tabular-nums text-[var(--color-text-muted)]">{lineCountLabel}</span>
          </div>

          <h2 class="truncate text-[15px] font-semibold text-[var(--color-text-primary)]">
            {skill?.name ?? "Skill"}
          </h2>

          {#if skill?.description}
            <p class="line-clamp-2 text-[11px] text-[var(--color-text-secondary)]">{skill.description}</p>
          {/if}

          {#if skill?.filePath}
            <p class="truncate font-mono text-[9px] text-[var(--color-text-muted)]">{skill.filePath}</p>
          {/if}
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          {#if skill}
            <button
              class="rounded-md border px-2 py-1 text-[10px] font-medium transition-colors"
              style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
              onclick={handleCopyReference}
              title="Copy reference"
            >
              Copy ref
            </button>
          {/if}

          <button
            class="rounded-md border px-2 py-1 text-[10px] font-medium transition-colors"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
            onclick={handleCopyFullContent}
            disabled={!content}
            title="Copy full content"
          >
            Copy full
          </button>

          <button
            bind:this={closeButtonEl}
            class="rounded-md border px-2 py-1 text-[10px] font-medium transition-colors"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
            onclick={closeFullSkillModal}
            aria-label="Close full skill modal"
          >
            Close
          </button>
        </div>
      </header>

      {#if skill}
        <div class="shrink-0 border-b px-4 py-1.5 text-[9px]"
          style="border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-muted);"
        >
          Press <span class="text-[var(--color-text-secondary)]">Esc</span> to close, use the header actions to copy reference or full content
        </div>
      {/if}

      <div
        class="min-h-0 flex-1 overflow-y-auto px-4 py-3"
        style="background: var(--color-code-bg);"
      >
        {#if store.fullSkillModalLoading}
          <div class="flex items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
            <span class="spin inline-block h-3 w-3 rounded-full border border-[var(--color-accent)] border-t-transparent"></span>
            Loading full skill content...
          </div>
        {:else if store.fullSkillModalError}
          <p class="text-[11px] text-[var(--color-error)]">{store.fullSkillModalError}</p>
        {:else if rendered}
          <div class="skill-content-preview full-skill-content">
            {@html rendered.html}
          </div>
        {:else}
          <p class="text-[11px] text-[var(--color-text-muted)]">No content available</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
