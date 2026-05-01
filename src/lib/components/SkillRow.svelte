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
  import { toggleStar, copySkillReference } from "$lib/stores/skills.svelte";
  import { highlightLine } from "$lib/utils/highlight";

  let {
    skill,
    index = 0,
    isFocused = false,
  }: { skill: Skill; index?: number; isFocused?: boolean } = $props();

  let isExpanded = $state(false);
  let fileContent = $state<string | null>(null);
  let contentLoading = $state(false);
  let starAnimating = $state(false);

  async function handleClick() {
    isExpanded = !isExpanded;
    if (isExpanded && fileContent === null) {
      contentLoading = true;
      try {
        fileContent = await invoke<string>("read_skill_content", {
          skillId: skill.id,
          projectPath: skill.projectPath,
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
    const lines = fileContent.split("\n");
    const previewLines = lines.slice(0, 22);
    return { lines: previewLines, truncated: lines.length > 22, totalLines: lines.length };
  });
</script>

<!-- Row -->
<div
  class="row-enter group relative flex items-center gap-2 rounded-[var(--radius-sm)] px-2 cursor-pointer
    transition-[background-color] duration-[120ms] ease-out select-none
    hover:bg-[var(--color-surface-2)]
    {isExpanded ? 'bg-[var(--color-surface-2)]' : ''}
    {isFocused ? 'row-focused' : ''}"
  style="min-height: 34px; animation-delay: {index * 20}ms;"
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

    {#if skill.metadata.version}
      <span class="text-[9px] tabular-nums text-[var(--color-text-muted)] opacity-50">
        v{skill.metadata.version}
      </span>
    {/if}

    {#if skill.scope === "project"}
      <span class="text-[8px] font-semibold uppercase tracking-[0.06em]
        text-[var(--color-accent-muted)]">
        proj
      </span>
    {/if}

    {#if skill.updateAvailable}
      <span class="h-[5px] w-[5px] rounded-full bg-[var(--color-update-available)]"
        title="Update available"></span>
    {/if}

    <!-- Star -->
    <button
      class="rounded p-0.5 text-[11px] leading-none
        transition-all duration-[120ms] ease-out
        {skill.starred
          ? 'text-[var(--color-starred)]'
          : 'text-[var(--color-text-muted)] opacity-0 group-hover:opacity-40 hover:!opacity-100 hover:text-[var(--color-starred)]'}
        {starAnimating ? 'star-pop' : ''}"
      onclick={handleStarClick}
      aria-label={skill.starred ? "Unstar" : "Star"}
    >{skill.starred ? "★" : "☆"}</button>

    <!-- Copy -->
    <button
      class="rounded p-0.5 text-[var(--color-text-muted)]
        opacity-0 group-hover:opacity-40 hover:!opacity-100
        hover:text-[var(--color-accent)]
        transition-all duration-[120ms] ease-out"
      onclick={handleCopyClick}
      aria-label="Copy reference"
    >
      <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
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
        <div class="skill-content-preview px-3 py-2 max-h-[180px] overflow-y-auto">{#each contentPreview.lines as line, lineIdx}{@html highlightLine(line, lineIdx, contentPreview.lines)}{"\n"}{/each}{#if contentPreview.truncated}<span class="text-[var(--color-text-muted)] opacity-35">… {contentPreview.totalLines - 22} more lines</span>{/if}</div>
      {/if}
    </div>

    <!-- File path + install command -->
    <div class="space-y-0.5">
      <p class="truncate font-mono text-[9px] text-[var(--color-text-muted)] opacity-45">
        {skill.filePath}
      </p>
      {#if skill.metadata.installCommand}
        <p class="truncate font-mono text-[9px] text-[var(--color-accent-muted)]">
          {skill.metadata.installCommand}
        </p>
      {/if}
    </div>
  </div>
{/if}
