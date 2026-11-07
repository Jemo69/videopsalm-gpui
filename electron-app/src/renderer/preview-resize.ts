/**
 * Preview drag-resize — `BACKLOG.md` **B-07**.
 *
 * The geometry MODEL already exists (`core/geometry.ts`: normalized 0..10000
 * space, `resolveRectPercent`). This module is the missing UI: pure drag math
 * plus thin DOM wiring so an operator can drag a frame edge in the slide
 * preview and see it move on the projector.
 *
 * B-07's acceptance criterion, quoted: *"electron-app: absolutely-positioned
 * `.vp-header/.vp-body/.vp-footer` inside `#live-preview` + pointer drag →
 * write normalized rect."* That is exactly what this implements: the slide
 * HTML carries the frames as absolutely-positioned boxes whose geometry comes
 * from `frameBoxCss` (i.e. `resolveRectPercent` under Fill), so the preview
 * and the projector — which renders the same HTML — agree by construction.
 *
 * Handles: **8** (`n/s/e/w/ne/nw/se/sw`) plus move-by-dragging the frame
 * outline. Why 8 and not 4: the vendor's "Size and location" form exposes
 * x/y/w/h as four independent numbers, and edges map onto them one-to-one
 * (west moves x, east moves w, north moves y, south moves h). Corners adjust
 * a pair in one gesture. Four edges alone cannot do a diagonal resize; four
 * corners alone cannot adjust one axis without touching the other.
 *
 * Coordinate spaces (UX-RESEARCH §5.4 AS CORRECTED — the space is 0..10000,
 * not 0..1000; text size lives in a different 0..1000 space and never
 * appears here):
 *
 * * normalized — integers in 0..=10000, what the style stores;
 * * percent — what the slide HTML positions with (`resolveRectPercent`
 *   under Fill, which is viewport-independent by construction);
 * * preview pixels — pointer events, converted back with the INVERSE of the
 *   model's own fit (`previewFit`, same two branches as `geometry.ts`'s
 *   private `fit` — reused semantics, not a second mapping).
 */

import {
  NORMALIZED_SPACE,
  RECT_DEFAULT_BODY,
  TEXT_SIZE_SPACE,
  type Rect,
} from "../core/schemas.js";
import {
  clampRect,
  normalizeRect,
  resolveRectPercent,
  viewport,
  type FitMode,
  type Viewport,
} from "../core/geometry.js";

/** Which style frame is being resized. All three — B-07 names body/header/footer. */
export type FrameId = "body" | "header" | "footer";

export const FRAME_IDS: readonly FrameId[] = ["body", "header", "footer"];

/**
 * Effective default frames, in x;y;w;h order.
 *
 * Body mirrors `RECT_DEFAULT_BODY` from schemas. Header/footer have no
 * exported defaults in `schemas.ts` (frozen — file ownership), so they live
 * here, mirroring Rust `Rect::DEFAULT_HEADER` / `Rect::DEFAULT_FOOTER`
 * exactly. All three match the vendor's printed values (`OBSERVATIONS.md`
 * §8): `Recta… 0;10000;9300` (body: full width, 93% tall, top-anchored),
 * `Recta… 0;10000;700` (header: full width, 7% tall, top), and
 * `Recta… 0;9300;10000;…` (footer: full width, starting where the body ends
 * at 9300, 7% tall — `9300 + 700 = 10000`, the footer fills exactly the band
 * the body leaves).
 */
export const DEFAULT_FRAME_RECTS: Record<FrameId, Rect> = {
  body: { ...RECT_DEFAULT_BODY },
  header: { x: 0, y: 0, w: NORMALIZED_SPACE, h: 700 },
  footer: { x: 0, y: 9_300, w: NORMALIZED_SPACE, h: 700 },
};

/** Draggable affordances: 4 edges + 4 corners + move-by-outline. */
export type HandleId = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw" | "move";

