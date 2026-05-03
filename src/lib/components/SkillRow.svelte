<!--
  @agent-context: Dense 34px skill row for the grouped list view.
  Click to expand and show file content preview with syntax highlighting.
  Shows metadata (trigger, version, update indicator) inline.
  Star and copy buttons appear on hover.
-->
<script lang="ts">
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import type { Skill } from "$lib/types";
  import {
    toggleStar,
    copySkillReference,
    checkSkillUpdate,
    snapshotSkillBeforeUpdate,
    loadSkillVersionHistory,
    restoreSkillVersion,
    store,
  } from "$lib/stores/skills.svelte";
  import { renderSkillContent } from "$lib/utils/renderSkillContent";

  let {
    skill,
    index = 0,
    delayIndex = 0,
    isFocused = false,
  }: { skill: Skill; index?: number; delayIndex?: number; isFocused?: boolean } = $props();

  let isExpanded = $state(false);
  let fileContent = $state<string | null>(null);
  let contentLoading = $state(false);
  let starAnimating = $state(false);
  let repoCheckLoading = $state(false);
  let snapshotLoading = $state(false);
  let historyLoading = $state(false);
  let restoringVersionId = $state<string | null>(null);

  async function handleClick() {
    isExpanded = !isExpanded;
    if (isExpanded && fileContent === null) {
      contentLoading = true;
      try {
        fileContent = await invoke<string>("read_skill_content", {
          skillId: skill.id,
        });
      } catch {
        fileContent = "// Could not read file";
      } finally {
        contentLoading = false;
      }
    }
  }

  function handleStarClick(e: MouseEvent) {
    e.stopPropagation();
    starAnimating = true;
    toggleStar(skill.id);
    setTimeout(() => { starAnimating = false; }, 250);
  }

  function handleCopyClick(e: MouseEvent) {
    e.stopPropagation();
    copySkillReference(skill);
  }

  async function handleCheckUpdate(e: MouseEvent) {
    e.stopPropagation();
    repoCheckLoading = true;
    await checkSkillUpdate(skill);
    repoCheckLoading = false;
  }

  async function handleSnapshot(e: MouseEvent) {
    e.stopPropagation();
    snapshotLoading = true;
    await snapshotSkillBeforeUpdate(skill, updateStatus?.remoteRef ?? undefined, "before-update");
    snapshotLoading = false;
  }

  async function handleLoadHistory(e: MouseEvent) {
    e.stopPropagation();
    historyLoading = true;
    await loadSkillVersionHistory(skill.id);
    historyLoading = false;
  }

  async function handleRestore(e: MouseEvent, versionId: string) {
    e.stopPropagation();
    restoringVersionId = versionId;
    await restoreSkillVersion(skill, versionId);
    restoringVersionId = null;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      handleClick();
    } else if (e.key === "c" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      copySkillReference(skill);
    }
  }

  const triggerLabel = $derived(
    skill.metadata.trigger === "autoAttach" ? "auto"
    : skill.metadata.trigger === "user" ? "manual"
    : skill.metadata.trigger ?? null
  );

  const contentPreview = $derived.by(() => {
    if (!fileContent) return null;
    return renderSkillContent(fileContent, 220);
  });

  const updateStatus = $derived(store.updateStatus[skill.id] ?? null);
  const historyEntries = $derived(store.versionHistory[skill.id] ?? []);

  const repoUrlDisplay = $derived.by(() => {
    const value = skill.metadata.repositoryUrl?.trim() ?? "";
    if (!value) return null;
    try {
      const parsed = new URL(value.startsWith("github.com/") ? `https://${value}` : value);
      if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
      return parsed.toString();
    } catch {
      return null;
    }
  });

  const entryDelayMs = $derived(Math.min(delayIndex, 8) * 16);
</script>

<!-- Row -->
<div
  class="row-enter group relative flex items-center gap-2 rounded-[var(--radius-sm)] px-2 cursor-pointer
    transition-[background-color] duration-[120ms] ease-out select-none
    hover:bg-[var(--color-surface-2)]
    {isExpanded ? 'bg-[var(--color-surface-2)]' : ''}
    {isFocused ? 'row-focused' : ''}"
  style="min-height: 34px; animation-delay: {entryDelayMs}ms;"
  data-index={index}
  role="option"
  tabindex={isFocused ? 0 : -1}
  aria-selected={isFocused}
  onclick={handleClick}
  onkeydown={handleKeydown}
