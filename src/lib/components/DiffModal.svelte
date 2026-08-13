<!--
  @agent-context: Compare / view archived snapshots.

  Two modes share one modal:
  - VIEW: shows a single snapshot's content, read-only.
  - COMPARE: line-aware diff between two sources. The user picks the right-hand
    side from a dropdown listing every OTHER archive of the same skill, plus
    a virtual "Current file" option that diffs against the live skill on disk.
    When the skill has only one archive, COMPARE falls back to "Current file"
    automatically because there is nothing else to compare to.

  PHOSPHOR modal (DESIGN 6/8), matching FullSkillModal: .scrim backdrop +
  .layer-modal panel (surface-0, radius-lg, --shadow-modal), scale(0.98->1)+fade
  entrance at --motion-enter, a Tab focus trap + focus restore, Escape to close.
  The diff renders as a side-by-side table in the one mono voice (--font-ui) with
  tabular gutter numbers and --color-ok / --color-error subtle tints for the
  added / removed lines.
-->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";

  import Spinner from "./ui/Spinner.svelte";
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

  // Tab focus trap + Escape, propagated from FullSkillModal (the reference
  // implementation) so focus can never walk out of the open diff dialog into
  // the scrim-covered content behind it.
  function handleWindowKeydown(e: KeyboardEvent) {
    if (!open) return;

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
      showToast(`Copied ${kind}`, "ok");
    } catch {
      showToast(`Could not copy ${kind}`, "error");
    }
  }
</script>

<svelte:window onkeydown={handleWindowKeydown} />

