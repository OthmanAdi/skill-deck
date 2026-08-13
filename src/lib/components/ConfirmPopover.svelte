<!--
  @agent-context: Inline confirmation popover anchored to a trigger element.

  Used by destructive actions (delete archived snapshot, etc.) that need a
  quick "are you sure" without launching a full modal.

  PHOSPHOR floating surface (DESIGN 6/8):
  - one shared positioning brain: positionPopover() from $lib/utils/position.ts
    (viewport-aware flip + clamp, scale-corrected for CSS `zoom`). Preferred
    placement is "top" (destructive actions usually live inside expanded cards
    near the bottom, so opening above keeps them in view); it flips below when
    there is no room. Panel height is measured after paint, not guessed.
  - .layer-popover chrome (surface-1, hairline, radius-md, shadow-popover).
  - .popover-enter entrance (140ms scale+fade, reduced-motion aware); a subtle
    fade on exit.
  - focus is trapped between Cancel and Confirm, moves to Cancel (the safe
    default) on open, and restores to the trigger on close. Esc cancels,
    Enter confirms, outside click cancels.
-->
<script lang="ts">
  import { fade } from "svelte/transition";
  import { positionPopover, type Placement } from "$lib/utils/position";

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

  // 240px = 60 grid units. Height starts as an estimate and is corrected to the
  // real rendered height after first paint (message copy is free-form).
  const POPOVER_WIDTH = 240;
  const GAP = 8;
  const MARGIN = 8;

  let popoverEl: HTMLDivElement | undefined = $state();
  let cancelButtonEl: HTMLButtonElement | undefined = $state();
  let confirmButtonEl: HTMLButtonElement | undefined = $state();
  let previousFocusEl: HTMLElement | null = null;
  let confirming = $state(false);
  let measuredHeight = $state(112);

  const pos = $derived.by(() => {
    if (!open || !anchorRect) {
      return { x: MARGIN, y: MARGIN, placement: "bottom" as Placement };
    }
    return positionPopover({
      anchor: anchorRect,
      panel: { width: POPOVER_WIDTH, height: measuredHeight },
      placement: "top",
      align: "center",
      gap: GAP,
      margin: MARGIN,
    });
  });

  // Correct the guessed height to what actually rendered, so the flip/clamp
  // math can never place a taller message off-screen. One reflow, no loop.
  $effect(() => {
    if (!open || !popoverEl) return;
    const h = popoverEl.offsetHeight;
    if (h > 0 && Math.abs(h - measuredHeight) > 1) {
      measuredHeight = h;
    }
  });

  function originForPlacement(p: Placement): string {
    switch (p) {
      case "top":
        return "bottom center";
      case "left":
        return "right center";
      case "right":
        return "left center";
      case "bottom":
      default:
        return "top center";
    }
  }

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
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      void runConfirm();
      return;
    }
    // Two-element focus trap: Tab can never leave the popover while it is open.
    if (e.key === "Tab") {
      const els = [cancelButtonEl, confirmButtonEl].filter(
        (el): el is HTMLButtonElement => !!el,
      );
      if (els.length < 2) return;
      const first = els[0];
      const last = els[els.length - 1];
      const active = document.activeElement as HTMLElement | null;
      if (!active || !els.includes(active as HTMLButtonElement)) {
        e.preventDefault();
        first.focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
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

  // Listen globally only while open; manage focus for a destructive dialog:
  // move focus to Cancel (the safe default) on open and restore it to whatever
  // was focused before — usually the trigger — on close.
  $effect(() => {
    if (!open) return;
    previousFocusEl =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.addEventListener("mousedown", handleOutsideClick, true);
    document.addEventListener("keydown", handleKeydown, true);
    const raf = requestAnimationFrame(() => cancelButtonEl?.focus());
    return () => {
      cancelAnimationFrame(raf);
      document.removeEventListener("mousedown", handleOutsideClick, true);
      document.removeEventListener("keydown", handleKeydown, true);
      previousFocusEl?.focus?.();
      previousFocusEl = null;
    };
  });
</script>

{#if open}
  <div
    bind:this={popoverEl}
    role="dialog"
    aria-modal="false"
    aria-label={title}
    aria-describedby={message ? "confirm-popover-message" : undefined}
    class="layer-popover popover-enter fixed z-[210] w-60"
    style="left: {pos.x}px; top: {pos.y}px; --popover-origin: {originForPlacement(pos.placement)};"
    out:fade={{ duration: 120 }}
  >
    <div class="p-3">
      <p class="text-xs font-semibold text-text-primary">{title}</p>
      {#if message}
        <p
          id="confirm-popover-message"
          class="mt-1 text-2xs leading-relaxed text-text-secondary"
        >
          {message}
        </p>
      {/if}

      <div class="mt-3 flex items-center justify-end gap-2">
        <button
          type="button"
          bind:this={cancelButtonEl}
          class="btn-secondary btn-sm"
          onclick={onCancel}
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          bind:this={confirmButtonEl}
          class="btn-primary btn-sm"
          style={tone === "danger"
            ? "background: var(--color-error); border-color: var(--color-error);"
            : undefined}
          onclick={() => void runConfirm()}
          disabled={confirming}
        >
          {confirming ? "…" : confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}