>
  <!-- Expand indicator -->
  <svg
    class="h-2.5 w-2.5 shrink-0 text-[var(--color-text-muted)] opacity-0
      group-hover:opacity-40
      transition-[transform,opacity] duration-200 ease-in-out
      {isExpanded ? 'rotate-90 !opacity-50' : ''}"
    fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
  >
    <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
  </svg>

  <!-- Skill name -->
  <span class="min-w-0 flex-1 truncate text-[12px] font-[450]
    text-[var(--color-text-primary)] leading-none">
    {skill.name}
  </span>

  <!-- Inline metadata (right side) -->
  <div class="flex shrink-0 items-center gap-2">
    {#if triggerLabel}
      <span class="text-[9px] font-medium uppercase tracking-[0.06em]
        text-[var(--color-text-muted)]">
        {triggerLabel}
      </span>
    {/if}

    <span
      class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium"
      style="border-color: var(--color-border); color: var(--color-text-muted); background: var(--color-surface-1);"
      title="Artifact type"
    >
      {skill.artifactType}
    </span>

    {#if skill.metadata.version}
      <span
        class="rounded-md border px-1.5 py-0.5 text-[9px] font-semibold tabular-nums
          text-[var(--color-text-secondary)]"
        style="border-color: var(--color-border); background: var(--color-surface-3);"
      >
        v{skill.metadata.version}
      </span>
    {/if}

    {#if skill.updateAvailable}
      <span class="h-[5px] w-[5px] rounded-full bg-[var(--color-update-available)]"
        title="Update available"></span>
    {/if}

    <!-- Star -->
    <button
      class="flex h-6 w-6 items-center justify-center rounded-md text-[14px] leading-none
        transition-all duration-[120ms] ease-out
        {skill.starred
          ? 'text-[var(--color-starred)] bg-[var(--color-surface-3)]'
          : 'text-[var(--color-text-secondary)] opacity-80 hover:bg-[var(--color-surface-3)] hover:text-[var(--color-starred)] hover:opacity-100'}
        {starAnimating ? 'star-pop' : ''}"
      onclick={handleStarClick}
      aria-label={skill.starred ? "Unstar" : "Star"}
    >{skill.starred ? "★" : "☆"}</button>

    <!-- Copy -->
    <button
      class="flex h-6 w-6 items-center justify-center rounded-md
        text-[var(--color-text-secondary)] opacity-80
        hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)] hover:opacity-100
        transition-all duration-[120ms] ease-out"
      onclick={handleCopyClick}
      aria-label="Copy reference"
    >
      <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round"
          d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
      </svg>
    </button>
  </div>
</div>

<!-- Expanded content -->
{#if isExpanded}
  <div
    class="ml-6 mr-1 mb-1.5 space-y-2 border-l border-[var(--color-border)] pl-3 pt-1.5 pb-2"
    transition:slide={{ duration: 200, easing: cubicOut }}
  >
    <!-- Description -->
    {#if skill.description}
      <p class="text-[11px] leading-relaxed text-[var(--color-text-secondary)]">
        {skill.description}
      </p>
    {/if}

    <!-- Quick metadata bar -->
    {#if skill.metadata.allowedTools || skill.metadata.globs || skill.metadata.author}
      <div class="flex flex-wrap gap-x-4 gap-y-1 text-[10px]">
        {#if skill.metadata.allowedTools}
          <div class="flex gap-1.5">
            <span class="font-medium text-[var(--color-text-secondary)]">Tools</span>
            <span class="text-[var(--color-text-muted)]">{skill.metadata.allowedTools}</span>
          </div>
        {/if}
        {#if skill.metadata.globs}
          <div class="flex gap-1.5">
            <span class="font-medium text-[var(--color-text-secondary)]">Globs</span>
            <span class="text-[var(--color-text-muted)]">{skill.metadata.globs.join(", ")}</span>
          </div>
        {/if}
        {#if skill.metadata.author}
          <div class="flex gap-1.5">
            <span class="font-medium text-[var(--color-text-secondary)]">Author</span>
            <span class="text-[var(--color-text-muted)]">{skill.metadata.author}</span>
          </div>
        {/if}
      </div>
    {/if}

    <!-- File content preview -->
    <div class="rounded-[var(--radius-md)] overflow-hidden border border-[var(--color-code-border)]"
      style="background: var(--color-code-bg);">
      {#if contentLoading}
        <div class="flex items-center gap-2 px-3 py-3 text-[10px] text-[var(--color-text-muted)]">
          <span class="h-3 w-3 rounded-full border-[1.5px] border-[var(--color-accent)] border-t-transparent spin"></span>
          Reading…
        </div>
      {:else if contentPreview}
        <div class="skill-content-preview px-3 py-2 max-h-[180px] overflow-y-auto">
          {@html contentPreview.html}
          {#if contentPreview.truncated}
            <div class="skill-truncate-note">... {contentPreview.hiddenLineCount} more lines</div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- File path + install command -->
    <div class="space-y-0.5">
      <p class="truncate font-mono text-[9px] text-[var(--color-text-secondary)] opacity-95">
        {skill.filePath}
      </p>

        {#if skill.metadata.repositoryUrl}
        <div class="flex items-center gap-1.5">
          {#if repoUrlDisplay}
            <a
              href={repoUrlDisplay}
              target="_blank"
              rel="noopener noreferrer"
              class="truncate text-[9px] text-[var(--color-accent)] hover:underline"
              onclick={(e) => e.stopPropagation()}
            >
              {repoUrlDisplay.replace(/^https?:\/\//, "")}
            </a>
          {:else}
            <span class="truncate text-[9px] text-[var(--color-error)]">Invalid repo URL</span>
          {/if}

          <button
            class="shrink-0 rounded px-1.5 py-0.5 text-[9px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
            onclick={handleCheckUpdate}
            disabled={repoCheckLoading || !repoUrlDisplay}
            title="Check live update status"
          >
            {repoCheckLoading ? "..." : "check"}
          </button>

          <button
            class="shrink-0 rounded px-1.5 py-0.5 text-[9px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
            onclick={handleSnapshot}
            disabled={snapshotLoading}
            title="Archive local snapshot before updating"
          >
            {snapshotLoading ? "..." : "archive"}
          </button>

          <button
            class="shrink-0 rounded px-1.5 py-0.5 text-[9px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
            onclick={handleLoadHistory}
            disabled={historyLoading}
            title="Load archived versions"
          >
            {historyLoading ? "..." : "history"}
          </button>
        </div>

        {#if updateStatus?.error}
          <p class="truncate text-[9px] text-[var(--color-error)]">{updateStatus.error}</p>
        {/if}

        {#if historyEntries.length > 0}
          <div class="max-h-[88px] overflow-y-auto rounded border border-[var(--color-border)] bg-[var(--color-surface-1)] p-1.5 text-[9px]">
            {#each historyEntries as entry (entry.versionId)}
              <div class="mb-1 flex items-center justify-between gap-1 last:mb-0">
                <span class="truncate text-[var(--color-text-muted)]">
                  {entry.reason} {new Date(entry.createdAt * 1000).toLocaleString()}
                </span>
                <button
                  class="rounded px-1 py-0.5 text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                  onclick={(e) => handleRestore(e, entry.versionId)}
                  disabled={restoringVersionId === entry.versionId}
                >
                  {restoringVersionId === entry.versionId ? "..." : "restore"}
                </button>
              </div>
            {/each}
          </div>
        {/if}
        {/if}

        {#if skill.metadata.slashCommand}
          <p class="truncate font-mono text-[9px] text-[var(--color-text-secondary)] opacity-95">
            slash {skill.metadata.slashCommand}
          </p>
        {/if}

        {#if skill.metadata.hookEvent}
          <p class="truncate text-[9px] text-[var(--color-text-secondary)] opacity-95">
            hook event {skill.metadata.hookEvent}{skill.metadata.hookMatcher ? `, matcher ${skill.metadata.hookMatcher}` : ""}
          </p>
        {/if}

        {#if skill.metadata.hookCommand}
          <p class="truncate font-mono text-[9px] text-[var(--color-accent-muted)]">
            {skill.metadata.hookCommand}
          </p>
        {/if}

        {#if skill.metadata.installCommand}
          <p class="truncate font-mono text-[9px] text-[var(--color-accent-muted)]">
            {skill.metadata.installCommand}
        </p>
      {/if}
    </div>
  </div>
{/if}
