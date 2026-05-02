<!--
  @agent-context: Individual skill card with glassmorphism and content preview.
  Shows: icon/emoji, name, description, agent badge, star button, copy button.
  Expanded view fetches and displays the actual skill file content.
  Supports keyboard focus via isFocused prop from parent Overlay.
-->
<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import type { Skill } from "$lib/types";
  import {
    toggleStar,
    copySkillReference,
    startDragPoll,
    stopDragPoll,
    injectSkillToTerminal,
    checkSkillUpdate,
    setSkillRepo,
    store,
  } from "$lib/stores/skills.svelte";
  import { renderSkillContent } from "$lib/utils/renderSkillContent";
  import AgentBadge from "./AgentBadge.svelte";

  let { skill, index = 0, isFocused = false }: { skill: Skill; index?: number; isFocused?: boolean } = $props();

  let isExpanded = $state(false);
  let isDragging = $state(false);
  let starAnimating = $state(false);
  let fileContent = $state<string | null>(null);
  let contentLoading = $state(false);

  function handleStarClick(e: MouseEvent) {
    e.stopPropagation();
    starAnimating = true;
    toggleStar(skill.id);
    setTimeout(() => { starAnimating = false; }, 350);
  }

  function handleCopyClick(e: MouseEvent) {
    e.stopPropagation();
    copySkillReference(skill);
  }

  async function handleCardClick() {
    isExpanded = !isExpanded;
    if (isExpanded && fileContent === null) {
      contentLoading = true;
      try {
        const raw: string = await invoke("read_skill_content", {
          skillId: skill.id,
          projectPath: skill.projectPath,
        });
        fileContent = raw;
      } catch {
        fileContent = "// Could not read file";
      } finally {
        contentLoading = false;
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleCardClick();
    } else if (e.key === "c" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      copySkillReference(skill);
    }
  }

  // ── Drag handlers ──
  function handleDragStart(e: DragEvent) {
    isDragging = true;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "copy";
      e.dataTransfer.setData("text/plain", skill.filePath);
      e.dataTransfer.setData("text/uri-list", `file://${skill.filePath}`);
      e.dataTransfer.setData("application/x-skill-deck", JSON.stringify({
        id: skill.id,
        name: skill.name,
        filePath: skill.filePath,
        agentId: skill.agentId,
      }));
    }
    startDragPoll();
  }

  async function handleDragEnd(e: DragEvent) {
    isDragging = false;
    const wasOverTerminal = store.dragOverTerminal;
    const terminalPid = store.dragTerminalPid;
    stopDragPoll();

    if (wasOverTerminal) {
      // Try to inject into the terminal — clipboard+paste approach
      const injected = await injectSkillToTerminal(skill, terminalPid ?? undefined);
      if (!injected) {
        copySkillReference(skill);
      }
    } else {
      // Fallback: copy to clipboard so the user can paste manually
      copySkillReference(skill);
    }
  }

  // ── Repo UI state ──
  let repoInputVisible = $state(false);
  let repoInputValue = $state("");
  let repoCheckLoading = $state(false);

  async function handleCheckUpdate() {
    repoCheckLoading = true;
    await checkSkillUpdate(skill);
    repoCheckLoading = false;
  }

  async function handleSaveRepo() {
    if (!repoInputValue.trim()) return;
    await setSkillRepo(skill.id, repoInputValue.trim());
    repoInputVisible = false;
    repoInputValue = "";
  }

  const displayIcon = $derived(
    skill.icon ?? skill.name.charAt(0).toUpperCase()
  );

  const isEmoji = $derived(
    skill.icon ? /\p{Emoji}/u.test(skill.icon) : false
  );

  const contentPreview = $derived.by(() => {
    if (!fileContent) return null;
    return renderSkillContent(fileContent, 240);
  });

  const entryDelayMs = $derived(Math.min(index, 8) * 16);
</script>

<div
  class="card-enter group relative flex w-full gap-3 rounded-xl p-3 text-left cursor-pointer
    transition-[background,border-color,box-shadow,transform] duration-[180ms] ease-[var(--motion-ease-standard)]
    border
    active:scale-[0.985]
    {isDragging && !store.dragOverTerminal ? 'opacity-40 scale-[0.96]' : ''}
    {isDragging && store.dragOverTerminal ? 'opacity-90 scale-[0.97] border-[var(--color-accent-muted)]' : ''}
    {isExpanded
      ? 'border-[var(--color-border-active)] bg-[var(--color-surface-2)]'
      : 'border-transparent bg-[var(--color-surface-1)] hover:border-[var(--color-border-hover)] hover:bg-[var(--color-surface-2)]'}
    {isFocused ? 'card-focused border-[var(--color-accent-muted)]' : ''}"
  style="animation-delay: {entryDelayMs}ms;
    {isExpanded ? 'box-shadow: 0 4px 20px -4px var(--color-overlay-shadow), inset 0 1px 0 0 rgba(247, 248, 248, 0.05);' : ''}"
  data-index={index}
  draggable="true"
  onclick={handleCardClick}
  onkeydown={handleKeydown}
  ondragstart={handleDragStart}
  ondragend={(e) => handleDragEnd(e)}
  role="option"
  aria-selected={isFocused}
  tabindex={isFocused ? 0 : -1}
