<!--
  @agent-context: iOS-style emoji picker popover for skill icons.
  Uses emoji-mart Picker with local dataset and themed styling.
-->
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { Skill } from "$lib/types";
  import data from "@emoji-mart/data";
  import { Picker } from "emoji-mart";

  let {
    open = false,
    skill,
    anchorRect = null,
    onSelect,
    onClose,
  }: {
    open?: boolean;
    skill: Skill;
    anchorRect?: DOMRect | null;
    onSelect: (emoji: string) => void;
    onClose: () => void;
  } = $props();

  let pickerHost: HTMLDivElement | undefined = $state();
  let picker: Picker | null = null;
  let handleWindowClick: ((e: MouseEvent) => void) | null = null;
  let handleEscape: ((e: KeyboardEvent) => void) | null = null;

  const POPOVER_W = 336;
  const POPOVER_H = 430;
  const VIEWPORT_MARGIN = 8;

  const popoverStyle = $derived.by(() => {
    const base = [
      "border-color: var(--color-border)",
      "background: var(--color-surface-1)",
      "box-shadow: 0 22px 44px -16px var(--color-overlay-shadow)",
    ];

    if (!anchorRect) {
      return [
        ...base,
        `left: ${VIEWPORT_MARGIN}px`,
        `top: ${VIEWPORT_MARGIN}px`,
        `width: ${POPOVER_W}px`,
      ].join(";");
    }

    const viewportW = window.innerWidth;
    const viewportH = window.innerHeight;
    const gap = 8;

    let left = anchorRect.right + gap;
    if (left + POPOVER_W > viewportW - VIEWPORT_MARGIN) {
      left = anchorRect.left - POPOVER_W - gap;
    }
    left = Math.max(VIEWPORT_MARGIN, Math.min(left, viewportW - POPOVER_W - VIEWPORT_MARGIN));

    let top = anchorRect.top + (anchorRect.height / 2) - (POPOVER_H / 2);
    if (top + POPOVER_H > viewportH - VIEWPORT_MARGIN) {
      top = viewportH - POPOVER_H - VIEWPORT_MARGIN;
    }
    if (top < VIEWPORT_MARGIN) {
      top = VIEWPORT_MARGIN;
    }

    return [
      ...base,
      `left: ${left}px`,
      `top: ${top}px`,
      `width: ${POPOVER_W}px`,
    ].join(";");
  });

  function getTheme() {
    const isLight = document.documentElement.getAttribute("data-theme") === "light";
    return isLight ? "light" : "dark";
  }

  function mountPicker() {
    if (!pickerHost || picker) return;

    picker = new Picker({
      data,
      theme: getTheme(),
      locale: "en",
      previewPosition: "none",
      skinTonePosition: "none",
      navPosition: "bottom",
      perLine: 8,
      emojiButtonSize: 46,
      emojiButtonRadius: "14px",
      emojiSize: 24,
      icons: "outline",
      searchPosition: "sticky",
      maxFrequentRows: 2,
      onEmojiSelect: (emoji: { native?: string }) => {
        if (emoji.native) {
          onSelect(emoji.native);
        }
      },
    });

    pickerHost.appendChild(picker as unknown as Node);
  }

  function destroyPicker() {
    if (pickerHost) {
      pickerHost.innerHTML = "";
    }
    picker = null;
  }

  $effect(() => {
    if (open) {
      queueMicrotask(() => mountPicker());
    } else {
      destroyPicker();
    }
  });

  onMount(() => {
    handleWindowClick = (e: MouseEvent) => {
      if (!open) return;
      const target = e.target as HTMLElement | null;
      if (!target) return;
      if (target.closest("[data-emoji-picker-popover]") || target.closest("[data-emoji-picker-trigger]")) {
        return;
      }
      onClose();
    };
    window.addEventListener("click", handleWindowClick);

    handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && open) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleEscape);
  });

  onDestroy(() => {
    destroyPicker();
    if (handleWindowClick) {
      window.removeEventListener("click", handleWindowClick);
    }
    if (handleEscape) {
      window.removeEventListener("keydown", handleEscape);
    }
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed z-[130] overflow-hidden rounded-2xl border p-2"
    style={popoverStyle}
    data-emoji-picker-popover
    onclick={(e) => e.stopPropagation()}
  >
    <div class="mb-2 flex items-center justify-between px-1">
      <span class="text-[11px] font-semibold text-[var(--color-text-secondary)]">Choose icon for {skill.name}</span>
      <button
        class="rounded-md px-1.5 py-0.5 text-[10px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]"
        onclick={onClose}
      >
        close
      </button>
    </div>

    <div class="mb-2 flex items-center justify-between gap-1.5 rounded-lg border px-2 py-1.5"
      style="border-color: var(--color-border); background: var(--color-surface-2);">
      <span class="truncate text-[10px] text-[var(--color-text-muted)]">Current</span>
      <span class="text-[18px]">{skill.icon ?? skill.name.charAt(0).toUpperCase()}</span>
      <button
        class="rounded-md px-1.5 py-0.5 text-[10px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]"
        onclick={() => onSelect("")}
      >
        reset
      </button>
    </div>

    <div
      class="emoji-picker-shell overflow-hidden rounded-xl border"
      style="border-color: var(--color-border);"
      bind:this={pickerHost}
    ></div>
  </div>
{/if}

<style>
  .emoji-picker-shell :global(em-emoji-picker) {
    width: 100%;
    height: 336px;
    --background: var(--color-surface-0);
    --border-color: var(--color-border);
    --border-radius: 14px;
    --emoji-size: 1.35rem;
    --emoji-padding: 0.44rem;
    --emoji-button-radius: 14px;
    --button-hover-background: var(--color-surface-3);
    --button-active-background: var(--color-surface-3);
    --input-font-color: var(--color-text-primary);
    --input-placeholder-color: var(--color-text-muted);
    --indicator-color: var(--color-accent);
    --category-font-color: var(--color-text-secondary);
    --outline-color: var(--color-accent-muted);
  }
</style>
