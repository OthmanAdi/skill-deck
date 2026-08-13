/*
 * @agent-context: The ONE viewport-aware, scale-aware popover positioning util.
 * Generalizes the three ad-hoc implementations that used to live in components:
 * EmojiPickerPopover (the scale-aware reference), ConfirmPopover (above/below
 * flip + clamp), and ThemeMenu (CSS-absolute). Consumers pass the trigger's
 * getBoundingClientRect(), the panel size, and a preferred placement; they get
 * back logical-coordinate {x, y, placement} to drop straight into inline
 * left/top on a `position: fixed` panel.
 *
 * SCALE: the app uses CSS `zoom` on <html> for font scaling (setFontScale
 * resizes the OS window proportionally). Chromium reports
 * getBoundingClientRect() in visual OR logical space depending on version,
 * while inline left/top is always interpreted in logical (post-zoom) space. We
 * derive `scale = documentElement.getBoundingClientRect().width / clientWidth`
 * (1 when BCR is logical, `zoom` when visual) and divide the anchor by it, so
 * the panel lands on its trigger at any font scale and any scroll position.
 */

export type Placement = "top" | "bottom" | "left" | "right";
export type Align = "start" | "center" | "end";

export interface PopoverPositionOptions {
  /** Trigger rect, as returned by element.getBoundingClientRect(). */
  anchor: DOMRect;
  /** Panel size in logical CSS px. */
  panel: { width: number; height: number };
  /** Preferred side; flips to the opposite (then the roomiest) if it overflows. */
  placement?: Placement;
  /** Cross-axis alignment relative to the anchor. */
  align?: Align;
  /** Gap between anchor edge and panel. */
  gap?: number;
  /** Minimum distance kept from the viewport edges. */
  margin?: number;
}

export interface PopoverPosition {
  x: number;
  y: number;
  placement: Placement;
}

const OPPOSITE: Record<Placement, Placement> = {
  top: "bottom",
  bottom: "top",
  left: "right",
  right: "left",
};

function readViewport(): { vpW: number; vpH: number; scale: number } {
  if (typeof document === "undefined") {
    return { vpW: 0, vpH: 0, scale: 1 };
  }
  const html = document.documentElement;
  const docRect = html.getBoundingClientRect();
  // clientWidth/Height are the layout viewport in logical CSS px (post-zoom).
  const vpW = html.clientWidth || window.innerWidth;
  const vpH = html.clientHeight || window.innerHeight;
  let scale = vpW > 0 ? docRect.width / vpW : 1;
  if (!Number.isFinite(scale) || scale <= 0) scale = 1;
  return { vpW, vpH, scale };
}

const isVertical = (p: Placement): boolean => p === "top" || p === "bottom";
const clamp = (v: number, min: number, max: number): number =>
  Math.max(min, Math.min(v, max));

/**
 * Compute a viewport-clamped, scale-corrected position for a floating panel.
 * The returned x/y are logical coordinates for inline left/top on a fixed panel.
 */
export function positionPopover(opts: PopoverPositionOptions): PopoverPosition {
  const gap = opts.gap ?? 8;
  const margin = opts.margin ?? 8;
  const align: Align = opts.align ?? "center";
  const pw = opts.panel.width;
  const ph = opts.panel.height;
  const { vpW, vpH, scale } = readViewport();

  // No usable viewport (SSR / zero-size doc) — pin to the top-left margin.
  if (vpW <= 0 || vpH <= 0) {
    return { x: margin, y: margin, placement: opts.placement ?? "bottom" };
  }

  // Project the anchor into the logical coord system inline left/top uses.
  const a = {
    left: opts.anchor.left / scale,
    right: opts.anchor.right / scale,
    top: opts.anchor.top / scale,
    bottom: opts.anchor.bottom / scale,
    width: opts.anchor.width / scale,
    height: opts.anchor.height / scale,
  };

  const space: Record<Placement, number> = {
    top: a.top - margin,
    bottom: vpH - a.bottom - margin,
    left: a.left - margin,
    right: vpW - a.right - margin,
  };

  const fits = (p: Placement): boolean =>
    space[p] >= (isVertical(p) ? ph : pw) + gap;

  // Placement: preferred -> opposite -> roomiest side on the same axis.
  let placement: Placement = opts.placement ?? "bottom";
  if (!fits(placement)) {
    const opp = OPPOSITE[placement];
    if (fits(opp)) {
      placement = opp;
    } else {
      const [first, second]: Placement[] = isVertical(placement)
        ? ["bottom", "top"]
        : ["right", "left"];
      placement = space[first] >= space[second] ? first : second;
    }
  }

  const maxX = Math.max(margin, vpW - pw - margin);
  const maxY = Math.max(margin, vpH - ph - margin);
  let x: number;
  let y: number;

  if (isVertical(placement)) {
    y = placement === "bottom" ? a.bottom + gap : a.top - ph - gap;
    if (align === "start") x = a.left;
    else if (align === "end") x = a.right - pw;
    else x = a.left + a.width / 2 - pw / 2;
  } else {
    x = placement === "right" ? a.right + gap : a.left - pw - gap;
    if (align === "start") y = a.top;
    else if (align === "end") y = a.bottom - ph;
    else y = a.top + a.height / 2 - ph / 2;
  }

  return {
    x: clamp(x, margin, maxX),
    y: clamp(y, margin, maxY),
    placement,
  };
}
