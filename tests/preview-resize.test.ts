/**
 * Preview drag-resize — `BACKLOG.md` **B-07** acceptance.
 *
 * B-07: *"electron-app: absolutely-positioned
 * `.vp-header/.vp-body/.vp-footer` inside `#live-preview` + pointer drag →
 * write normalized rect."* These tests pin the drag math headlessly (no
 * window driving, per the task): drag → normalized rect → resolve on 4:3 AND
 * 16:9 → same relative layout under Fill with different pixels; drags past
 * the slide edge clamp, never invert, never go negative.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_FRAME_RECTS,
  FRAME_IDS,
  GEOMETRY_SPACES,
  MIN_FRAME,
  applyDrag,
  deltaToNormalized,
  frameBoxCss,
  frameFractions,
  getStyleRect,
  getStyleValign,
  handleAnchor,
  pointToNormalized,
  previewFit,
  setStyleRect,
  valignJustify,
  type FrameId,
  type HandleId,
} from "../src/renderer/preview-resize.js";
import {
  resolveRect,
  resolveRectPercent,
  viewport,
} from "../src/core/geometry.js";
import { NORMALIZED_SPACE } from "../src/core/schemas.js";

const close = (a: number, b: number, eps = 1e-6) => Math.abs(a - b) < eps;

// ── Defaults match the vendor's printed values (OBSERVATIONS.md §8) ─────────

test("default frames match Rect::DEFAULT_BODY etc. (0..10000, not 0..1000)", () => {
  assert.equal(NORMALIZED_SPACE, 10_000);
  assert.equal(GEOMETRY_SPACES.geometry, 10_000);
  // OBSERVATIONS.md §8: `Recta… 0;10000;9300`, `0;10000;700`,
  // `0;9300;10000;…` — x;y;w;h in the 0..10000 square.
  assert.deepEqual(DEFAULT_FRAME_RECTS.body, { x: 0, y: 0, w: 10_000, h: 9_300 });
  assert.deepEqual(DEFAULT_FRAME_RECTS.header, { x: 0, y: 0, w: 10_000, h: 700 });
  assert.deepEqual(DEFAULT_FRAME_RECTS.footer, { x: 0, y: 9_300, w: 10_000, h: 700 });
  // Body + footer exactly fill the square; footer starts where the body ends.
  assert.equal(DEFAULT_FRAME_RECTS.body.h + DEFAULT_FRAME_RECTS.footer.h, 10_000);
  assert.equal(DEFAULT_FRAME_RECTS.footer.y, DEFAULT_FRAME_RECTS.body.h);
  // No 1000-space leak: UX-RESEARCH §5.4's old divisor must not appear here.
  for (const f of FRAME_IDS) {
    const r = DEFAULT_FRAME_RECTS[f];
    assert.ok(r.w > 1000 || r.h > 1000, `${f} looks like 0..1000 space: ${JSON.stringify(r)}`);
  }
  assert.equal(GEOMETRY_SPACES.text, 1000);
});

// ── applyDrag: every handle ─────────────────────────────────────────────────

test("edges move one axis each, corners move pairs, move shifts the frame", () => {
  const start = { x: 1000, y: 1000, w: 5000, h: 5000 };
  assert.deepEqual(applyDrag(start, "e", 700, 0), { x: 1000, y: 1000, w: 5700, h: 5000 });
  assert.deepEqual(applyDrag(start, "w", 700, 0), { x: 1700, y: 1000, w: 4300, h: 5000 });
  assert.deepEqual(applyDrag(start, "s", 0, 400), { x: 1000, y: 1000, w: 5000, h: 5400 });
  assert.deepEqual(applyDrag(start, "n", 0, 400), { x: 1000, y: 1400, w: 5000, h: 4600 });
  assert.deepEqual(applyDrag(start, "se", 700, 400), { x: 1000, y: 1000, w: 5700, h: 5400 });
  assert.deepEqual(applyDrag(start, "nw", 700, 400), { x: 1700, y: 1400, w: 4300, h: 4600 });
  assert.deepEqual(applyDrag(start, "ne", 700, 400), { x: 1000, y: 1400, w: 5700, h: 4600 });
  assert.deepEqual(applyDrag(start, "sw", 700, 400), { x: 1700, y: 1000, w: 4300, h: 5400 });
  assert.deepEqual(applyDrag(start, "move", 700, 400), { x: 1700, y: 1400, w: 5000, h: 5000 });
});

test("the shared cross-port example: east +700 on (500,1000,9000,8000)", () => {
  // Mirrored in gpui-app's preview_resize tests — both ports must agree.
  assert.deepEqual(
    applyDrag({ x: 500, y: 1000, w: 9000, h: 8000 }, "e", 700, 0),
    { x: 500, y: 1000, w: 9500, h: 8000 },
  );
});

// ── Clamping: past the edge pins, never inverts, never negative ─────────────

test("a drag past the slide edge clamps instead of overflowing", () => {
  assert.deepEqual(applyDrag({ x: 1000, y: 1000, w: 5000, h: 5000 }, "e", 9000, 0).w, 9000);
  const south = applyDrag({ x: 1000, y: 1000, w: 5000, h: 5000 }, "s", 0, 9000);
  assert.equal(south.y + south.h, 10_000);
  const west = applyDrag({ x: 1000, y: 1000, w: 5000, h: 5000 }, "w", -5000, 0);
  assert.deepEqual(west, { x: 0, y: 1000, w: 6000, h: 5000 });
  const north = applyDrag({ x: 1000, y: 1000, w: 5000, h: 5000 }, "n", 0, -5000);
  assert.deepEqual(north, { x: 1000, y: 0, w: 5000, h: 6000 });
  const moved = applyDrag({ x: 8000, y: 8000, w: 1500, h: 1500 }, "move", 5000, 5000);
  assert.deepEqual(moved, { x: 8500, y: 8500, w: 1500, h: 1500 });
});

test("a drag past the opposite edge pins at MIN_FRAME, never inverts", () => {
  for (const h of ["e", "w", "se", "nw", "ne", "sw"] as HandleId[]) {
    const r = applyDrag({ x: 4000, y: 4000, w: 2000, h: 2000 }, h, h.includes("w") || h === "nw" || h === "sw" ? 9000 : -9000, 0);
    assert.ok(r.w >= MIN_FRAME && r.h >= MIN_FRAME, `${h}: ${JSON.stringify(r)}`);
    assert.ok(r.x >= 0 && r.y >= 0 && r.x + r.w <= 10_000 && r.y + r.h <= 10_000, `${h}: ${JSON.stringify(r)}`);
  }
  // West dragged all the way across: the left edge stops MIN_FRAME from the right.
  const crushed = applyDrag({ x: 4000, y: 4000, w: 2000, h: 2000 }, "w", 9000, 0);
  assert.deepEqual(crushed, { x: 5900, y: 4000, w: MIN_FRAME, h: 2000 });
});

// ── Pointer inversion reuses the model's fit ─────────────────────────────────

test("delta inversion is the exact inverse of the Fill fit", () => {
  const box = viewport(960, 540);
  const fit = previewFit(box, "fill");
  assert.ok(close(fit.sx, 960 / 10_000) && close(fit.sy, 540 / 10_000));
  assert.equal(fit.ox, 0);
  assert.equal(fit.oy, 0);
  // 96 px on a 960 px edge is exactly 1000 units.
  assert.ok(close(deltaToNormalized(96, fit.sx), 1000));
  assert.ok(close(deltaToNormalized(54, fit.sy), 1000));
});

test("letterbox inversion accounts for the bar origin", () => {
  // 16:9 box letterboxed: 1080×1080 square centred in 1920, bars of 420.
  const fit = previewFit(viewport(1920, 1080), "letterbox");
  assert.ok(close(fit.sx, fit.sy));
  assert.ok(close(fit.ox, 420) && close(fit.oy, 0));
  assert.ok(close(pointToNormalized(420, fit.ox, fit.sx), 0));
  assert.ok(close(pointToNormalized(1500, fit.ox, fit.sx), 10_000));
});

test("a degenerate box never divides by zero", () => {
  assert.equal(deltaToNormalized(50, 0), 0);
  assert.equal(pointToNormalized(50, 0, -1), 0);
});

// ── Round trip: drag → rect → resolve on 4:3 AND 16:9 ───────────────────────

test("drag → rect → Fill resolve: same fractions on 4:3 and 16:9, different pixels", () => {
  const dragged = applyDrag(DEFAULT_FRAME_RECTS.body, "e", -700, 0);
  assert.deepEqual(dragged, { x: 0, y: 0, w: 9300, h: 9300 });
  const v43 = viewport(1024, 768);
  const v169 = viewport(1920, 1080);
  const p43 = resolveRectPercent(dragged, v43, "fill");
  const p169 = resolveRectPercent(dragged, v169, "fill");
  for (const k of ["x", "y", "w", "h"] as const) {
    assert.ok(close(p43[k], p169[k], 1e-9), `Fill ${k}: 4:3 ${p43[k]} vs 16:9 ${p169[k]}`);
  }
  const px43 = resolveRect(dragged, v43, "fill");
  const px169 = resolveRect(dragged, v169, "fill");
  assert.notDeepEqual(px43, px169);
  // …and the fractions are exactly what was authored: 93% × 93%.
  assert.ok(close(p169.w, 93) && close(p169.h, 93));
});

test("frameBoxCss carries the resolved fractions (preview ≡ projector)", () => {
  const css = frameBoxCss(DEFAULT_FRAME_RECTS.body, "body");
  assert.match(css, /position:absolute/);
  assert.match(css, /left:0\.000%/);
  assert.match(css, /top:0\.000%/);
  assert.match(css, /width:100\.000%/);
  assert.match(css, /height:93\.000%/);
  const foot = frameBoxCss(DEFAULT_FRAME_RECTS.footer, "footer");
  assert.match(foot, /top:93\.000%/);
  assert.match(foot, /height:7\.000%/);
});

test("handle anchors sit on the frame box", () => {
  const box = frameFractions({ x: 1000, y: 2000, w: 5000, h: 6000 });
  assert.deepEqual(box, { x: 0.1, y: 0.2, w: 0.5, h: 0.6 });
  assert.deepEqual(handleAnchor(box, "nw"), { x: 0.1, y: 0.2 });
  assert.deepEqual(handleAnchor(box, "se"), { x: 0.6, y: 0.8 });
  assert.deepEqual(handleAnchor(box, "e"), { x: 0.6, y: 0.5 });
});

// ── Style read/write through the agenda settings ────────────────────────────

test("getStyleRect falls back to vendor defaults on missing/garbage settings", () => {
  for (const f of FRAME_IDS) {
    assert.deepEqual(getStyleRect(undefined, f), DEFAULT_FRAME_RECTS[f]);
    assert.deepEqual(getStyleRect({}, f), DEFAULT_FRAME_RECTS[f]);
    assert.deepEqual(getStyleRect({ style: null }, f), DEFAULT_FRAME_RECTS[f]);
  }
  assert.deepEqual(
    getStyleRect({ style: { body: { rect: { x: 1, y: 2 } } } }, "body"),
    { x: 1, y: 2, w: 10_000, h: 9_300 },
  );
});

test("setStyleRect writes the Rust Style wire shape and round-trips", () => {
  const agenda: { settings?: Record<string, unknown> } = {};
  setStyleRect(agenda, "body", { x: 500, y: 1000, w: 9000, h: 8000 });
  assert.deepEqual(getStyleRect(agenda.settings, "body"), { x: 500, y: 1000, w: 9000, h: 8000 });
  // Untouched frames still read their defaults.
  assert.deepEqual(getStyleRect(agenda.settings, "header"), DEFAULT_FRAME_RECTS.header);
  // The JSON shape is what Rust's Style deserializes: style.body.rect.
  const wire = JSON.parse(JSON.stringify(agenda.settings)) as {
    style: { level: string; body: { rect: { x: number; y: number; w: number; h: number } } };
  };
  assert.equal(wire.style.level, "Song");
  assert.deepEqual(wire.style.body.rect, { x: 500, y: 1000, w: 9000, h: 8000 });
});

test("setStyleRect clamps garbage instead of storing it", () => {
  const agenda: { settings?: Record<string, unknown> } = {};
  setStyleRect(agenda, "footer", { x: -50, y: 9_300, w: 99_999, h: 700 });
  const back = getStyleRect(agenda.settings, "footer");
  assert.ok(back.x >= 0 && back.w <= 10_000, JSON.stringify(back));
});

test("valign: Rust-authored top/bottom reaches the projected slide, center stays default", () => {
  // Parity with gpui-app/app/src/projector_window.rs body_layout:
  // Top→flex-start, Center→center, Bottom→flex-end. An omitted valign
  // (Rust skips the key on write when center) reads as center.
  assert.equal(getStyleValign(undefined), "center");
  assert.equal(getStyleValign({}), "center");
  assert.equal(getStyleValign({ style: { body: {} } }), "center");
  assert.equal(getStyleValign({ style: { body: { valign: "top" } } }), "top");
  assert.equal(getStyleValign({ style: { body: { valign: "bottom" } } }), "bottom");
  assert.equal(getStyleValign({ style: { body: { valign: "sideways" } } }), "center");
  assert.equal(valignJustify("top"), "flex-start");
  assert.equal(valignJustify("center"), "center");
  assert.equal(valignJustify("bottom"), "flex-end");
  const body = { x: 0, y: 0, w: 10_000, h: 9_300 };
  assert.match(frameBoxCss(body, "body"), /justify-content:center;/);
  assert.match(frameBoxCss(body, "body", valignJustify("top")), /justify-content:flex-start;/);
  assert.match(frameBoxCss(body, "body", valignJustify("bottom")), /justify-content:flex-end;/);
  // Header/footer keep the historical centering.
  assert.match(frameBoxCss(body, "header"), /justify-content:center;/);
});
