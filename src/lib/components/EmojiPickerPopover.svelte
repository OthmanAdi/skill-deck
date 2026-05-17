<!--
  @agent-context: iOS-style emoji picker popover for skill icons.
  Singleton — driven by `emojiPicker` store. Rendered ONCE at the Overlay
  root, never per-card, so multi-spawn is impossible.

  Positioning detects the BCR coord space at runtime: with CSS `zoom` on
  <html>, Chromium can report `getBoundingClientRect()` in either visual
  or logical space depending on version. We compute
    scale = documentElement.getBoundingClientRect().width / clientWidth
  which is 1 when BCR is logical and `zoom` when BCR is visual. Dividing
  the anchor rect by `scale` and using `clientWidth/clientHeight` as the
  viewport bound puts every input into the same logical coordinate system
  that inline left/top is interpreted in — so the popover lands on the
  trigger regardless of font scale or where in the list the trigger sits.
-->
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import data from "@emoji-mart/data";
  import { Picker } from "emoji-mart";
  import {
    closeEmojiPicker,
    emojiPicker,
  } from "$lib/stores/emojiPicker.svelte";
  import { setSkillIcon } from "$lib/stores/skills.svelte";

  let pickerHost: HTMLDivElement | undefined = $state();
  let picker: Picker | null = null;
  let handleWindowClick: ((e: MouseEvent) => void) | null = null;
  let handleEscape: ((e: KeyboardEvent) => void) | null = null;
  let handleResize: (() => void) | null = null;
  let viewportLogicalW = $state(0);
  let viewportLogicalH = $state(0);
  let bcrScale = $state(1);

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
    // clientWidth/Height are the layout viewport in logical CSS px (post-zoom).
    // BCR-space scale tells us whether BCR is in logical (scale=1) or
    // visual (scale=zoom) coords.
    const docRect = html.getBoundingClientRect();
    viewportLogicalW = html.clientWidth || window.innerWidth;
    viewportLogicalH = html.clientHeight || window.innerHeight;
    bcrScale = viewportLogicalW > 0 ? docRect.width / viewportLogicalW : 1;
    if (!Number.isFinite(bcrScale) || bcrScale <= 0) bcrScale = 1;
  }

  const popoverLayout = $derived.by(() => {
    const anchorRect = emojiPicker.anchorRect;
    const scale = bcrScale > 0 ? bcrScale : 1;
    const vpW = Math.max(viewportLogicalW, 1);
    const vpH = Math.max(viewportLogicalH, 1);
    const width = Math.max(
      POPOVER_MIN_W,
      Math.min(POPOVER_W, vpW - VIEWPORT_MARGIN * 2)
    );
    const height = Math.max(
      POPOVER_MIN_H,
      Math.min(POPOVER_H, vpH - VIEWPORT_MARGIN * 2)
    );
    const pickerHeight = Math.max(PICKER_MIN_H, height - PICKER_CHROME_H);

    const base = [
      "border-color: var(--color-border)",
      "background: var(--color-surface-1)",
      "box-shadow: 0 22px 44px -16px var(--color-overlay-shadow)",
      `height: ${height}px`,
      `width: ${width}px`,
    ];

    if (!anchorRect) {
      return {
        pickerHeight,
        style: [
          ...base,
          `left: ${VIEWPORT_MARGIN}px`,
          `top: ${VIEWPORT_MARGIN}px`,
        ].join(";"),
      };
    }

    // Project anchor rect into the same coord system inline left/top uses.
    const anchorLeft = anchorRect.left / scale;
    const anchorRight = anchorRect.right / scale;
    const anchorTop = anchorRect.top / scale;
    const anchorBottom = anchorRect.bottom / scale;
    const anchorWidth = anchorRect.width / scale;
    const anchorHeight = anchorRect.height / scale;

    const gap = 8;

    // Horizontal placement: prefer right of anchor, fall back to left, then clamp.
    let left = anchorRight + gap;
    if (left + width > vpW - VIEWPORT_MARGIN) {
      left = anchorLeft - width - gap;
    }
    left = Math.max(VIEWPORT_MARGIN, Math.min(left, vpW - width - VIEWPORT_MARGIN));

    // Vertical placement: try to vertically center on the anchor; if it
    // would overflow either edge, flip to align with the anchor's other
    // side. Bottom-of-list anchors used to disappear because the popover
    // was simply clamped to viewportH - height with no awareness of WHERE
    // the anchor is. Now we pin to the visible edge that the anchor is
    // closest to, so the popover stays attached.
    const spaceBelow = vpH - anchorBottom;
    const spaceAbove = anchorTop;

    let top: number;
    if (height + VIEWPORT_MARGIN * 2 <= vpH) {
      // Plenty of room — center vertically on anchor first.
      top = anchorTop + anchorHeight / 2 - height / 2;
      // If centering pushes off the bottom, place above the anchor.
      if (top + height > vpH - VIEWPORT_MARGIN) {
        if (spaceAbove >= height + VIEWPORT_MARGIN) {
          top = anchorTop - height - gap;
        } else if (spaceBelow >= height + VIEWPORT_MARGIN) {
          top = anchorBottom + gap;
        } else {
          // Neither side fits cleanly — clamp to the side closer to anchor.
          top = spaceBelow >= spaceAbove
            ? Math.max(VIEWPORT_MARGIN, anchorBottom + gap)
            : Math.max(VIEWPORT_MARGIN, anchorTop - height - gap);
        }
      }
      // If centering pushed off the top, drop below.
      if (top < VIEWPORT_MARGIN) {
        top = spaceBelow >= height + VIEWPORT_MARGIN ? anchorBottom + gap : VIEWPORT_MARGIN;
      }
      // Final clamp into viewport.
      top = Math.max(VIEWPORT_MARGIN, Math.min(top, vpH - height - VIEWPORT_MARGIN));
    } else {
      // Popover is taller than the viewport — pin to top margin.
      top = VIEWPORT_MARGIN;
    }

    return {
      pickerHeight,
      style: [
        ...base,
        `left: ${left}px`,
        `top: ${top}px`,
      ].join(";"),
    };
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
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed z-[210] overflow-hidden rounded-2xl border p-2"
    style={popoverLayout.style}
    data-emoji-picker-popover
    onclick={(e) => e.stopPropagation()}
  >
    <div class="mb-2 flex items-center justify-between px-1">
      <span class="text-[11px] font-semibold text-[var(--color-text-secondary)]"
        >Choose icon for {emojiPicker.skill.name}</span
      >
      <button
        class="rounded-md px-1.5 py-0.5 text-[10px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]"
        onclick={closeEmojiPicker}
      >
        close
      </button>
    </div>

    <div
      class="mb-2 flex items-center justify-between gap-1.5 rounded-lg border px-2 py-1.5"
      style="border-color: var(--color-border); background: var(--color-surface-2);"
    >
      <span class="truncate text-[10px] text-[var(--color-text-muted)]">Current</span>
      <span class="text-[18px]"
        >{emojiPicker.skill.icon ?? emojiPicker.skill.name.charAt(0).toUpperCase()}</span
      >
      <button
        class="rounded-md px-1.5 py-0.5 text-[10px] text-[var(--color-text-muted)] hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-secondary)]"
        onclick={resetIcon}
      >
        reset
      </button>
    </div>

    <div
      class="emoji-picker-shell overflow-hidden rounded-xl border"
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
