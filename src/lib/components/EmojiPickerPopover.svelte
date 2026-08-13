<!--
  @agent-context: iOS-style emoji picker popover for skill icons.
  Singleton — driven by `emojiPicker` store. Rendered ONCE at the Overlay
  root, never per-card, so multi-spawn is impossible.

  Positioning is delegated to the one shared brain, positionPopover() from
  $lib/utils/position.ts, which is viewport-aware and scale-corrected for the
  CSS `zoom` the app uses for font scaling (it derives the BCR coordinate space
  internally, so the panel lands on its trigger at any font scale or scroll
  position). This component only computes the panel SIZE (clamped to the
  viewport) and hands it to the util; the flip/clamp math lives in one place.

  PHOSPHOR floating surface (DESIGN 6/8): .layer-popover chrome, .popover-enter
  entrance, subtle fade on exit. Focus is captured on open, moved into the
  popover, and restored to the trigger on close; Esc closes.
-->
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { fade } from "svelte/transition";
  import data from "@emoji-mart/data";
  import { Picker } from "emoji-mart";
  import {
    closeEmojiPicker,
    emojiPicker,
  } from "$lib/stores/emojiPicker.svelte";
  import { setSkillIcon } from "$lib/stores/skills.svelte";
  import { positionPopover, type Placement } from "$lib/utils/position";

  let popoverEl: HTMLDivElement | undefined = $state();
  let pickerHost: HTMLDivElement | undefined = $state();
  let picker: Picker | null = null;
  let previousFocusEl: HTMLElement | null = null;
  let handleWindowClick: ((e: MouseEvent) => void) | null = null;
  let handleEscape: ((e: KeyboardEvent) => void) | null = null;
  let handleResize: (() => void) | null = null;
  let viewportLogicalW = $state(0);
  let viewportLogicalH = $state(0);

  const POPOVER_W = 336;
  const POPOVER_H = 430;
  const POPOVER_MIN_W = 260;
  const POPOVER_MIN_H = 280;
  const PICKER_CHROME_H = 92;
  const PICKER_MIN_H = 160;
  const VIEWPORT_MARGIN = 8;

  function syncViewport() {
    if (typeof document === "undefined") return;
    const html = document.documentElement;
    // clientWidth/Height are the layout viewport in logical CSS px (post-zoom),
    // which is what the panel-size clamp needs. positionPopover derives its own
    // scale factor for placement.
    viewportLogicalW = html.clientWidth || window.innerWidth;
    viewportLogicalH = html.clientHeight || window.innerHeight;
  }

  const popoverLayout = $derived.by(() => {
    const anchorRect = emojiPicker.anchorRect;
    const vpW = Math.max(viewportLogicalW, 1);
    const vpH = Math.max(viewportLogicalH, 1);
    const width = Math.max(
      POPOVER_MIN_W,
      Math.min(POPOVER_W, vpW - VIEWPORT_MARGIN * 2),
    );
    const height = Math.max(
      POPOVER_MIN_H,
      Math.min(POPOVER_H, vpH - VIEWPORT_MARGIN * 2),
    );
    const pickerHeight = Math.max(PICKER_MIN_H, height - PICKER_CHROME_H);

    if (!anchorRect) {
      return {
        pickerHeight,
        width,
        height,
        x: VIEWPORT_MARGIN,
        y: VIEWPORT_MARGIN,
        placement: "right" as Placement,
      };
    }

    const p = positionPopover({
      anchor: anchorRect,
      panel: { width, height },
      placement: "right",
      align: "center",
      gap: 8,
      margin: VIEWPORT_MARGIN,
    });
    return { pickerHeight, width, height, x: p.x, y: p.y, placement: p.placement };
  });

  function originForPlacement(p: Placement): string {
    switch (p) {
      case "top":
        return "bottom center";
      case "bottom":
        return "top center";
      case "left":
        return "right center";
      case "right":
      default:
        return "left center";
    }
  }

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
      emojiButtonRadius: "8px",
      emojiSize: 24,
      icons: "outline",
      searchPosition: "sticky",
      maxFrequentRows: 2,
      onEmojiSelect: (emoji: { native?: string }) => {
        if (!emoji.native || !emojiPicker.skill) return;
        void selectEmoji(emoji.native);
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

  async function selectEmoji(emoji: string) {
    const target = emojiPicker.skill;
    if (!target) return;
    try {
      await setSkillIcon(target.id, emoji || null);
    } finally {
      closeEmojiPicker();
    }
  }

  async function resetIcon() {
    const target = emojiPicker.skill;
    if (!target) return;
    try {
      await setSkillIcon(target.id, null);
    } finally {
      closeEmojiPicker();
    }
  }

  $effect(() => {
    if (!emojiPicker.open) {
      destroyPicker();
      return;
    }
    syncViewport();
    if (!pickerHost) {
      return;
    }
    mountPicker();
  });

  // Focus management: capture the trigger on open, move focus into the popover
  // so it is keyboard-operable, and restore focus on close.
  $effect(() => {
    if (emojiPicker.open) {
      previousFocusEl =
        document.activeElement instanceof HTMLElement ? document.activeElement : null;
      requestAnimationFrame(() => popoverEl?.focus());
    } else if (previousFocusEl) {
      previousFocusEl.focus();
      previousFocusEl = null;
    }
  });

  onMount(() => {
    syncViewport();

    handleWindowClick = (e: MouseEvent) => {
      if (!emojiPicker.open) return;
      const target = e.target as HTMLElement | null;
      if (!target) return;
      if (
        target.closest("[data-emoji-picker-popover]") ||
        target.closest("[data-emoji-picker-trigger]")
      ) {
        return;
      }
      closeEmojiPicker();
    };
    window.addEventListener("click", handleWindowClick);

    handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape" && emojiPicker.open) {
        closeEmojiPicker();
      }
    };
    window.addEventListener("keydown", handleEscape);

    handleResize = () => {
      syncViewport();
    };
    window.addEventListener("resize", handleResize);
  });

  onDestroy(() => {
    destroyPicker();
    if (handleWindowClick) window.removeEventListener("click", handleWindowClick);
    if (handleEscape) window.removeEventListener("keydown", handleEscape);
    if (handleResize) window.removeEventListener("resize", handleResize);
  });
