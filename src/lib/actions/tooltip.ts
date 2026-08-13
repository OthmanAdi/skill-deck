/**
 * @agent-context: Body-portaled tooltip available in two shapes:
 *
 * 1) Svelte action `use:tooltip={"text" | { text, placement, delayMs }}`
 *    for new callsites where a static prop is convenient.
 *
 * 2) `installGlobalTooltips()` — boot-time event delegation that handles
 *    every existing `[data-tooltip]` element in the overlay. The old CSS
 *    pattern (`.instant-tooltip::after { position: absolute }`) was clipped
 *    by any ancestor with `overflow: hidden`, which is every skill row,
 *    card, modal, and chat shell in this app. The delegated listener
 *    portals the tooltip to `document.body` instead, so it always floats
 *    above the rest of the overlay regardless of how deep the trigger sits.
 *
 * Both paths share the same render + positioning code so behavior is
 * identical whether the markup was annotated with `data-tooltip="…"` or
 * wired with `use:tooltip={…}`.
 */

export type TooltipPlacement = "top" | "bottom" | "left" | "right";

export interface TooltipConfig {
  text: string;
  placement?: TooltipPlacement;
  delayMs?: number;
}

export type TooltipInput = string | TooltipConfig | null | undefined;

interface Resolved {
  text: string;
  placement: TooltipPlacement;
  delayMs: number;
}

function resolve(input: TooltipInput): Resolved | null {
  if (input === null || input === undefined) return null;
  if (typeof input === "string") {
    if (!input.trim()) return null;
    return { text: input, placement: "bottom", delayMs: 80 };
  }
  if (!input.text || !input.text.trim()) return null;
  return {
    text: input.text,
    placement: input.placement ?? "bottom",
    delayMs: input.delayMs ?? 80,
  };
}

const TOOLTIP_CLASS = "skill-deck-tooltip-portal";
// One stable id so the trigger can point aria-describedby at the shared node.
const TOOLTIP_ID = "skill-deck-tooltip-node";
// Single source of truth for the fade duration: the CSS transition AND the JS
// removal timeout both read this, so they can never drift apart (they used to:
// 80ms CSS vs 120ms JS left an invisible node hanging in the DOM).
const TOOLTIP_FADE_MS = 120;

function ensureStylesInjected(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById("skill-deck-tooltip-style")) return;
  const style = document.createElement("style");
  style.id = "skill-deck-tooltip-style";
  // PHOSPHOR floating surface (DESIGN 5-8): surface-3, hairline, radius-xs,
  // --text-2xs, --shadow-popover, one mono voice. Tokens only — no literals.
  style.textContent = `
    .${TOOLTIP_CLASS} {
      position: fixed;
      z-index: 9999;
      pointer-events: none;
      padding: 4px 8px;
      border-radius: var(--radius-xs);
      border: 1px solid var(--color-border);
      background: var(--color-surface-3);
      color: var(--color-text-secondary);
      font-family: var(--font-ui);
      font-size: var(--text-2xs);
      font-weight: 500;
      line-height: 1.3;
      letter-spacing: 0.01em;
      max-width: 280px;
      white-space: normal;
      box-shadow: var(--shadow-popover);
      opacity: 0;
      transform: translateY(-2px);
      transition: opacity ${TOOLTIP_FADE_MS}ms ease-out, transform ${TOOLTIP_FADE_MS}ms ease-out;
    }
    .${TOOLTIP_CLASS}.is-visible {
      opacity: 1;
      transform: translateY(0);
    }
  `;
  document.head.appendChild(style);
}

function place(el: HTMLElement, trigger: HTMLElement, placement: TooltipPlacement): void {
  const rect = trigger.getBoundingClientRect();
  const tipRect = el.getBoundingClientRect();
  const gap = 6;
  let left: number;
  let top: number;
  switch (placement) {
    case "top":
      left = rect.left + rect.width / 2 - tipRect.width / 2;
      top = rect.top - tipRect.height - gap;
      break;
    case "left":
      left = rect.left - tipRect.width - gap;
      top = rect.top + rect.height / 2 - tipRect.height / 2;
      break;
    case "right":
      left = rect.right + gap;
      top = rect.top + rect.height / 2 - tipRect.height / 2;
      break;
    case "bottom":
    default:
      left = rect.left + rect.width / 2 - tipRect.width / 2;
      top = rect.bottom + gap;
      break;
  }
  // Keep tooltip inside the viewport with an 8px gutter so it never escapes
  // the overlay window or hangs off the corner.
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  left = Math.max(8, Math.min(left, vw - tipRect.width - 8));
  top = Math.max(8, Math.min(top, vh - tipRect.height - 8));
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
}

