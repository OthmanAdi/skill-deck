<!--
  @agent-context: Compare / view archived snapshots.

  Two modes share one modal:
  - VIEW: shows a single snapshot's content, read-only.
  - COMPARE: line-aware diff between two sources. The user picks the right-hand
    side from a dropdown listing every OTHER archive of the same skill, plus
    a virtual "Current file" option that diffs against the live skill on disk.
    When the skill has only one archive, COMPARE falls back to "Current file"
    automatically because there is nothing else to compare to.

  Visuals follow FullSkillModal: glass surface, fade backdrop, fly-in panel,
  focus trap, Escape to close. The diff renders as a side-by-side table with
  per-line gutter numbers and red / green tint for removed / added lines.
-->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";

  import {
    closeDiffModal,
    readSkillSnapshot,
    showToast,
    store,
  } from "$lib/stores/skills.svelte";
  import { diffLines, type DiffResult, type DiffRow } from "$lib/utils/diffLines";
  import { renderSkillContent } from "$lib/utils/renderSkillContent";
  import { highlightCode, languageFromPath } from "$lib/utils/highlight";

  const CURRENT_FILE_SENTINEL = "__current_file__";

  let panelEl: HTMLDivElement | undefined = $state();
  let closeButtonEl: HTMLButtonElement | undefined = $state();
  let previousFocusEl: HTMLElement | null = null;

  let leftContent = $state<string | null>(null);
  let leftMeta = $state<{ label: string; sub: string } | null>(null);
  let rightContent = $state<string | null>(null);
  let rightMeta = $state<{ label: string; sub: string } | null>(null);
  let loading = $state(false);
  let errorMessage = $state<string | null>(null);
  /** Selected "other side" id for compare mode — version id or sentinel. */
  let compareWith = $state<string>(CURRENT_FILE_SENTINEL);

  const open = $derived(store.diffModalOpen);
  const mode = $derived(store.diffModalMode);
  const skill = $derived(store.diffModalSkill);
  const versionId = $derived(store.diffModalVersionId);

  const historyEntries = $derived.by(() => {
    if (!skill) return [];
    return store.versionHistory[skill.id] ?? [];
  });

  /** Other archives we can compare against (everything except `versionId`). */
  const otherEntries = $derived.by(() =>
    historyEntries.filter((entry) => entry.versionId !== versionId)
  );

  function formatEntryLabel(entry: { createdAt: number; reason: string }): string {
    const stamp = new Date(entry.createdAt * 1000);
    const dateStr = stamp.toLocaleString();
    return `${entry.reason} • ${dateStr}`;
  }

  function metaForCurrentFile(): { label: string; sub: string } {
    return {
      label: "Current file",
      sub: skill?.filePath ?? "",
    };
  }

  function metaForEntry(entry: {
    versionId: string;
    createdAt: number;
    reason: string;
  }): { label: string; sub: string } {
    return {
      label: entry.reason,
      sub: new Date(entry.createdAt * 1000).toLocaleString(),
    };
  }

  async function loadContents() {
    if (!skill || !versionId) return;
    loading = true;
    errorMessage = null;
    leftContent = null;
    rightContent = null;

    try {
      const leftSnap = await readSkillSnapshot(skill, versionId);
      if (!leftSnap) {
        errorMessage = "Could not load snapshot";
        return;
      }
      leftContent = leftSnap.content;
      leftMeta = metaForEntry(leftSnap);

      if (mode === "view") {
        rightContent = null;
        rightMeta = null;
        return;
      }

      // COMPARE: load the chosen "other side".
      if (compareWith === CURRENT_FILE_SENTINEL) {
        try {
          const current = await invoke<string>("read_skill_content", { skillId: skill.id });
          rightContent = current;
          rightMeta = metaForCurrentFile();
        } catch (e) {
          errorMessage = `Could not read current file: ${e}`;
        }
      } else {
        const otherSnap = await readSkillSnapshot(skill, compareWith);
        if (!otherSnap) {
          errorMessage = "Could not load comparison snapshot";
          return;
        }
        rightContent = otherSnap.content;
        rightMeta = metaForEntry(otherSnap);
      }
    } catch (e) {
      errorMessage = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  // Re-load when modal opens or when the user picks a different "compare with".
  $effect(() => {
    if (!open) return;
    // Default compareWith: prefer the most recent OTHER entry if available, else current file.
    if (mode === "compare") {
      if (otherEntries.length > 0) {
        // Keep current selection if still valid; otherwise pick newest other entry.
        const stillValid = otherEntries.some((e) => e.versionId === compareWith);
        if (!stillValid) {
          compareWith = otherEntries[0].versionId;
        }
      } else {
        compareWith = CURRENT_FILE_SENTINEL;
      }
    }
    void loadContents();
  });

  const diff = $derived.by<DiffResult | null>(() => {
    if (mode !== "compare") return null;
    if (leftContent === null || rightContent === null) return null;
    return diffLines(leftContent, rightContent);
  });

  /**
   * Render the View pane through `renderSkillContent` so it gets the same
   * markdown structure (headings, lists, frontmatter, fenced code blocks with
   * syntax highlighting) as FullSkillModal. For non-markdown files this
   * automatically falls back to a single highlighted code block.
   */
  const viewRendered = $derived.by(() => {
    if (mode !== "view") return null;
    if (leftContent === null) return null;
    return renderSkillContent(leftContent, {
      maxLines: Number.MAX_SAFE_INTEGER,
      filePath: skill?.filePath ?? null,
    });
  });

  /**
   * For Compare mode, pre-highlight every diff row's left/right line content
   * using the source file's language so reviewers see actual syntax colors
   * instead of monochrome text. Done in one pass and cached via `$derived`
   * so we don't re-highlight on every re-render.
   */
  const diffLanguage = $derived.by(() => {
    return languageFromPath(skill?.filePath ?? null) ?? "markdown";
  });

  interface HighlightedDiffRow extends DiffRow {
    leftHtml: string;
    rightHtml: string;
  }

  const highlightedDiffRows = $derived.by<HighlightedDiffRow[] | null>(() => {
    if (!diff) return null;
    const lang = diffLanguage;
    return diff.rows.map((row) => ({
      ...row,
      leftHtml: row.left ? highlightCode(row.left, lang).html : "",
      rightHtml: row.right ? highlightCode(row.right, lang).html : "",
    }));
  });

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      closeDiffModal();
    }
  }

  function handleBackdropKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeDiffModal();
    }
  }

  $effect(() => {
    if (open) {
      previousFocusEl =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      requestAnimationFrame(() => closeButtonEl?.focus());
      return;
    }
    if (previousFocusEl) {
      previousFocusEl.focus();
      previousFocusEl = null;
    }
  });

  async function copyContent(content: string | null, kind: string) {
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      showToast(`Copied ${kind}`);
    } catch {
      showToast(`Could not copy ${kind}`);
    }
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-[210] flex items-center justify-center bg-black/55 px-4"
    transition:fade={{ duration: 140 }}
    onclick={handleBackdropClick}
    onkeydown={handleBackdropKeydown}
    role="presentation"
  >
    <div
      bind:this={panelEl}
      class="flex max-h-[88vh] w-full max-w-[860px] flex-col overflow-hidden rounded-lg border shadow-2xl"
      style="background: var(--color-surface-1); border-color: var(--color-border);"
      transition:fly={{ y: 12, duration: 180, easing: cubicOut }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="diff-modal-title"
    >
      <!-- Header -->
      <header
        class="flex shrink-0 items-start justify-between gap-3 border-b px-4 py-3"
        style="border-color: var(--color-border); background: var(--color-surface-2);"
      >
        <div class="min-w-0 flex-1">
          <p
            class="truncate text-[10px] uppercase tracking-[0.08em] text-[var(--color-text-muted)]"
          >
            {mode === "compare" ? "Compare archives" : "Archive preview"}
          </p>
          <h2
            id="diff-modal-title"
            class="truncate text-[14px] font-semibold text-[var(--color-text-primary)]"
          >
            {skill?.name ?? "Skill"}
          </h2>
          {#if skill}
            <p class="truncate font-mono text-[9px] text-[var(--color-text-muted)]">
              {skill.filePath}
            </p>
          {/if}
        </div>

        <button
          bind:this={closeButtonEl}
          type="button"
          class="rounded-md border px-2 py-1 text-[10px] font-medium transition-colors duration-150
            hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-primary)]"
          style="border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-secondary);"
          onclick={closeDiffModal}
          aria-label="Close diff dialog"
        >
          Close
        </button>
      </header>

      <!-- Controls / metadata bar -->
      <div
        class="flex shrink-0 flex-wrap items-center gap-2 border-b px-4 py-2 text-[10px]"
        style="border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-secondary);"
      >
        {#if mode === "compare"}
          <span class="font-medium text-[var(--color-text-muted)]">Comparing</span>
          {#if leftMeta}
            <span
              class="instant-tooltip rounded-md border px-1.5 py-0.5"
              style="border-color: var(--color-border); background: var(--color-surface-2);"
              data-tooltip={leftMeta.sub}
            >
              {leftMeta.label}
            </span>
          {/if}
          <span class="text-[var(--color-text-muted)]">vs</span>

          <select
            class="rounded-md border px-1.5 py-0.5 text-[10px] focus:outline-none"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-primary);"
            bind:value={compareWith}
          >
            {#if otherEntries.length === 0}
              <option value={CURRENT_FILE_SENTINEL}>Current file (only option)</option>
            {:else}
              <option value={CURRENT_FILE_SENTINEL}>Current file (live on disk)</option>
              {#each otherEntries as entry (entry.versionId)}
                <option value={entry.versionId}>{formatEntryLabel(entry)}</option>
              {/each}
            {/if}
          </select>

          {#if diff}
            <span class="ml-auto flex items-center gap-1.5 text-[var(--color-text-muted)]">
              <span class="rounded-md border px-1.5 py-0.5"
                style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-accent);">
                +{diff.summary.added}
              </span>
              <span class="rounded-md border px-1.5 py-0.5"
                style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-error);">
                -{diff.summary.removed}
              </span>
              <span class="opacity-75">{diff.summary.unchanged} unchanged</span>
            </span>
          {/if}
        {:else if leftMeta}
          <span
            class="instant-tooltip rounded-md border px-1.5 py-0.5"
            style="border-color: var(--color-border); background: var(--color-surface-2);"
            data-tooltip={leftMeta.sub}
          >
            {leftMeta.label}
          </span>
          <span class="text-[var(--color-text-muted)]">{leftMeta.sub}</span>

          <button
            type="button"
            class="ml-auto rounded-md border px-1.5 py-0.5 text-[10px] font-medium transition-colors duration-150
              hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
            onclick={() => copyContent(leftContent, "snapshot")}
          >
            Copy text
          </button>
        {/if}
      </div>

      <!-- Body -->
      <div class="diff-body min-h-0 flex-1 overflow-auto">
        {#if loading}
          <div class="flex items-center gap-2 px-4 py-6 text-[11px] text-[var(--color-text-muted)]">
            <span
              class="h-3 w-3 rounded-full border-[1.5px] border-[var(--color-accent)] border-t-transparent"
              style="animation: refresh-spin 0.7s linear infinite;"
            ></span>
            Loading snapshot...
          </div>
        {:else if errorMessage}
          <div class="px-4 py-6 text-[11px] text-[var(--color-error)]">{errorMessage}</div>
        {:else if mode === "view"}
          {#if viewRendered}
            <div
              class="skill-content-preview full-skill-content px-4 py-3 hljs"
              style="background: var(--color-code-bg); color: var(--color-text-primary);"
            >
              {@html viewRendered.html}
            </div>
          {:else}
            <p class="px-4 py-6 text-[11px] text-[var(--color-text-muted)]">No content available</p>
          {/if}
        {:else if highlightedDiffRows && diff}
          {@const truncated = diff.summary.truncated}
          <table
            class="hljs w-full border-separate text-[11px] font-mono leading-snug"
            style="background: var(--color-code-bg); border-spacing: 0;"
            data-language={diffLanguage}
          >
            <colgroup>
              <col style="width: 32px;" />
              <col style="width: 12px;" />
              <col />
              <col style="width: 32px;" />
              <col style="width: 12px;" />
              <col />
            </colgroup>
            <tbody>
              {#each highlightedDiffRows as row, idx (idx)}
                <tr
                  class="align-top"
                  style="background: {row.kind === 'add'
                    ? 'rgba(46, 160, 67, 0.10)'
                    : row.kind === 'remove'
                      ? 'rgba(248, 81, 73, 0.10)'
                      : 'transparent'};"
                >
                  <td
                    class="select-none px-2 text-right text-[9px] text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.leftNumber ?? ""}
                  </td>
                  <td
                    class="px-1 text-center text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.kind === "remove" ? "-" : row.kind === "add" ? "" : ""}
                  </td>
                  <td
                    class="diff-code-cell whitespace-pre-wrap break-words px-2"
                    style="border-right: 1px solid var(--color-border);"
                  >{@html row.leftHtml}</td>

                  <td
                    class="select-none px-2 text-right text-[9px] text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.rightNumber ?? ""}
                  </td>
                  <td
                    class="px-1 text-center text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.kind === "add" ? "+" : row.kind === "remove" ? "" : ""}
                  </td>
                  <td
                    class="diff-code-cell whitespace-pre-wrap break-words px-2"
                  >{@html row.rightHtml}</td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if truncated}
            <p
              class="border-t px-4 py-2 text-[10px] text-[var(--color-text-muted)]"
              style="border-color: var(--color-border); background: var(--color-surface-1);"
            >
              Diff truncated to the first 4000 lines per side. Use the View action
              instead to inspect the full snapshot.
            </p>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}