</script>

{#if emojiPicker.open && emojiPicker.skill}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    bind:this={popoverEl}
    class="layer-popover popover-enter fixed z-[210] overflow-hidden p-2"
    style="left: {popoverLayout.x}px; top: {popoverLayout.y}px; width: {popoverLayout.width}px; height: {popoverLayout.height}px; --popover-origin: {originForPlacement(popoverLayout.placement)};"
    data-emoji-picker-popover
    role="dialog"
    aria-label="Choose icon for {emojiPicker.skill.name}"
    tabindex="-1"
    out:fade={{ duration: 120 }}
    onclick={(e) => e.stopPropagation()}
  >
    <div class="mb-2 flex items-center justify-between gap-2 px-1">
      <span class="truncate text-xs font-semibold text-text-secondary"
        >Choose icon for {emojiPicker.skill.name}</span
      >
      <button class="btn-ghost btn-sm" onclick={closeEmojiPicker}>Close</button>
    </div>

    <div
      class="mb-2 flex items-center justify-between gap-2 rounded-sm border px-2 py-1"
      style="border-color: var(--color-border); background: var(--color-surface-2);"
    >
      <span class="truncate text-2xs text-text-muted">Current</span>
      <span class="text-xl leading-none"
        >{emojiPicker.skill.icon ?? emojiPicker.skill.name.charAt(0).toUpperCase()}</span
      >
      <button class="btn-ghost btn-sm" onclick={resetIcon}>Reset</button>
    </div>

    <div
      class="emoji-picker-shell overflow-hidden rounded-sm border"
      style="border-color: var(--color-border); --emoji-picker-height: {popoverLayout.pickerHeight}px;"
      bind:this={pickerHost}
    ></div>
  </div>
{/if}

<style>
  .emoji-picker-shell :global(em-emoji-picker) {
    width: 100%;
    height: var(--emoji-picker-height, 336px);
    --background: var(--color-surface-0);
    --border-color: var(--color-border);
    --border-radius: var(--radius-sm);
    --emoji-size: 20px;
    --emoji-padding: 8px;
    --emoji-button-radius: var(--radius-md);
    --button-hover-background: var(--color-surface-3);
    --button-active-background: var(--color-surface-3);
    --input-font-color: var(--color-text-primary);
    --input-placeholder-color: var(--color-text-muted);
    --indicator-color: var(--color-accent);
    --category-font-color: var(--color-text-secondary);
    --outline-color: var(--color-accent-muted);
  }
</style>