// One floating tooltip element shared across all `[data-tooltip]` triggers.
// Cheaper than spinning a new node per hover and means the fade-in animation
// reads the same way everywhere.
let sharedTip: HTMLDivElement | null = null;
let sharedTimer: ReturnType<typeof setTimeout> | null = null;
let sharedTrigger: HTMLElement | null = null;

function showShared(trigger: HTMLElement, text: string, placement: TooltipPlacement, delayMs: number): void {
  ensureStylesInjected();
  if (sharedTimer) {
    clearTimeout(sharedTimer);
    sharedTimer = null;
  }
  hideShared(true);
  sharedTrigger = trigger;
  sharedTimer = setTimeout(() => {
    sharedTimer = null;
    if (sharedTrigger !== trigger) return;
    const tip = document.createElement("div");
    tip.className = TOOLTIP_CLASS;
    tip.id = TOOLTIP_ID;
    // role="tooltip" + aria-describedby so screen readers get the supplementary
    // text (timestamps, source counts) several badges convey via data-tooltip alone.
    tip.setAttribute("role", "tooltip");
    tip.textContent = text;
    document.body.appendChild(tip);
    sharedTip = tip;
    trigger.setAttribute("aria-describedby", TOOLTIP_ID);
    place(tip, trigger, placement);
    requestAnimationFrame(() => {
      tip.classList.add("is-visible");
    });
  }, delayMs);
}

function hideShared(immediate = false): void {
  if (sharedTimer) {
    clearTimeout(sharedTimer);
    sharedTimer = null;
  }
  // Drop the aria link before we lose the trigger reference.
  if (sharedTrigger) sharedTrigger.removeAttribute("aria-describedby");
  sharedTrigger = null;
  if (!sharedTip) return;
  const tip = sharedTip;
  sharedTip = null;
  if (immediate) {
    tip.remove();
    return;
  }
  tip.classList.remove("is-visible");
  // Match the CSS fade exactly so the node leaves the DOM the instant it's gone.
  window.setTimeout(() => tip.remove(), TOOLTIP_FADE_MS);
}

function syncSharedPosition(): void {
  if (sharedTip && sharedTrigger) {
    const placement = (sharedTrigger.getAttribute("data-tooltip-placement") as TooltipPlacement) || "bottom";
    place(sharedTip, sharedTrigger, placement);
  }
}

/**
 * Boot-time hook: attach event delegation so every existing
 * `[data-tooltip]` trigger gets a body-portaled tooltip on hover / focus.
 * Idempotent — safe to call multiple times.
 */
let globalInstalled = false;
export function installGlobalTooltips(): void {
  if (globalInstalled || typeof document === "undefined") return;
  globalInstalled = true;
  ensureStylesInjected();

  const onPointerEnter = (e: Event) => {
    const target = e.target as HTMLElement | null;
    if (!target) return;
    const trigger = target.closest<HTMLElement>("[data-tooltip]");
    if (!trigger) return;
    const text = trigger.getAttribute("data-tooltip") ?? "";
    if (!text.trim()) return;
    const placement = (trigger.getAttribute("data-tooltip-placement") as TooltipPlacement) || "bottom";
    showShared(trigger, text, placement, 80);
  };

  const onPointerLeave = (e: Event) => {
    const target = e.target as HTMLElement | null;
    if (!target) return;
    const trigger = target.closest<HTMLElement>("[data-tooltip]");
    if (!trigger) return;
    hideShared();
  };

  // Capture phase so we see the event regardless of where it bubbles up to.
  // Use `mouseover`/`mouseout` (which bubble) instead of `mouseenter` (which
  // doesn't) — needed for delegation against `document`.
  document.addEventListener("mouseover", onPointerEnter, true);
  document.addEventListener("mouseout", onPointerLeave, true);
  document.addEventListener("focusin", onPointerEnter, true);
  document.addEventListener("focusout", onPointerLeave, true);
  window.addEventListener("scroll", syncSharedPosition, true);
  window.addEventListener("resize", syncSharedPosition);
}

/**
 * Svelte action variant — preferred for new code.
 */
export function tooltip(node: HTMLElement, input: TooltipInput) {
  ensureStylesInjected();
  let config = resolve(input);

  function onEnter() {
    if (!config) return;
    showShared(node, config.text, config.placement, config.delayMs);
  }

  function onLeave() {
    hideShared();
  }

  node.addEventListener("mouseenter", onEnter);
  node.addEventListener("mouseleave", onLeave);
  node.addEventListener("focusin", onEnter);
  node.addEventListener("focusout", onLeave);

  return {
    update(next: TooltipInput) {
      config = resolve(next);
    },
    destroy() {
      node.removeEventListener("mouseenter", onEnter);
      node.removeEventListener("mouseleave", onLeave);
      node.removeEventListener("focusin", onEnter);
      node.removeEventListener("focusout", onLeave);
      if (sharedTrigger === node) hideShared(true);
    },
  };
}
