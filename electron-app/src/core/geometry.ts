/**
 * Normalized slide geometry — `BACKLOG.md` **B-06**.
 *
 * The TypeScript mirror of `gpui-app/vp-core/src/geometry.rs` (rule 3 of
 * AGENTS.md: one schema, two ports). The Rust side is canonical; this file
 * must implement the **same arithmetic**, and `tests/crossPort.test.ts` pins
 * both against `tests/fixtures/cross-port-schema.json`.
 *
 * A slide frame is authored in the normalized **0..=10000** square (`Rect`)
 * and mapped to pixels only when a renderer asks for it. The divisor is 10000,
 * not 1000 — the vendor's own style explorer prints `Recta… 0;10000;9300`
 * (`OBSERVATIONS.md` §8).
 *
 * The normalized space is a **square**, and mapping a square onto a non-square
 * output is a genuine choice. `FitMode` is that choice:
 *
 * * `"fill"` — **the default, and what a renderer uses.** Each axis is a
 *   fraction of the output's own edge, so a frame reaches every corner of a
 *   16:9 screen with no bars. Relative layout is preserved exactly.
 * * `"letterbox"` — uniform scale by the output's *shorter* edge, so the
 *   square fits whole and the remainder is bars. Nothing distorts. Relative
 *   layout is preserved relative to the *content square*, not the output.
 *
 * **Text size lives in a different space** (`TEXT_SIZE_SPACE` = 1000, not
 * `NORMALIZED_SPACE`): changelog 1.26's *"default is now 70 (100 was too big)"*
 * only reads correctly if 70 is 7% of the mapped edge — 75.6 px at 1080p.
 */

import {
  NORMALIZED_SPACE,
  TEXT_SIZE_SPACE,
  RECT_FULL,
  type Rect,
} from "./schemas.js";

export { NORMALIZED_SPACE, TEXT_SIZE_SPACE, RECT_FULL };

/** How the normalized square is mapped onto a non-square output. */
export type FitMode = "fill" | "letterbox";

export const FIT_MODES: readonly FitMode[] = ["fill", "letterbox"];

/** A viewport (projector, preview) in pixels. Both edges are at least 1. */
export interface Viewport {
  width: number;
  height: number;
}

/** Build a viewport, clamping both edges to at least 1. */
export function viewport(width: number, height: number): Viewport {
  return {
    width: Math.max(1, Math.floor(width)),
    height: Math.max(1, Math.floor(height)),
  };
}

/** Width ÷ height. */
export function viewportAspect(vp: Viewport): number {
  return vp.width / vp.height;
}

/** A resolved rect in output pixels. Non-negative and inside the viewport. */
export interface PixelRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A resolved rect as percentages of the output (0..=100 per axis). */
export interface PercentRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Clamp every component into the normalized square.
 *
 * Out-of-range values are **clamped, not rejected**: a file dragged in from
 * the vendor, or hand-edited, carries negatives and overshoots, and refusing
 * to open it would be worse than projecting a slightly wrong frame.
 */
export function clampRect(r: Rect): Rect {
  const c = (v: number) => Math.min(Math.max(Math.round(v), 0), NORMALIZED_SPACE);
  return { x: c(r.x), y: c(r.y), w: c(r.w), h: c(r.h) };
}

/**
 * Fill in a missing dimension from the one that is given, keeping the rect's
 * proportions.
 *
 * - both `w` and `h` given → unchanged, the authored value wins
 * - only `w` → `h = w / canvasAspect`
 * - only `h` → `w = h * canvasAspect`
 * - neither → the full square, since a rect with no size is no rect
 *
 * `canvasAspect` is 1.0 under `"letterbox"` (the author works in a square),
 * the output's own aspect under `"fill"`.
 */
export function rectWithAspect(r: Rect, canvasAspect: number): Rect {
  const aspect = Number.isFinite(canvasAspect) && canvasAspect > 0 ? canvasAspect : 1;
  let { w, h } = r;
  if (w <= 0 && h <= 0) {
    w = NORMALIZED_SPACE;
    h = NORMALIZED_SPACE;
  } else if (w <= 0) {
    w = Math.min(Math.max(Math.round(h * aspect), 0), NORMALIZED_SPACE);
  } else if (h <= 0) {
    h = Math.min(Math.max(Math.round(w / aspect), 0), NORMALIZED_SPACE);
  }
  return { x: r.x, y: r.y, w, h };
}