>
  <!-- Icon / Emoji / Letter avatar -->
  <div
    class="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-sm font-semibold
      transition-all duration-200 group-hover:scale-105
      {isEmoji ? '' : 'text-[var(--color-text-secondary)]'}"
    style="{isEmoji
      ? ''
      : `background: var(--color-surface-3); border: 1px solid var(--color-border);`}"
  >
    {displayIcon}
  </div>

  <!-- Content -->
  <div class="min-w-0 flex-1">
    <!-- Header row: name + actions -->
    <div class="flex items-start justify-between gap-1.5">
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-[13px] font-medium leading-snug text-[var(--color-text-primary)]">
          {skill.name}
        </h3>
        <p class="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-[var(--color-text-secondary)]">
          {skill.description || "No description"}
        </p>
      </div>

      <!-- Action buttons -->
      <div class="flex shrink-0 items-center gap-0.5">
        <!-- Copy button -->
        <button
          class="rounded-md p-1 text-[var(--color-text-muted)]
            opacity-0 transition-all duration-150
            group-hover:opacity-70
            hover:!opacity-100 hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
          onclick={handleCopyClick}
          title="Copy skill reference (Ctrl+C)"
          aria-label="Copy skill reference"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round"
              d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
          </svg>
        </button>

        <!-- Star button -->
        <button
          class="shrink-0 rounded-md p-1 text-sm transition-all duration-150
            {skill.starred
              ? 'text-[var(--color-starred)]'
              : 'text-[var(--color-text-muted)] opacity-0 group-hover:opacity-70 hover:!opacity-100 hover:text-[var(--color-starred)]'}
            {starAnimating ? 'star-pop' : ''}"
          onclick={handleStarClick}
          aria-label={skill.starred ? "Unstar skill" : "Star skill"}
        >
          {skill.starred ? "\u2605" : "\u2606"}
        </button>
      </div>
    </div>

    <!-- Metadata row -->
    <div class="mt-1.5 flex flex-wrap items-center gap-1.5">
      <AgentBadge agentId={skill.agentId} />

      {#if skill.scope === "project"}
        <span class="rounded-md px-1.5 py-0.5 text-[9px] font-medium tracking-wide uppercase
          border border-[var(--color-border)]
          text-[var(--color-text-muted)]"
          style="background: var(--color-surface-2);">
          project
        </span>
      {/if}

      {#if skill.metadata.trigger}
        <span class="rounded-md px-1.5 py-0.5 text-[9px] font-medium tracking-wide
          text-[var(--color-text-muted)]"
          style="background: var(--color-surface-2); border: 1px solid var(--color-border);">
          {skill.metadata.trigger}
        </span>
      {/if}

      {#if skill.metadata.version}
        <span class="text-[9px] tabular-nums text-[var(--color-text-muted)] opacity-60">
          v{skill.metadata.version}
        </span>
      {/if}

      {#if skill.updateAvailable}
        <span class="h-1.5 w-1.5 rounded-full bg-[var(--color-update-available)]"
          style="animation: breathe 2s ease-in-out infinite;"
          title="Update available"></span>
      {/if}
    </div>

    <!-- Expanded: file content preview -->
    {#if isExpanded}
      <div class="mt-3 space-y-2 border-t border-[var(--color-border)] pt-3"
        transition:slide={{ duration: 200, easing: cubicOut }}
      >
        <!-- Quick metadata -->
        <div class="flex flex-wrap gap-x-4 gap-y-1 text-[10px]">
          {#if skill.metadata.allowedTools}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Tools</span>
              <span class="text-[var(--color-text-muted)]">{skill.metadata.allowedTools}</span>
            </div>
          {/if}
          {#if skill.metadata.globs}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Globs</span>
              <span class="text-[var(--color-text-muted)]">{skill.metadata.globs.join(", ")}</span>
            </div>
          {/if}
        </div>

        <!-- File content preview -->
        <div class="rounded-lg overflow-hidden"
          style="background: var(--color-code-bg); border: 1px solid var(--color-code-border);">
          {#if contentLoading}
            <div class="flex items-center gap-2 px-3 py-4 text-[10px] text-[var(--color-text-muted)]">
              <div class="h-3 w-3 rounded-full border-[1.5px] border-[var(--color-accent)] border-t-transparent"
                style="animation: refresh-spin 0.7s linear infinite;"></div>
              Reading file...
            </div>
          {:else if contentPreview}
            <div class="skill-content-preview px-3 py-2.5 max-h-[200px] overflow-y-auto">
              {@html contentPreview.html}
              {#if contentPreview.truncated}
                <div class="skill-truncate-note">... {contentPreview.hiddenLineCount} more lines</div>
              {/if}
            </div>
          {/if}
        </div>

        <!-- Repo / update section -->
        <div class="rounded-lg px-2.5 py-2 text-[10px]"
          style="background: var(--color-surface-3); border: 1px solid var(--color-border);">

          {#if skill.metadata.repositoryUrl}
            <!-- Has a repo URL -->
            <div class="flex items-center justify-between gap-2">
              <a
                href={skill.metadata.repositoryUrl}
                target="_blank"
                rel="noopener noreferrer"
                class="flex min-w-0 items-center gap-1.5 text-[var(--color-accent)] hover:underline"
                onclick={(e) => e.stopPropagation()}
              >
                <svg class="h-3 w-3 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                </svg>
                <span class="truncate">{skill.metadata.repositoryUrl.replace(/^https?:\/\//, '')}</span>
              </a>
              <button
                class="shrink-0 rounded px-1.5 py-0.5 text-[9px] font-medium transition-all duration-150
                  text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={(e) => { e.stopPropagation(); handleCheckUpdate(); }}
                disabled={repoCheckLoading}
                title="Check for updates"
              >
                {#if repoCheckLoading}
                  <span class="spin inline-block h-2.5 w-2.5 rounded-full border border-[var(--color-accent)] border-t-transparent"></span>
                {:else if skill.updateAvailable}
                  ↑ update
                {:else}
                  check
                {/if}
              </button>
            </div>
          {:else}
            <!-- No repo URL — show input or prompt -->
            {#if repoInputVisible}
              <div class="flex items-center gap-1.5" role="group" aria-label="Set repository URL">
                <input
                  type="text"
                  bind:value={repoInputValue}
                  placeholder="https://github.com/owner/repo"
                  class="flex-1 rounded border bg-[var(--color-surface-1)] px-2 py-1 text-[10px]
                    text-[var(--color-text-primary)] placeholder-[var(--color-text-muted)]
                    focus:outline-none focus:border-[var(--color-accent-muted)]"
                  style="border-color: var(--color-border);"
                  onkeydown={(e) => { if (e.key === 'Enter') handleSaveRepo(); if (e.key === 'Escape') repoInputVisible = false; }}
                />
                <button
                  class="shrink-0 rounded px-2 py-1 text-[9px] font-medium transition-colors
                    bg-[var(--color-accent-subtle)] text-[var(--color-accent)]
                    hover:bg-[var(--color-accent-muted)] hover:text-white"
                  onclick={handleSaveRepo}
                >save</button>
                <button
                  class="shrink-0 rounded px-1.5 py-1 text-[9px] text-[var(--color-text-muted)]
                    hover:text-[var(--color-text-secondary)]"
                  onclick={() => repoInputVisible = false}
                >✕</button>
              </div>
            {:else}
              <button
                class="flex items-center gap-1 text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]
                  transition-colors duration-150"
                onclick={(e) => { e.stopPropagation(); repoInputVisible = true; repoInputValue = ""; }}
              >
                <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
                </svg>
                <span>Add repo URL for update checking</span>
              </button>
            {/if}
          {/if}

          {#if skill.metadata.installCommand}
            <div class="mt-1.5 flex items-center justify-between gap-2 border-t pt-1.5"
              style="border-color: var(--color-border);">
              <span class="truncate font-mono text-[9px] text-[var(--color-text-muted)]">
                {skill.metadata.installCommand}
              </span>
              <button
                class="shrink-0 rounded px-1.5 py-0.5 text-[9px] text-[var(--color-text-muted)]
                  hover:bg-[var(--color-surface-2)] hover:text-[var(--color-accent)] transition-colors"
                onclick={(e) => {
                  e.stopPropagation();
                  navigator.clipboard.writeText(skill.metadata.installCommand!).catch(() => {});
                }}
                title="Copy install command"
              >copy</button>
            </div>
          {/if}
        </div>

        <!-- File path -->
        <div class="flex items-center gap-1.5 text-[9px] text-[var(--color-text-secondary)] opacity-95">
          <svg class="h-2.5 w-2.5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
          </svg>
          <span class="truncate font-mono">{skill.filePath}</span>
        </div>
      </div>
    {/if}
  </div>
</div>
