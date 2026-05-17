<!--
  @agent-context: Inline confirmation popover anchored to a trigger element.

  Used by destructive actions (delete archived snapshot, etc.) that need a
  quick "are you sure" without launching a full modal. Smart positioning:
  - prefers to open above the anchor when there is room
  - flips below when the anchor is near the top of the overlay
  - clamps horizontally within the overlay viewport
  - closes on outside click and on Escape

  Visuals match SkillCard / ContextBar conventions (color-surface tokens,
  border + accent for destructive emphasis).
-->
<script lang="ts">
  type Tone = "danger" | "neutral";

  let {
    open = false,
    anchorRect = null,
    title = "Are you sure?",
    message = "",
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    tone = "danger" as Tone,
    onConfirm = () => {},
    onCancel = () => {},
  }: {
    open?: boolean;
    anchorRect?: DOMRect | null;
    title?: string;
    message?: string;
    confirmLabel?: string;
    cancelLabel?: string;
    tone?: Tone;
    onConfirm?: () => void | Promise<void>;
    onCancel?: () => void;
  } = $props();

  const POPOVER_WIDTH = 240;
  const POPOVER_HEIGHT_ESTIMATE = 112;
  const GAP = 6;
  const SIDE_PADDING = 8;

  let popoverEl: HTMLDivElement | undefined = $state();
  let confirming = $state(false);

  function position(): { left: number; top: number; placement: "above" | "below" } {
    if (!anchorRect) {
      return { left: SIDE_PADDING, top: SIDE_PADDING, placement: "below" };
    }
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    // Default: open ABOVE the anchor (destructive actions usually live inside
    // expanded cards near the bottom, so opening above keeps them in view).
    const spaceAbove = anchorRect.top;
    const spaceBelow = viewportHeight - anchorRect.bottom;
    const placement: "above" | "below" =
      spaceAbove >= POPOVER_HEIGHT_ESTIMATE + GAP || spaceAbove > spaceBelow
        ? "above"
        : "below";

    const top =
      placement === "above"
        ? Math.max(SIDE_PADDING, anchorRect.top - POPOVER_HEIGHT_ESTIMATE - GAP)
        : Math.min(
            viewportHeight - POPOVER_HEIGHT_ESTIMATE - SIDE_PADDING,
            anchorRect.bottom + GAP,
          );

    const anchorCenter = anchorRect.left + anchorRect.width / 2;
    let left = anchorCenter - POPOVER_WIDTH / 2;
    if (left < SIDE_PADDING) left = SIDE_PADDING;
    if (left + POPOVER_WIDTH > viewportWidth - SIDE_PADDING) {
      left = viewportWidth - POPOVER_WIDTH - SIDE_PADDING;
    }
    return { left, top, placement };
  }

  const pos = $derived(open ? position() : { left: 0, top: 0, placement: "below" as const });

  function handleOutsideClick(e: MouseEvent) {
    if (!open) return;
    if (popoverEl && e.target instanceof Node && popoverEl.contains(e.target)) {
      return;
    }
    onCancel();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    } else if (e.key === "Enter") {
      e.preventDefault();
      void runConfirm();
    }
  }

  async function runConfirm() {
    if (confirming) return;
    confirming = true;
    try {
      await onConfirm();
    } finally {
      confirming = false;
    }
  }

  // Listen globally only while open.
  $effect(() => {
    if (!open) return;
    document.addEventListener("mousedown", handleOutsideClick, true);
    document.addEventListener("keydown", handleKeydown, true);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick, true);
      document.removeEventListener("keydown", handleKeydown, true);
    };
  });
</script>

{#if open}
  <div
    bind:this={popoverEl}
    role="dialog"
    aria-modal="false"
    aria-label={title}
    class="fixed z-[220] rounded-md border shadow-lg"
    style="
      left: {pos.left}px;
      top: {pos.top}px;
      width: {POPOVER_WIDTH}px;
      background: var(--color-surface-1);
      border-color: var(--color-border-active);
      box-shadow: 0 10px 28px -10px var(--color-overlay-shadow);
    "
  >
    <div class="px-3 py-2.5">
      <p class="text-[11px] font-semibold text-[var(--color-text-primary)]">{title}</p>
      {#if message}
        <p class="mt-1 text-[10px] leading-relaxed text-[var(--color-text-secondary)]">{message}</p>
      {/if}

      <div class="mt-2.5 flex items-center justify-end gap-1.5">
        <button
          type="button"
          class="rounded-md border px-2 py-1 text-[10px] font-medium transition-colors duration-150
            hover:bg-[var(--color-surface-3)] hover:text-[var(--color-text-primary)]"
          style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
          onclick={onCancel}
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          class="rounded-md border px-2 py-1 text-[10px] font-semibold transition-colors duration-150"
          style="
            border-color: {tone === 'danger' ? 'var(--color-error)' : 'var(--color-border-active)'};
            background: {tone === 'danger' ? 'var(--color-error)' : 'var(--color-accent)'};
            color: white;
          "
          onclick={() => void runConfirm()}
          disabled={confirming}
        >
          {confirming ? "..." : confirmLabel}
        </button>
      </div>
    </div>

    <!-- Pointer notch -->
    {#if anchorRect}
      <div
        class="absolute h-2 w-2 rotate-45 border"
        style="
          left: {Math.max(8, Math.min(POPOVER_WIDTH - 16, anchorRect.left + anchorRect.width / 2 - pos.left - 4))}px;
          {pos.placement === 'above' ? 'bottom: -5px;' : 'top: -5px;'}
          background: var(--color-surface-1);
          border-color: var(--color-border-active);
          {pos.placement === 'above'
            ? 'border-top: none; border-left: none;'
            : 'border-bottom: none; border-right: none;'}
        "
      ></div>
    {/if}
  </div>
{/if}