/**
 * Clamp, derive, then shrink so the rect lies wholly inside the normalized
 * square. The canonical form every renderer should use: never a negative edge,
 * never an overflow. Shrinks rather than shifting the origin, so a rect that
 * starts inside the square keeps the position the operator dragged it to.
 */
export function normalizeRect(r: Rect, canvasAspect: number): Rect {
  const c = rectWithAspect(clampRect(r), canvasAspect);
  return {
    x: c.x,
    y: c.y,
    w: Math.min(c.w, NORMALIZED_SPACE - c.x),
    h: Math.min(c.h, NORMALIZED_SPACE - c.y),
  };
}

/** The aspect a normalized rect is authored against under this mode. */
export function canvasAspect(mode: FitMode, vp: Viewport): number {
  return mode === "letterbox" ? 1 : viewportAspect(vp);
}

/**
 * The scale and origin one fit mode applies, before any rounding.
 *
 * Shared by `resolveRect` and `resolveRectPercent` so the two cannot drift.
 */
function fit(vp: Viewport, mode: FitMode): [number, number, number, number] {
  if (mode === "letterbox") {
    const scale = Math.min(vp.width, vp.height) / NORMALIZED_SPACE;
    const used = scale * NORMALIZED_SPACE;
    return [scale, scale, (vp.width - used) / 2, (vp.height - used) / 2];
  }
  return [vp.width / NORMALIZED_SPACE, vp.height / NORMALIZED_SPACE, 0, 0];
}

/**
 * Resolve a normalized rect to pixel bounds for one output.
 *
 * Guarantees: nothing distorts (letterbox uses one scale for both axes);
 * relative layout is preserved (fill: same fraction on any output); the result
 * is inside the output.
 */
export function resolveRect(r: Rect, vp: Viewport, mode: FitMode): PixelRect {
  const n = normalizeRect(r, canvasAspect(mode, vp));
  const [sx, sy, ox, oy] = fit(vp, mode);
  // Round half away from zero, symmetrically, so a rect and its mirror map to
  // pixels the same distance apart.
  const px = (v: number) => Math.floor(v + 0.5);
  const x = Math.min(Math.max(px(n.x * sx + ox), 0), vp.width);
  const y = Math.min(Math.max(px(n.y * sy + oy), 0), vp.height);
  const w = Math.min(Math.max(px(n.w * sx), 0), vp.width - x);
  const h = Math.min(Math.max(px(n.h * sy), 0), vp.height - y);
  return { x, y, w, h };
}

/**
 * Resolve a normalized rect to **percentages of the output**, without ever
 * rounding to whole pixels.
 *
 * The path a renderer that positions in percentages must take. Rounding to
 * integers first and converting back is the defect this closes: `0..=1200` of
 * `0..=10000` is 12% of the mapped edge exactly, but rounding first gives
 * 11.979% on a 4:3 output and 12.037% on a 16:9 one.
 */
export function resolveRectPercent(r: Rect, vp: Viewport, mode: FitMode): PercentRect {
  const n = normalizeRect(r, canvasAspect(mode, vp));
  const [sx, sy, ox, oy] = fit(vp, mode);
  const x = Math.min(Math.max(((n.x * sx + ox) / vp.width) * 100, 0), 100);
  const y = Math.min(Math.max(((n.y * sy + oy) / vp.height) * 100, 0), 100);
  const w = Math.min(Math.max(((n.w * sx) / vp.width) * 100, 0), 100 - x);
  const h = Math.min(Math.max(((n.h * sy) / vp.height) * 100, 0), 100 - y);
  return { x, y, w, h };
}

/**
 * Scale a normalized font size to pixels.
 *
 * Changelog 1.4: *"Font size stored proportionally to the screen size"*.
 * Measured against the 0..=1000 `TEXT_SIZE_SPACE`, so 70 units are 7% of the
 * mapped edge — 75.6 px at 1080p letterboxed.
 */
