<!--
  @agent-context: Individual skill card on a flat surface-1 carbon surface (no
  glassmorphism) with content preview. Collapsed density follows DESIGN 8:
  agent badge + at most 2 meta chips + 1 signal (the update dot); everything
  else is demoted into the expanded body, nothing removed. Expanded view fetches
  and displays the actual skill file content; a read failure shows a real error
  + Retry rather than a fake "// Could not read file" string.
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
    checkSkillUpdate,
    setSkillRepo,
    snapshotSkillBeforeUpdate,
    loadSkillVersionHistory,
    restoreSkillVersion,
    restoringVersionForSkill,
    deleteSkillVersion,
    openDiffModal,
    openFullSkillModal,
    store,
  } from "$lib/stores/skills.svelte";
  import ConfirmPopover from "./ConfirmPopover.svelte";
  import { renderSkillContent } from "$lib/utils/renderSkillContent";
  import { relativeTime, absoluteTime } from "$lib/utils/formatTime";
  import AgentBadge from "./AgentBadge.svelte";
  import Spinner from "./ui/Spinner.svelte";
  import {
    emojiPicker,
    requestOpenEmojiPicker,
  } from "$lib/stores/emojiPicker.svelte";

  let {
    skill,
    index = 0,
    isFocused = false,
    hasChildren = false,
    childrenCollapsed = false,
    childrenCount = 0,
    onToggleChildren,
    enter = true,
  }: {
    skill: Skill;
    index?: number;
    isFocused?: boolean;
    hasChildren?: boolean;
    childrenCollapsed?: boolean;
    childrenCount?: number;
    onToggleChildren?: () => void;
    /** Play the one-time card-entrance stagger (first reveal only). */
    enter?: boolean;
  } = $props();

  function handleChildrenChevron(e: MouseEvent) {
    e.stopPropagation();
    onToggleChildren?.();
  }

  let isExpanded = $state(false);
  let starAnimating = $state(false);
  let fileContent = $state<string | null>(null);
  let contentLoading = $state(false);
  let contentError = $state<string | null>(null);

  // Per-card flag derived from the global picker so styling / z-index still
  // reflects which card is hosting the open popover.
  const emojiPickerOpen = $derived(
    emojiPicker.open && emojiPicker.skill?.id === skill.id
  );

  function handleStarClick(e: MouseEvent) {
    e.stopPropagation();
    starAnimating = true;
    toggleStar(skill.id);
    setTimeout(() => { starAnimating = false; }, 220); // matches .star-pop keyframe
  }

  function handleIconClick(e: MouseEvent) {
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement | null;
    if (!target) return;
    requestOpenEmojiPicker(skill, target.getBoundingClientRect());
  }

  function handleCopyClick(e: MouseEvent) {
    e.stopPropagation();
    copySkillReference(skill);
  }

  function handleOpenFullSkill(e: MouseEvent) {
    e.stopPropagation();
    void openFullSkillModal(skill, fileContent);
  }

  async function loadContent() {
    contentLoading = true;
    contentError = null;
    try {
      fileContent = await invoke<string>("read_skill_content", {
        skillId: skill.id,
      });
    } catch (e) {
      // Real reason + retry affordance instead of a fake "// Could not read file".
      contentError = e instanceof Error ? e.message : String(e);
      fileContent = null;
    } finally {
      contentLoading = false;
    }
  }

  async function handleCardClick() {
    isExpanded = !isExpanded;
    if (isExpanded && fileContent === null && !contentError) {
      await loadContent();
    }
  }

  function handleRetryContent(e: MouseEvent) {
    e.stopPropagation();
    void loadContent();
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

  // ── Repo UI state ──
  let repoInputVisible = $state(false);
  let repoInputValue = $state("");
  let repoCheckLoading = $state(false);
  let historyLoading = $state(false);
  let snapshotLoading = $state(false);

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

  const updateStatus = $derived(store.updateStatus[skill.id] ?? null);
  const historyEntries = $derived(store.versionHistory[skill.id] ?? []);
  const restoringVersionId = $derived(restoringVersionForSkill(skill));

  async function handleCheckUpdate() {
    repoCheckLoading = true;
    await checkSkillUpdate(skill);
    repoCheckLoading = false;
  }

  async function handleSnapshot() {
    snapshotLoading = true;
    await snapshotSkillBeforeUpdate(skill, updateStatus?.remoteRef ?? undefined, "before-update");
    snapshotLoading = false;
  }

  async function handleLoadHistory() {
    historyLoading = true;
    await loadSkillVersionHistory(skill.id);
    historyLoading = false;
  }

  async function handleRestore(versionId: string) {
    // The shared store protects the same skill across every renderer.
    if (restoringVersionId !== null) return;
    await restoreSkillVersion(skill, versionId);
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
    return renderSkillContent(fileContent, { maxLines: 240, filePath: skill.filePath });
  });

  const parentSkill = $derived.by(() => {
    if (!skill.parentId) return null;
    return store.skills.find((s) => s.id === skill.parentId) ?? null;
  });

  const childSkills = $derived.by(() =>
    store.skills.filter((s) => s.parentId === skill.id)
  );

  const sourceAgentNames = $derived.by(() =>
    (skill.sourceAgents ?? [])
      .map((agent) => {
        const id = typeof agent === "string" ? agent : "custom";
        return store.getAgentDisplayName(id);
      })
      .filter((name, index, list) => list.indexOf(name) === index)
      .sort((a, b) => a.localeCompare(b))
  );

  const extraSourceAgentNames = $derived.by(() => {
    const current = store.getAgentDisplayName(typeof skill.agentId === "string" ? skill.agentId : "custom");
    return sourceAgentNames.filter((name) => name !== current);
  });

  const entryDelayMs = $derived(Math.min(index, 8) * 16);

  // Install / updated timestamp pill — shows on install-related and updated-newest sorts.
  const timeAwareSort = $derived(
    store.skillSortMode === "installed-newest"
      || store.skillSortMode === "installed-oldest"
      || store.skillSortMode === "updated-newest"
  );
  const installedRelative = $derived(relativeTime(skill.installedAt));
  const installedAbsolute = $derived(absoluteTime(skill.installedAt));
  const lastModifiedAt = $derived(skill.lastModifiedAt ?? null);
  const updatedRelative = $derived(relativeTime(lastModifiedAt));
  const updatedAbsolute = $derived(absoluteTime(lastModifiedAt));
  // Treat the file as "updated since install" only when the on-disk mtime is
  // meaningfully newer (60 second grace) than the recorded install timestamp.
  const wasUpdatedAfterInstall = $derived(
    lastModifiedAt !== null
      && skill.installedAt !== null
      && skill.installedAt !== undefined
      && lastModifiedAt > skill.installedAt + 60
  );
  const showTimePill = $derived(
    timeAwareSort && (installedRelative !== null || updatedRelative !== null)
  );
  const pillIsUpdated = $derived(
    store.skillSortMode === "updated-newest" || wasUpdatedAfterInstall
  );
  const timePillRelative = $derived(
    pillIsUpdated ? updatedRelative ?? installedRelative : installedRelative ?? updatedRelative
  );
  const timePillTooltip = $derived(
    pillIsUpdated
      ? `Updated ${updatedAbsolute ?? "?"}${installedAbsolute ? ` • installed ${installedAbsolute}` : ""}`
      : `Installed ${installedAbsolute ?? "?"}${updatedAbsolute ? ` • last modified ${updatedAbsolute}` : ""}`
  );
  const timePillLabel = $derived(pillIsUpdated ? "updated" : "installed");

  // Archive indicator + delete confirm popover state.
  const archiveCount = $derived(skill.archiveCount ?? 0);

  // Collapsed density (DESIGN 8): agent badge + <=2 meta chips + 1 signal (the
  // update dot). artifactType always leads; the second chip is the single most
  // relevant of archive / time / version. Everything else lives in the expanded
  // body (trigger, version, sources, use cases, install time) — nothing removed.
  const metaChips = $derived.by(() => {
    const chips: { text: string; tip: string }[] = [
      { text: skill.artifactType, tip: "Artifact type" },
    ];
    if (archiveCount > 0) {
      chips.push({
        text: `${archiveCount} archived`,
        tip: `${archiveCount} archived version${archiveCount === 1 ? "" : "s"} — expand to manage`,
      });
    } else if (showTimePill && timePillRelative) {
      chips.push({ text: `${timePillLabel} ${timePillRelative}`, tip: timePillTooltip });
    } else if (skill.metadata.version) {
      chips.push({ text: `v${skill.metadata.version}`, tip: "Version" });
    }
    return chips.slice(0, 2);
  });

  let archiveDeleteOpenFor = $state<string | null>(null);
  let archiveDeleteAnchorRect = $state<DOMRect | null>(null);
  let archiveDeleteLoading = $state<string | null>(null);

  function startDeleteVersion(e: MouseEvent, versionId: string) {
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement | null;
    archiveDeleteAnchorRect = target?.getBoundingClientRect() ?? null;
    archiveDeleteOpenFor = versionId;
  }

  async function confirmDeleteVersion() {
    const versionId = archiveDeleteOpenFor;
    if (!versionId) return;
    archiveDeleteLoading = versionId;
    const ok = await deleteSkillVersion(skill, versionId);
    archiveDeleteLoading = null;
    if (ok) {
      archiveDeleteOpenFor = null;
      archiveDeleteAnchorRect = null;
    }
  }
