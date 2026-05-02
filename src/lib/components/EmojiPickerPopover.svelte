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
    onSelect,
    onClose,
  }: {
    open?: boolean;
    skill: Skill;
    onSelect: (emoji: string) => void;
    onClose: () => void;
  } = $props();

  let pickerHost: HTMLDivElement | undefined;
  let picker: Picker | null = null;
  let handleWindowClick: ((e: MouseEvent) => void) | null = null;

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
      emojiButtonSize: 42,
      emojiSize: 23,
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
  });

  onDestroy(() => {
    destroyPicker();
    if (handleWindowClick) {
      window.removeEventListener("click", handleWindowClick);
    }
  });
</script>

{#if open}
  <div
    class="absolute right-0 top-[2.35rem] z-[70] w-[336px] overflow-hidden rounded-2xl border p-2"
    style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-1) 94%, transparent); box-shadow: 0 22px 44px -16px var(--color-overlay-shadow);"
    data-emoji-picker-popover
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