export function textSize(units: number, vp: Viewport, mode: FitMode): number {
  const base = mode === "letterbox" ? Math.min(vp.width, vp.height) : vp.height;
  return (units * base) / TEXT_SIZE_SPACE;
}

/** The same rect as fractions of the output, for percentage renderers. */
export function pixelRectToPercent(p: PixelRect, vp: Viewport): PercentRect {
  return {
    x: (p.x / vp.width) * 100,
    y: (p.y / vp.height) * 100,
    w: (p.w / vp.width) * 100,
    h: (p.h / vp.height) * 100,
  };
}

// ── Aspect-ratio presets ────────────────────────────────────────────────────
// The vendor's ribbon shows five presets (`resolution-ratio.png`); the leading
// `4x3.2` is a distinct 5:4 preset, not a duplicated `16x9`.

export interface AspectRatio {
  label: string;
  num: number;
  den: number;
}

export const ASPECT_RATIOS: readonly AspectRatio[] = [
  { label: "4x3.2", num: 5, den: 4 },
  { label: "4x3", num: 4, den: 3 },
  { label: "16x10", num: 16, den: 10 },
  { label: "16x9", num: 16, den: 9 },
  { label: "16x8", num: 16, den: 8 },
];

/** Look up by vendor label, accepting the aliases a UI is likely to use. */
export function aspectRatioFromLabel(label: string): AspectRatio | undefined {
  const key = label.trim().toLowerCase().replace(/[ :]/g, "x");
  const hit: Record<string, number> = {
    "4x3.2": 0,
    "5x4": 0,
    "4x3": 1,
    "16x10": 2,
    "16x9": 3,
    hd: 3,
    "16x8": 4,
    "2x1": 4,
  };
  const i = hit[key];
  return i === undefined ? undefined : ASPECT_RATIOS[i];
}

/** Width ÷ height. */
export function aspectRatioValue(r: AspectRatio): number {
  return r.num / r.den;
}

/** The output size for this ratio at a given height, rounded. */
export function sizeWithHeight(r: AspectRatio, height: number): Viewport {
  const h = Math.max(1, Math.round(height));
  return viewport(Math.max(1, Math.round((h * r.num) / r.den)), h);
}

/** The output size for this ratio at a given width, rounded. */
export function sizeWithWidth(r: AspectRatio, width: number): Viewport {
  const w = Math.max(1, Math.round(width));
  return viewport(w, Math.max(1, Math.round((w * r.den) / r.num)));
}

/**
 * A canonical viewport for this preset, sized from `base`, interpreted as the
 * long edge (every preset here is landscape).
 */
export function aspectViewport(r: AspectRatio, base: number): Viewport {
  const b = Math.max(1, Math.round(base));
  return r.num >= r.den ? sizeWithWidth(r, b) : sizeWithHeight(r, b);
}

/**
 * `true` when this viewport's ratio matches `r`.
 *
 * The error is measured as a **fraction of the longer edge**, and any viewport
 * whose shorter edge is not larger than the tolerance matches nothing — an
 * absolute pixel comparison lets a degenerate output claim a projector's
 * ratio (a 1×1 viewport answered "yes, 16:9" for a tolerance of 4).
 */
export function matchesAspect(vp: Viewport, r: AspectRatio, tolerance: number): boolean {
  if (vp.width < 1 || vp.height < 1) return false;
  const tol = Math.max(0, Math.floor(tolerance));
  if (Math.min(vp.width, vp.height) <= Math.max(tol, 1)) return false;
  const rel = tol / Math.max(vp.width, vp.height);
  const want = aspectRatioValue(r);
  return Math.abs(viewportAspect(vp) - want) / want <= rel;
}

/** The empty margin a fit leaves around the mapped square, in pixels. */
export interface Letterbox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function letterbox(vp: Viewport, mode: FitMode): Letterbox {
  const s = resolveRect(RECT_FULL, vp, mode);
  return {
    left: s.x,
    top: s.y,
    right: vp.width - (s.x + s.w),
    bottom: vp.height - (s.y + s.h),
  };
}