export const HANDLES: readonly HandleId[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

/**
 * Smallest frame a drag can produce, in normalized units (1% of the slide).
 * Drags clamp to this instead of inverting or collapsing: a drag past the
 * opposite edge pins at MIN_FRAME, never a negative or zero size.
 */
export const MIN_FRAME = 100;

/** Sanity export for the §5.4-correction test: geometry is 0..10000, text is 0..1000. */
export const GEOMETRY_SPACES = { geometry: NORMALIZED_SPACE, text: TEXT_SIZE_SPACE } as const;

/**
 * Read the effective rect for one frame out of an agenda's settings.
 *
 * `settings` is `agenda.settings` (`Record<string, unknown> | undefined`):
 * traversed defensively, so a missing style, a missing section, or a
 * non-numeric rect falls back to the vendor default instead of throwing.
 * The result is clamped into the normalized square.
 */
export function getStyleRect(settings: unknown, frame: FrameId): Rect {
  const fallback = DEFAULT_FRAME_RECTS[frame];
  const style = (settings as { style?: unknown } | null)?.style as
    | Record<string, { rect?: unknown }>
    | undefined;
  const raw = style?.[frame]?.rect as Partial<Rect> | undefined;
  const num = (v: unknown, d: number): number =>
    typeof v === "number" && Number.isFinite(v) ? v : d;
  if (!raw) return { ...fallback };
  return clampRect({
    x: num(raw.x, fallback.x),
    y: num(raw.y, fallback.y),
    w: num(raw.w, fallback.w),
    h: num(raw.h, fallback.h),
  });
}

/**
 * Write a normalized rect back into the agenda settings' style.
 *
 * Creates `settings.style` / the frame section when absent (level defaults to
 * `"Song"`, matching the GPUI port's default level). The wire shape
 * `{ style: { body: { rect: { x, y, w, h } } } }` is what Rust's `Style`
 * deserializes (`#[serde(default)]` everywhere), so a dragged frame
 * round-trips across ports through the agenda file.
 */
export function setStyleRect(
  agenda: { settings?: Record<string, unknown> },
  frame: FrameId,
  rect: Rect,
): void {
  const settings = (agenda.settings ??= {});
  const style = ((settings["style"] as Record<string, unknown>) ??= {});
  if (typeof style["level"] !== "string" || !(style["level"] as string).trim()) {
    style["level"] = "Song";
  }
  const section = ((style[frame] as Record<string, unknown>) ??= {});
  section["rect"] = { x: rect.x, y: rect.y, w: rect.w, h: rect.h };
}

/**
 * Apply one drag gesture to a normalized rect.
 *
 * `dx`/`dy` are in normalized units (already converted from pixels by the
 * caller). Starts from the rect's canonical form (`normalizeRect`, the same
 * form every renderer resolves), moves the dragged edges, then clamps: a
 * dragged edge stops `MIN_FRAME` before the OPPOSITE edge, so the frame
 * crushes in place instead of teleporting to the far wall; walls stay at
 * `0..=10000`, sizes stay `>= MIN_FRAME`, nothing inverts, nothing goes
 * negative. Corners adjust both axes; `move` shifts the whole frame and
 * slides it along the walls instead of pushing through them.
 */
export function applyDrag(start: Rect, handle: HandleId, dx: number, dy: number): Rect {
  const s = normalizeRect(start, 1);
  let l = s.x;
  let t = s.y;
  let r = s.x + s.w;
  let b = s.y + s.h;
  const S = NORMALIZED_SPACE;
  if (handle === "move") {
    const w = Math.min(Math.max(Math.round(s.w) || MIN_FRAME, MIN_FRAME), S);
    const h = Math.min(Math.max(Math.round(s.h) || MIN_FRAME, MIN_FRAME), S);
    l = Math.min(Math.max(Math.round(l + dx), 0), S - w);
    t = Math.min(Math.max(Math.round(t + dy), 0), S - h);
    return { x: l, y: t, w, h };
  }
  const movesL = handle === "w" || handle === "nw" || handle === "sw";
  const movesT = handle === "n" || handle === "nw" || handle === "ne";
  if (movesL) l += dx;
  if (handle === "e" || handle === "ne" || handle === "se") r += dx;
  if (movesT) t += dy;
  if (handle === "s" || handle === "sw" || handle === "se") b += dy;
  l = Math.round(l);
  t = Math.round(t);
  r = Math.round(r);
  b = Math.round(b);
  // Opposite-edge pinning: the west edge can advance at most to r - MIN_FRAME
  // (the east edge's CURRENT position, fixed for w/nw/sw drags), never to
  // S - MIN_FRAME. Same on the other three sides.
  l = Math.min(Math.max(l, 0), Math.max(movesL ? r - MIN_FRAME : S - MIN_FRAME, 0));
  t = Math.min(Math.max(t, 0), Math.max(movesT ? b - MIN_FRAME : S - MIN_FRAME, 0));
  r = Math.min(Math.max(r, l + MIN_FRAME), S);
  b = Math.min(Math.max(b, t + MIN_FRAME), S);
  return { x: l, y: t, w: r - l, h: b - t };
}

/**
 * The scale and origin the model maps a viewport with — the same two
 * branches as `geometry.ts`'s private `fit` (`fill`: per-axis fraction of the
 * output's own edge; `letterbox`: uniform scale of the shorter edge,
 * centred). Re-declared here because `fit` is not exported; used ONLY to
 * invert pointer deltas back into normalized units. The forward path
 * (repaint) always goes through `resolveRectPercent`, so the two cannot
 * drift: this function must stay branch-identical to the model's `fit`.
 */
export function previewFit(
  vp: Viewport,
  mode: FitMode,
): { sx: number; sy: number; ox: number; oy: number } {
  if (mode === "letterbox") {
    const scale = Math.min(vp.width, vp.height) / NORMALIZED_SPACE;
    const used = scale * NORMALIZED_SPACE;
    return { sx: scale, sy: scale, ox: (vp.width - used) / 2, oy: (vp.height - used) / 2 };
  }
  return { sx: vp.width / NORMALIZED_SPACE, sy: vp.height / NORMALIZED_SPACE, ox: 0, oy: 0 };
}

/** A pointer delta in pixels → normalized units along one axis. */
export function deltaToNormalized(deltaPx: number, scale: number): number {
  if (!Number.isFinite(scale) || scale <= 0) return 0;
  return deltaPx / scale;
}

/** A pointer position in pixels → normalized units along one axis. */
export function pointToNormalized(posPx: number, originPx: number, scale: number): number {
  if (!Number.isFinite(scale) || scale <= 0) return 0;
  return (posPx - originPx) / scale;
}

function pct(v: number): string {
  return `${v.toFixed(3)}%`;
}

/**
 * Inline CSS positioning for one frame box.
 *
 * Resolved through `resolveRectPercent` under Fill against a dummy viewport:
 * under Fill each axis is a plain fraction of the output's own edge, so the
 * fractions are viewport-independent (asserted in the tests on 4:3 AND 16:9)
 * — the same string lays the frame out identically in the preview and on the
 * projector. `position:absolute` is inline (not a stylesheet rule) so the
 * projected HTML carries its own positioning into the projector window, which
 * shares no other CSS with the control preview. The body's vertical flex
 * follows the style's `valign` (Rust `BodyStyle.valign`: top/center/bottom);
 * header/footer keep the historical centering.
 */
export function frameBoxCss(rect: Rect, _frame: FrameId, justify: string = "center"): string {
  const p = resolveRectPercent(rect, viewport(1000, 1000), "fill");
  return (
    `position:absolute;left:${pct(p.x)};top:${pct(p.y)};` +
    `width:${pct(p.w)};height:${pct(p.h)};overflow:hidden;` +
    `display:flex;flex-direction:column;justify-content:${justify};`
  );
}

/**
 * The style's vertical lyric placement (`StyleBody.valign`) for these
 * settings, defaulting to `center` when absent — mirroring Rust, which
 * skips the key on write when it is center, so an omitted key must read
 * as centered, never as unaligned. Unknown literals also fall back to
 * center rather than breaking projection.
 */
export function getStyleValign(settings: unknown): "top" | "center" | "bottom" {
  const style = (settings as { style?: unknown } | null)?.style as
    | { body?: { valign?: unknown } }
    | undefined;
  const v = style?.body?.valign;
  return v === "top" || v === "bottom" ? v : "center";
}

/** CSS `justify-content` for a style `valign` (column flex): Rust maps
 * Top→Start, Center→Center, Bottom→End in `body_layout`. */
export function valignJustify(valign: "top" | "center" | "bottom"): string {
  return valign === "top" ? "flex-start" : valign === "bottom" ? "flex-end" : "center";
}

/** Fractional (0..1) box of a frame, for handle/outline placement. */
export function frameFractions(rect: Rect): { x: number; y: number; w: number; h: number } {
  const p = resolveRectPercent(rect, viewport(1000, 1000), "fill");
  return { x: p.x / 100, y: p.y / 100, w: p.w / 100, h: p.h / 100 };
}

/** Fractional anchor point of one handle on a frame box. */
export function handleAnchor(
  box: { x: number; y: number; w: number; h: number },
  handle: HandleId,
): { x: number; y: number } {
  const cx = box.x + box.w / 2;
  const cy = box.y + box.h / 2;
  switch (handle) {
    case "n": return { x: cx, y: box.y };
    case "s": return { x: cx, y: box.y + box.h };
    case "e": return { x: box.x + box.w, y: cy };
    case "w": return { x: box.x, y: cy };
    case "ne": return { x: box.x + box.w, y: box.y };
    case "nw": return { x: box.x, y: box.y };
    case "se": return { x: box.x + box.w, y: box.y + box.h };
    case "sw": return { x: box.x, y: box.y + box.h };
    case "move": return { x: cx, y: cy };
  }
}

// ── DOM wiring ──────────────────────────────────────────────────────────────
// Everything below needs `document` and runs only in the browser. The pure
// math above is what the unit tests exercise (headless: no window driving).

/** What the renderer provides; what this module calls back. */
export interface ResizeHooks {
  /** Current effective rect for a frame (reads the live style). */
  getRect: (frame: FrameId) => Rect;
  /** Which frame the handles are on. */
  selected: () => FrameId;
  /**
   * A drag finished: persist the rect, mark dirty, re-project. The renderer
   * owns `agendaDirty`/`paintDirty` — this module never keeps its own dirty
   * flag, it just calls back here.
   */
  commit: (frame: FrameId, rect: Rect) => void;
  /** Fit used to invert pointer deltas. The projection default. */
  mode?: FitMode;
}

export interface ResizeController {
  /** Re-read the style and reposition the overlay (call after re-render). */
  sync: () => void;
  /** Remove overlay + listeners. */
  destroy: () => void;
}

const HANDLE_CURSORS: Record<HandleId, string> = {
  n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize",
  ne: "nesw-resize", sw: "nesw-resize", nw: "nwse-resize", se: "nwse-resize",
  move: "move",
};

/**
 * Wire drag handles into `#live-preview`.
 *
 * The overlay (outline + 8 handles) is appended INSIDE the `.vp-slide`
 * element — never into `.preview-wrap` — so its percentage anchors resolve
 * against the same box the frames do (SCREENSHOT-1.29-LAYOUT.md: the preview
 * pane lives in the RIGHT column below the slides+strip; the handles live in
 * it, not floating elsewhere). A `MutationObserver` re-syncs after every
 * re-render (`pushPreview` replaces `innerHTML`, wiping the overlay).
 * Pointer capture keeps the drag alive under fast moves; the drag converts
 * pixel deltas with the inverse Fill fit and previews live on the overlay,
 * committing once on release.
 */
export function attachPreviewResize(hooks: ResizeHooks): ResizeController {
  const doc = document;
  const preview = doc.getElementById("live-preview");
  const mode: FitMode = hooks.mode ?? "fill";
  let destroyed = false;
  let drag: { handle: HandleId; start: Rect; startX: number; startY: number; boxW: number; boxH: number } | null = null;

  const overlay = doc.createElement("div");
  overlay.className = "vp-rs-overlay";
  overlay.hidden = true;
  const outline = doc.createElement("div");
  outline.className = "vp-rs-outline";
  outline.dataset["h"] = "move";
  outline.title = "Drag to move the frame";
  overlay.appendChild(outline);
  const handleEls = new Map<HandleId, HTMLElement>();
  for (const h of HANDLES) {
    const d = doc.createElement("div");
    d.className = "vp-rs-handle";
    d.dataset["h"] = h;
    d.title = `Drag ${h} edge`;
    d.style.cursor = HANDLE_CURSORS[h];
    overlay.appendChild(d);
    handleEls.set(h, d);
  }

  const slideBox = (): HTMLElement | null =>
    preview?.querySelector<HTMLElement>(":scope > .vp-slide") ?? null;

  function place(): void {
    if (!preview || destroyed) return;
    const slide = slideBox();
    if (!slide) {
      overlay.hidden = true;
      if (overlay.parentElement) overlay.parentElement.removeChild(overlay);
      return;
    }
    if (overlay.parentElement !== slide) slide.appendChild(overlay);
    const frame = hooks.selected();
    const rect = hooks.getRect(frame);
    const f = frameFractions(rect);
    overlay.hidden = false;
    outline.style.left = pct(f.x * 100);
    outline.style.top = pct(f.y * 100);
    outline.style.width = pct(f.w * 100);
    outline.style.height = pct(f.h * 100);
    for (const h of HANDLES) {
      const a = handleAnchor(f, h);
      const d = handleEls.get(h);
      if (!d) continue;
      d.style.left = pct(a.x * 100);
      d.style.top = pct(a.y * 100);
    }
    for (const id of FRAME_IDS) {
      const btn = doc.getElementById(`frame-${id}`);
      if (btn) btn.setAttribute("aria-pressed", id === frame ? "true" : "false");
    }
  }

  function onPointerDown(e: PointerEvent): void {
    if (destroyed || e.button !== 0) return;
    const t = e.target as HTMLElement | null;
    const h = t?.dataset?.["h"] as HandleId | undefined;
    if (!h) return;
    const slide = slideBox();
    if (!slide) return;
    const box = slide.getBoundingClientRect();
    if (box.width < 1 || box.height < 1) return;
    e.preventDefault();
    try {
      (t as Element).setPointerCapture?.(e.pointerId);
    } catch { /* capture unsupported: drag still works while hovered */ }
    drag = {
      handle: h,
      start: hooks.getRect(hooks.selected()),
      startX: e.clientX,
      startY: e.clientY,
      boxW: box.width,
      boxH: box.height,
    };
  }

  function onPointerMove(e: PointerEvent): void {
    if (!drag || destroyed) return;
    const vp = viewport(Math.max(1, Math.round(drag.boxW)), Math.max(1, Math.round(drag.boxH)));
    const fit = previewFit(vp, mode);
    const dx = deltaToNormalized(e.clientX - drag.startX, fit.sx);
    const dy = deltaToNormalized(e.clientY - drag.startY, fit.sy);
    const next = applyDrag(drag.start, drag.handle, dx, dy);
    // Live feedback on the overlay only — no style write, no re-render, no
    // dirty flag until release. Committing per-move would spam the projector
    // bridge and mark the agenda dirty for a drag the operator may cancel.
    const f = frameFractions(next);
    outline.style.left = pct(f.x * 100);
    outline.style.top = pct(f.y * 100);
    outline.style.width = pct(f.w * 100);
    outline.style.height = pct(f.h * 100);
    for (const hh of HANDLES) {
      const a = handleAnchor(f, hh);
      const d = handleEls.get(hh);
      if (!d) continue;
      d.style.left = pct(a.x * 100);
      d.style.top = pct(a.y * 100);
    }
  }

  function onPointerUp(e: PointerEvent): void {
    if (!drag) return;
    const d = drag;
    drag = null;
    // A press with no movement selects the frame; it must not dirty the agenda.
    if (e.clientX === d.startX && e.clientY === d.startY) {
      place();
      return;
    }
    const vp = viewport(Math.max(1, Math.round(d.boxW)), Math.max(1, Math.round(d.boxH)));
    const fit = previewFit(vp, mode);
    const dx = deltaToNormalized(e.clientX - d.startX, fit.sx);
    const dy = deltaToNormalized(e.clientY - d.startY, fit.sy);
    hooks.commit(hooks.selected(), applyDrag(d.start, d.handle, dx, dy));
    place();
  }

  overlay.addEventListener("pointerdown", onPointerDown);
  overlay.addEventListener("pointermove", onPointerMove);
  overlay.addEventListener("pointerup", onPointerUp);
  overlay.addEventListener("pointercancel", () => {
    drag = null;
    place();
  });

  const observer = new MutationObserver(() => place());
  if (preview) observer.observe(preview, { childList: true });
  place();

  return {
    sync: place,
    destroy: () => {
      destroyed = true;
      drag = null;
      observer.disconnect();
      if (overlay.parentElement) overlay.parentElement.removeChild(overlay);
    },
  };
}