{#if open}
  <div
    class="scrim z-[210] flex items-center justify-center px-4"
    transition:fade={{ duration: 140 }}
    onclick={handleBackdropClick}
    onkeydown={handleBackdropKeydown}
    role="presentation"
  >
    <div
      bind:this={panelEl}
      class="layer-modal modal-panel-enter flex max-h-[88vh] w-full max-w-[860px] flex-col overflow-hidden"
      style="transform-origin: center;"
      out:fly={{ y: 6, duration: 140, easing: cubicOut }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="diff-modal-title"
    >
      <!-- Header -->
      <header
        class="flex shrink-0 items-start justify-between gap-3 border-b px-4 py-3"
        style="border-color: var(--color-border); background: var(--color-surface-1);"
      >
        <div class="min-w-0 flex-1">
          <p class="u-caps truncate">
            {mode === "compare" ? "Compare archives" : "Archive preview"}
          </p>
          <h2
            id="diff-modal-title"
            class="truncate font-display text-lg font-semibold text-[var(--color-text-primary)]"
          >
            {skill?.name ?? "Skill"}
          </h2>
          {#if skill}
            <p class="truncate text-2xs text-[var(--color-text-muted)]">
              {skill.filePath}
            </p>
          {/if}
        </div>

        <button
          bind:this={closeButtonEl}
          type="button"
          class="btn-secondary btn-sm"
          onclick={closeDiffModal}
          aria-label="Close diff dialog"
        >
          Close
        </button>
      </header>

      <!-- Controls / metadata bar -->
      <div
        class="flex shrink-0 flex-wrap items-center gap-2 border-b px-4 py-2 text-2xs"
        style="border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-secondary);"
      >
        {#if mode === "compare"}
          <span class="font-medium text-[var(--color-text-muted)]">Comparing</span>
          {#if leftMeta}
            <span class="badge-meta instant-tooltip" data-tooltip={leftMeta.sub}>
              {leftMeta.label}
            </span>
          {/if}
          <span class="text-[var(--color-text-muted)]">vs</span>

          <select class="field" bind:value={compareWith} aria-label="Compare against">
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
            <span class="ml-auto flex items-center gap-2 text-[var(--color-text-muted)]">
              <span
                class="inline-flex items-center rounded-xs border px-1.5 py-0.5 tabular-nums"
                style="border-color: var(--color-border); background: var(--color-ok-subtle); color: var(--color-ok);"
              >
                +{diff.summary.added}
              </span>
              <span
                class="inline-flex items-center rounded-xs border px-1.5 py-0.5 tabular-nums"
                style="border-color: var(--color-border); background: var(--color-error-subtle); color: var(--color-error);"
              >
                -{diff.summary.removed}
              </span>
              <span class="tabular-nums opacity-75">{diff.summary.unchanged} unchanged</span>
            </span>
          {/if}
        {:else if leftMeta}
          <span class="badge-meta instant-tooltip" data-tooltip={leftMeta.sub}>
            {leftMeta.label}
          </span>
          <span class="text-[var(--color-text-muted)]">{leftMeta.sub}</span>

          <button
            type="button"
            class="btn-secondary btn-sm ml-auto"
            onclick={() => copyContent(leftContent, "snapshot")}
          >
            Copy text
          </button>
        {/if}
      </div>

      <!-- Body -->
      <div class="diff-body min-h-0 flex-1 overflow-auto">
        {#if loading}
          <div class="flex items-center gap-2 px-4 py-6 text-xs text-[var(--color-text-muted)]">
            <Spinner size={14} label="Loading snapshot" />
            Loading snapshot...
          </div>
        {:else if errorMessage}
          <div class="px-4 py-6 text-xs text-[var(--color-error)]">{errorMessage}</div>
        {:else if mode === "view"}
          {#if viewRendered}
            <div
              class="skill-content-preview full-skill-content px-4 py-3 hljs"
              style="background: var(--color-code-bg); color: var(--color-text-primary);"
            >
              {@html viewRendered.html}
            </div>
          {:else}
            <p class="px-4 py-6 text-xs text-[var(--color-text-muted)]">No content available</p>
          {/if}
        {:else if highlightedDiffRows && diff}
          {@const truncated = diff.summary.truncated}
          <table
            class="hljs w-full border-separate text-xs leading-snug"
            style="background: var(--color-code-bg); border-spacing: 0; font-family: var(--font-ui);"
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
                    ? 'var(--color-ok-subtle)'
                    : row.kind === 'remove'
                      ? 'var(--color-error-subtle)'
                      : 'transparent'};"
                >
                  <td
                    class="select-none px-2 text-right text-2xs tabular-nums text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.leftNumber ?? ""}
                  </td>
                  <td
                    class="select-none px-1 text-center"
                    style="border-right: 1px solid var(--color-border); color: {row.kind === 'remove'
                      ? 'var(--color-error)'
                      : 'var(--color-text-muted)'};"
                  >
                    {row.kind === "remove" ? "-" : ""}
                  </td>
                  <td
                    class="diff-code-cell whitespace-pre-wrap break-words px-2"
                    style="border-right: 1px solid var(--color-border);"
                  >{@html row.leftHtml}</td>

                  <td
                    class="select-none px-2 text-right text-2xs tabular-nums text-[var(--color-text-muted)]"
                    style="border-right: 1px solid var(--color-border);"
                  >
                    {row.rightNumber ?? ""}
                  </td>
                  <td
                    class="select-none px-1 text-center"
                    style="border-right: 1px solid var(--color-border); color: {row.kind === 'add'
                      ? 'var(--color-ok)'
                      : 'var(--color-text-muted)'};"
                  >
                    {row.kind === "add" ? "+" : ""}
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
              class="border-t px-4 py-2 text-2xs text-[var(--color-text-muted)]"
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

<style>
  /* Modal entrance (DESIGN 7): shared popover-in recipe (scale 0.98->1 + fade)
     at the modal tier — --motion-enter, emphasized ease. No `both` fill so the
     Svelte out: transition wins on exit; reduced-motion -> opacity-only. */
  .modal-panel-enter {
    animation: popover-in var(--motion-enter) var(--ease-emphasized);
  }
  @media (prefers-reduced-motion: reduce) {
    .modal-panel-enter {
      animation-name: u-fade-in;
    }
  }
</style>