</script>

<div
  class="{enter ? 'card-enter' : ''} group relative flex w-full gap-3 rounded-md p-3 text-left cursor-pointer
    transition-[background-color,border-color] duration-[var(--motion-base)] ease-[var(--ease-standard)]
    border
    {isExpanded
      ? 'border-[var(--color-border-active)] bg-[var(--color-surface-2)]'
      : 'border-transparent bg-[var(--color-surface-1)] hover:border-[var(--color-border-hover)] hover:bg-[var(--color-surface-2)]'}
    {isFocused ? 'card-focused border-[var(--color-accent-muted)]' : ''}
    {emojiPickerOpen ? 'z-[180]' : 'z-0'}"
  style="animation-delay: {entryDelayMs}ms;"
  data-index={index}
  onclick={handleCardClick}
  onkeydown={handleKeydown}
  role="option"
  aria-selected={isFocused}
  tabindex={isFocused ? 0 : -1}
>
  <!-- Icon / Emoji / Letter avatar (click to change icon) -->
  <button
    class="instant-tooltip relative flex h-9 w-9 shrink-0 items-center justify-center rounded-md text-sm font-semibold
      transition-[transform,background-color,border-color] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
      hover:scale-105 active:scale-[0.96]
      {isEmoji ? '' : 'text-[var(--color-text-secondary)]'}"
    style="{isEmoji
      ? ''
      : `background: var(--color-surface-3); border: 1px solid var(--color-border);`}"
    onclick={handleIconClick}
    data-emoji-picker-trigger
    data-tooltip="Pick an emoji icon for this skill"
    aria-label="Change icon"
  >
    <span class={isEmoji ? "emoji-avatar" : "letter-avatar"}>{displayIcon}</span>
  </button>

  <!-- Content -->
  <div class="min-w-0 flex-1">
    <!-- Header row: name + actions -->
    <div class="flex items-start justify-between gap-1.5">
      <div class="min-w-0 flex-1">
        <h3 class="truncate text-sm font-medium leading-snug text-[var(--color-text-primary)]">
          {skill.name}
        </h3>
        <p class="mt-0.5 line-clamp-2 text-xs leading-relaxed text-[var(--color-text-secondary)]">
          {skill.description || "No description"}
        </p>
      </div>

      <!-- Action buttons (archive count is now a meta chip below, not a
           duplicate indicator here) -->
      <div class="flex shrink-0 items-center gap-1">
        {#if hasChildren}
          <!-- Children expand/collapse chevron (inline so parent card keeps full width) -->
          <button
            class="instant-tooltip flex h-6 w-6 items-center justify-center rounded-sm
              text-[var(--color-text-muted)] opacity-85
              transition-[background-color,color,opacity,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
              hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)] hover:opacity-100 active:scale-[0.96]"
            onclick={handleChildrenChevron}
            data-tooltip="{childrenCollapsed ? 'Expand' : 'Collapse'} {childrenCount} sub-skill{childrenCount !== 1 ? 's' : ''}"
            aria-label="{childrenCollapsed ? 'Expand' : 'Collapse'} children"
          >
            <svg
              class="h-3 w-3 transition-transform duration-[var(--motion-base)] ease-[var(--ease-standard)]"
              style="transform: rotate({childrenCollapsed ? '-90deg' : '0deg'});"
              fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M19 9l-7 7-7-7" />
            </svg>
          </button>
        {/if}

        <!-- Copy button -->
        <button
          class="instant-tooltip flex h-6 w-6 items-center justify-center rounded-sm
            text-[var(--color-text-secondary)] opacity-85
            transition-[background-color,color,opacity,scale] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
            hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent-text)] hover:opacity-100 active:scale-[0.96]"
          onclick={handleCopyClick}
          data-tooltip="Copy skill reference (Ctrl+C)"
          aria-label="Copy skill reference"
        >
          <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round"
              d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
          </svg>
        </button>

        <!-- Star button -->
        <button
          class="shrink-0 flex h-6 w-6 items-center justify-center rounded-sm text-lg leading-none
            transition-[background-color,color,opacity] duration-[var(--motion-fast)] ease-[var(--ease-standard)]
            {skill.starred
              ? 'text-[var(--color-starred)] bg-[var(--color-surface-3)]'
              : 'text-[var(--color-text-secondary)] opacity-85 hover:bg-[var(--color-surface-3)] hover:text-[var(--color-starred)] hover:opacity-100'}
            {starAnimating ? 'star-pop' : ''}"
          onclick={handleStarClick}
          aria-label={skill.starred ? "Unstar skill" : "Star skill"}
        >
          {skill.starred ? "\u2605" : "\u2606"}
        </button>
      </div>
    </div>

    <!-- Metadata row (DESIGN 8): agent badge + <=2 meta chips + 1 signal.
         Demoted metadata (trigger, sources, version, use cases) lives in the
         expanded body below — nothing is dropped, only re-ranked by relevance. -->
      <div class="mt-1.5 flex flex-wrap items-center gap-1.5">
        <AgentBadge agentId={skill.agentId} />

        {#each metaChips as chip (chip.text)}
          <span class="badge-meta instant-tooltip" data-tooltip={chip.tip}>{chip.text}</span>
        {/each}

        {#if skill.updateAvailable}
          <!-- The one signal (DESIGN 8): update available, rendered as the calm
               pulsing accent dot via the shared .u-pulse (DESIGN 7, replacing the
               dead `breathe` keyframe) rather than a loud text chip. -->
          <span
            class="instant-tooltip u-pulse h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--color-update-available)]"
            data-tooltip="A newer version of this skill is available"
          ></span>
        {/if}
      </div>

    <!-- Expanded: file content preview -->
    {#if isExpanded}
      <div class="mt-3 space-y-2 border-t border-[var(--color-border)] pt-3"
        transition:slide={{ duration: 200, easing: cubicOut }}
      >
        <!-- Quick metadata (includes trigger + version demoted from the
             collapsed row so no fact is lost) -->
        <div class="flex flex-wrap gap-x-4 gap-y-1 text-2xs">
          {#if skill.metadata.trigger}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Trigger</span>
              <span class="text-[var(--color-text-muted)]">{skill.metadata.trigger}</span>
            </div>
          {/if}
          {#if skill.metadata.version}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Version</span>
              <span class="text-[var(--color-text-muted)] tabular-nums">{skill.metadata.version}</span>
            </div>
          {/if}
          {#if skill.discoveryTags.length > 0}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Tags</span>
              <span class="text-[var(--color-text-muted)]">{skill.discoveryTags.join(", ")}</span>
            </div>
          {/if}
          {#if skill.useCases.length > 0}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">When to use</span>
              <span class="text-[var(--color-text-muted)]">{skill.useCases.join(", ")}</span>
            </div>
          {/if}
          {#if skill.metadata.allowedTools}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Tools</span>
              <span class="text-[var(--color-text-muted)]">{skill.metadata.allowedTools}</span>
            </div>
          {/if}
          {#if skill.metadata.slashCommand}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Slash</span>
              <span class="font-mono text-[var(--color-text-muted)]">{skill.metadata.slashCommand}</span>
            </div>
          {/if}
          {#if skill.metadata.hookEvent}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Hook</span>
              <span class="text-[var(--color-text-muted)]">
                {skill.metadata.hookEvent}{skill.metadata.hookMatcher ? `, ${skill.metadata.hookMatcher}` : ""}
              </span>
            </div>
          {/if}
          {#if skill.metadata.hookCommand}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Command</span>
              <span class="font-mono text-[var(--color-text-muted)]">{skill.metadata.hookCommand}</span>
            </div>
          {/if}
          {#if skill.metadata.globs}
            <div class="flex gap-1">
              <span class="font-medium text-[var(--color-text-secondary)]">Globs</span>
              <span class="text-[var(--color-text-muted)]">{skill.metadata.globs.join(", ")}</span>
            </div>
          {/if}
        </div>

        {#if parentSkill || childSkills.length > 0}
          <div
            class="rounded-sm px-2.5 py-2 text-2xs"
            style="background: var(--color-surface-3); border: 1px solid var(--color-border);"
          >
            <div class="mb-1 text-2xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              hierarchy
            </div>

            {#if parentSkill}
              <div class="flex items-center gap-1.5">
                <span class="rounded-xs px-1.5 py-0.5 text-2xs font-semibold text-[var(--color-text-primary)]"
                  style="background: var(--color-surface-2); border: 1px solid var(--color-border);">
                  parent
                </span>
                <span class="instant-tooltip truncate text-[var(--color-text-secondary)]" data-tooltip={parentSkill.name}>
                  {parentSkill.name}
                </span>
              </div>
            {/if}

            {#if childSkills.length > 0}
              <div class="mt-1.5 space-y-1">
                <div class="flex items-center gap-1.5">
                  <span class="rounded-xs px-1.5 py-0.5 text-2xs font-semibold text-[var(--color-text-primary)]"
                    style="background: var(--color-surface-2); border: 1px solid var(--color-border);">
                    children
                  </span>
                  <span class="text-[var(--color-text-secondary)] opacity-85">{childSkills.length}</span>
                </div>

                <div class="flex flex-wrap gap-1">
                  {#each childSkills.slice(0, 5) as child}
                    <span
                      class="instant-tooltip max-w-[180px] truncate rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-secondary)]"
                      style="background: var(--color-surface-2); border: 1px solid var(--color-border);"
                      data-tooltip={child.name}
                    >
                      {child.name}
                    </span>
                  {/each}

                  {#if childSkills.length > 5}
                    <span class="rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)]"
                      style="background: var(--color-surface-2); border: 1px solid var(--color-border);">
                      +{childSkills.length - 5} more
                    </span>
                  {/if}
                </div>
              </div>
            {/if}
          </div>
        {/if}

        {#if extraSourceAgentNames.length > 0}
          <div
            class="rounded-sm px-2.5 py-2 text-2xs"
            style="background: var(--color-surface-3); border: 1px solid var(--color-border);"
          >
            <div class="mb-1 text-2xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">
              detected via
            </div>
            <div class="flex flex-wrap gap-1">
              {#each extraSourceAgentNames as sourceName}
                <span
                  class="rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-secondary)]"
                  style="background: var(--color-surface-2); border: 1px solid var(--color-border);"
                >
                  {sourceName}
                </span>
              {/each}
            </div>
          </div>
        {/if}

        <!-- File content preview -->
        <div class="flex items-center justify-between gap-2">
          <span class="text-2xs font-medium text-[var(--color-text-secondary)]">Preview</span>
          <button
            class="instant-tooltip rounded-md border px-2 py-1 text-2xs font-medium transition-colors duration-[var(--motion-fast)] ease-[var(--ease-standard)]"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
            onclick={handleOpenFullSkill}
            data-tooltip="Open the full skill body in a modal"
          >
            Open full skill
          </button>
        </div>

        <div class="rounded-sm overflow-hidden"
          style="background: var(--color-code-bg); border: 1px solid var(--color-code-border);">
          {#if contentLoading}
            <div class="flex items-center gap-2 px-3 py-4 text-2xs text-[var(--color-text-muted)]">
              <Spinner size={12} label="Reading skill file" />
              Reading file...
            </div>
          {:else if contentError}
            <!-- Real reason + retry, never the fake "// Could not read file". -->
            <div class="flex flex-col items-start gap-2 px-3 py-3 text-2xs">
              <div class="flex items-center gap-1.5 text-[var(--color-error)]">
                <svg class="h-3.5 w-3.5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" aria-hidden="true">
                  <path stroke-linecap="round" stroke-linejoin="round"
                    d="M12 9v4m0 4h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
                </svg>
                <span class="font-medium">Could not read this skill file</span>
              </div>
              <p class="leading-relaxed text-[var(--color-text-muted)]">{contentError}</p>
              <button class="btn-secondary btn-sm" onclick={handleRetryContent}>Retry</button>
            </div>
          {:else if fileContent === ""}
            <div class="px-3 py-4 text-2xs text-[var(--color-text-muted)]">
              This skill file is empty.
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
        <div class="rounded-sm px-2.5 py-2 text-2xs"
          style="background: var(--color-surface-3); border: 1px solid var(--color-border);">

          {#if skill.metadata.repositoryUrl}
            <!-- Has a repo URL -->
            <div class="flex items-center justify-between gap-2">
              {#if repoUrlDisplay}
                <a
                  href={repoUrlDisplay}
                  target="_blank"
                  rel="noopener noreferrer"
                  class="flex min-w-0 items-center gap-1.5 text-[var(--color-accent-text)] hover:underline"
                  onclick={(e) => e.stopPropagation()}
                >
                  <svg class="h-3 w-3 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                  </svg>
                  <span class="truncate">{repoUrlDisplay.replace(/^https?:\/\//, '')}</span>
                </a>
              {:else}
                <span class="truncate text-[var(--color-text-muted)]">Invalid repo URL</span>
              {/if}
              <button
                class="instant-tooltip shrink-0 rounded-xs px-1.5 py-0.5 text-2xs font-medium transition-[background-color,color] duration-[var(--motion-fast)]
                  text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={(e) => { e.stopPropagation(); handleCheckUpdate(); }}
                disabled={repoCheckLoading || !repoUrlDisplay}
                data-tooltip="Check upstream for a newer version"
              >
                {#if repoCheckLoading}
                  <Spinner size={10} label="Checking for updates" />
                {:else if skill.updateAvailable}
                  ↑ update
                {:else}
                  check live
                {/if}
              </button>
            </div>

            <div class="mt-1.5 flex flex-wrap items-center gap-1">
              <button
                class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs font-medium transition-[background-color,color] duration-[var(--motion-fast)]
                  text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={(e) => { e.stopPropagation(); handleSnapshot(); }}
                disabled={snapshotLoading}
                data-tooltip="Snapshot the current file so you can restore it later"
              >
                {snapshotLoading ? "saving..." : "archive"}
              </button>
              <button
                class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs font-medium transition-[background-color,color] duration-[var(--motion-fast)]
                  text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                onclick={(e) => { e.stopPropagation(); handleLoadHistory(); }}
                disabled={historyLoading}
                data-tooltip="Show the list of archived versions for this skill"
              >
                {historyLoading ? "loading..." : "history"}
              </button>
            </div>

            {#if updateStatus}
              <div class="mt-1 text-2xs text-[var(--color-text-muted)]">
                {#if updateStatus.error}
                  <span class="text-[var(--color-error)]">{updateStatus.error}</span>
                {:else if updateStatus.source === "cache"}
                  Using cached status
                {:else}
                  Live check succeeded
                {/if}
              </div>
            {/if}

            {#if historyEntries.length > 0}
              <div
                class="mt-1.5 max-h-[140px] overflow-y-auto rounded-xs border border-[var(--color-border)] bg-[var(--color-surface-1)] p-1.5"
              >
                {#each historyEntries as entry (entry.versionId)}
                  <div class="mb-1 flex items-center justify-between gap-1 text-2xs last:mb-0">
                    <span
                      class="instant-tooltip truncate text-[var(--color-text-muted)]"
                      data-tooltip={entry.versionId}
                    >
                      {entry.reason} {new Date(entry.createdAt * 1000).toLocaleString()}
                    </span>
                    <div class="flex shrink-0 items-center gap-1">
                      <button
                        type="button"
                        class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                        onclick={(e) => {
                          e.stopPropagation();
                          openDiffModal(skill, entry.versionId, "view");
                        }}
                        data-tooltip="Open this archived version in the diff viewer"
                      >
                        view
                      </button>
                      <button
                        type="button"
                        class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                        onclick={(e) => {
                          e.stopPropagation();
                          openDiffModal(skill, entry.versionId, "compare");
                        }}
                        data-tooltip={historyEntries.length > 1
                          ? "Compare against another archive or the current file"
                          : "Compare against the current file"}
                      >
                        diff
                      </button>
                      <button
                        type="button"
                        class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]"
                        onclick={(e) => { e.stopPropagation(); handleRestore(entry.versionId); }}
                        disabled={restoringVersionId !== null}
                        aria-busy={restoringVersionId === entry.versionId ? "true" : undefined}
                        aria-label={restoringVersionId === entry.versionId
                          ? "Restoring archived version"
                          : "Restore archived version"}
                        data-tooltip="Overwrite the live skill file with this archived version"
                      >
                        {restoringVersionId === entry.versionId ? "restoring..." : "restore"}
                      </button>
                      <button
                        type="button"
                        class="instant-tooltip rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)] hover:bg-[var(--color-surface-2)] hover:text-[var(--color-error)]"
                        onclick={(e) => startDeleteVersion(e, entry.versionId)}
                        disabled={archiveDeleteLoading === entry.versionId}
                        data-tooltip="Permanently delete this archive entry"
                      >
                        {archiveDeleteLoading === entry.versionId ? "..." : "delete"}
                      </button>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {:else}
            <!-- No repo URL — show input or prompt -->
            {#if repoInputVisible}
              <div class="flex items-center gap-1.5" role="group" aria-label="Set repository URL">
                <input
                  type="text"
                  bind:value={repoInputValue}
                  placeholder="https://github.com/owner/repo"
                  class="flex-1 rounded-xs border bg-[var(--color-surface-1)] px-2 py-1 text-2xs
                    text-[var(--color-text-primary)] placeholder-[var(--color-text-muted)]
                    focus:outline-none focus:border-[var(--color-accent-muted)]"
                  style="border-color: var(--color-border);"
                  onkeydown={(e) => { if (e.key === 'Enter') handleSaveRepo(); if (e.key === 'Escape') repoInputVisible = false; }}
                />
                <button
                  class="shrink-0 rounded-xs px-2 py-1 text-2xs font-medium transition-colors
                    bg-[var(--color-accent-subtle)] text-[var(--color-accent-text)]
                    hover:bg-[var(--color-accent-muted)] hover:text-[var(--color-on-accent)]"
                  onclick={handleSaveRepo}
                >save</button>
                <button
                  class="shrink-0 rounded-xs px-1.5 py-1 text-2xs text-[var(--color-text-muted)]
                    hover:text-[var(--color-text-secondary)]"
                  onclick={() => repoInputVisible = false}
                >✕</button>
              </div>
            {:else}
              <button
                class="flex items-center gap-1 text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)]
                  transition-colors duration-[var(--motion-fast)] ease-[var(--ease-standard)]"
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
              <span class="truncate font-mono text-2xs text-[var(--color-text-muted)]">
                {skill.metadata.installCommand}
              </span>
              <button
                class="instant-tooltip shrink-0 rounded-xs px-1.5 py-0.5 text-2xs text-[var(--color-text-muted)]
                  hover:bg-[var(--color-surface-2)] hover:text-[var(--color-accent-text)] transition-colors"
                onclick={(e) => {
                  e.stopPropagation();
                  navigator.clipboard.writeText(skill.metadata.installCommand!).catch(() => {});
                }}
                data-tooltip="Copy the install command to the clipboard"
              >copy</button>
            </div>
          {/if}
        </div>

        <!-- File path -->
        <div class="flex items-center gap-1.5 text-2xs text-[var(--color-text-secondary)] opacity-95">
          <svg class="h-2.5 w-2.5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
          </svg>
          <span class="truncate font-mono">{skill.filePath}</span>
        </div>
      </div>
    {/if}
  </div>
</div>

<ConfirmPopover
  open={archiveDeleteOpenFor !== null}
  anchorRect={archiveDeleteAnchorRect}
  title="Delete this archive?"
  message="The snapshot file is permanently removed and cannot be recovered."
  confirmLabel="Delete"
  tone="danger"
  onConfirm={confirmDeleteVersion}
  onCancel={() => {
    archiveDeleteOpenFor = null;
    archiveDeleteAnchorRect = null;
  }}
/>
