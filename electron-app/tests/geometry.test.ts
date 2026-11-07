/**
 * Normalized slide geometry — `BACKLOG.md` **B-06**.
 *
 * Mirrors `gpui-app/vp-core/src/geometry.rs`'s test block one-for-one, so far
 * as two languages allow. Where the Rust half uses exact rational arithmetic
 * the TypeScript half uses the same f64 operations in the same order, and
 * compares within 1e-6. `tests/crossPort.test.ts` pins both against the shared
 * fixture.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  ASPECT_RATIOS,
  aspectRatioFromLabel,
  aspectRatioValue,
  aspectViewport,
  canvasAspect,
  clampRect,
  letterbox,
  matchesAspect,
  normalizeRect,
  pixelRectToPercent,
  rectWithAspect,
  resolveRect,
  resolveRectPercent,
  sizeWithHeight,
  sizeWithWidth,
  textSize,
  viewport,
  viewportAspect,
} from "../src/core/geometry.js";
import { RECT_FULL } from "../src/core/schemas.js";

const close = (a: number, b: number, eps = 1e-6) => Math.abs(a - b) < eps;

test("the space is 10000, not 1000", () => {
  assert.deepEqual(RECT_FULL, { x: 0, y: 0, w: 10_000, h: 10_000 });
});

test("clamping keeps a hand-edited file loadable", () => {
  assert.deepEqual(clampRect({ x: -5, y: -1, w: 20_000, h: 9_999 }), {
    x: 0, y: 0, w: 10_000, h: 9_999,
  });
});

test("a missing dimension is derived, and an empty rect is the full square", () => {
  // Only w, square canvas: h = w.
  assert.deepEqual(rectWithAspect({ x: 0, y: 0, w: 5_000, h: 0 }, 1), {
    x: 0, y: 0, w: 5_000, h: 5_000,
  });
  // Only h under fill on 16:9: w = h * aspect.
  const wide = rectWithAspect({ x: 0, y: 0, w: 0, h: 5_000 }, 16 / 9);
  assert.ok(close(wide.w, 5_000 * (16 / 9), 1), `w was ${wide.w}`);
  // Neither: the full square.
  assert.deepEqual(rectWithAspect({ x: 0, y: 0, w: 0, h: 0 }, 1), RECT_FULL);
  // Both: the authored value wins.
  assert.deepEqual(rectWithAspect({ x: 1, y: 2, w: 3, h: 4 }, 1.78), { x: 1, y: 2, w: 3, h: 4 });
});

test("normalizing shrinks rather than shifting the origin", () => {
  const n = normalizeRect({ x: 9_000, y: 9_000, w: 5_000, h: 5_000 }, 1);
  assert.equal(n.x, 9_000);
  assert.equal(n.y, 9_000);
  assert.equal(n.w, 1_000);
  assert.equal(n.h, 1_000);
});

test("fill reaches every corner and preserves relative layout exactly", () => {
  const body = { x: 500, y: 1_000, w: 9_000, h: 8_000 };
  const at43 = resolveRectPercent(body, viewport(1024, 768), "fill");
  const at169 = resolveRectPercent(body, viewport(1920, 1080), "fill");
  for (const k of ["x", "y", "w", "h"] as const) {
    assert.ok(close(at43[k], at169[k], 1e-4), `fill ${k}: ${at43[k]} vs ${at169[k]}`);
  }
  // The full square under fill is the whole output, no bars.
  assert.deepEqual(resolveRect(RECT_FULL, viewport(1920, 1080), "fill"), {
    x: 0, y: 0, w: 1920, h: 1080,
  });
  assert.deepEqual(letterbox(viewport(1920, 1080), "fill"), {
    left: 0, top: 0, right: 0, bottom: 0,
  });
});

test("letterbox bars the sides on 4:3 and top/bottom on 16:9, undistorted", () => {
  const at43 = resolveRect(RECT_FULL, viewport(1024, 768), "letterbox");
  assert.deepEqual(at43, { x: 128, y: 0, w: 768, h: 768 });
  const at169 = resolveRect(RECT_FULL, viewport(1920, 1080), "letterbox");
  assert.deepEqual(at169, { x: 420, y: 0, w: 1080, h: 1080 });
  // A square rect stays square (w == h in pixels) on both.
  assert.equal(at43.w, at43.h);
  assert.equal(at169.w, at169.h);
  // Portrait: bars top and bottom instead.
  assert.deepEqual(resolveRect(RECT_FULL, viewport(1080, 1920), "letterbox"), {
    x: 0, y: 420, w: 1080, h: 1080,
  });
});

test("the vendor's set is five presets, in ribbon order", () => {
  assert.deepEqual(
    ASPECT_RATIOS.map((a) => a.label),
    ["4x3.2", "4x3", "16x10", "16x9", "16x8"]
  );
  assert.deepEqual(
    ASPECT_RATIOS.map((a) => [a.num, a.den]),
    [[5, 4], [4, 3], [16, 10], [16, 9], [16, 8]]
  );
  assert.equal(aspectRatioFromLabel("16x9"), ASPECT_RATIOS[3]);
  assert.equal(aspectRatioFromLabel("16:9"), ASPECT_RATIOS[3]);
  assert.equal(aspectRatioFromLabel("hd"), ASPECT_RATIOS[3]);
  assert.equal(aspectRatioFromLabel("5x4"), ASPECT_RATIOS[0]);
  assert.equal(aspectRatioFromLabel("nonsense"), undefined);
  const [w, h] = [aspectRatioValue(ASPECT_RATIOS[3]), 0];
  assert.ok(close(w, 16 / 9));
  assert.equal(h, 0);
});

test("size helpers round to whole pixels", () => {
  assert.deepEqual(sizeWithHeight(ASPECT_RATIOS[3], 1080), viewport(1920, 1080));
  assert.deepEqual(sizeWithWidth(ASPECT_RATIOS[3], 1920), viewport(1920, 1080));
  assert.deepEqual(aspectViewport(ASPECT_RATIOS[3], 1920), viewport(1920, 1080));
  assert.deepEqual(aspectViewport(ASPECT_RATIOS[0], 1920), viewport(1920, 1536));
});

test("matchesAspect tolerates a projector that reports odd pixels", () => {
  const hd = ASPECT_RATIOS[3];
  assert.ok(matchesAspect(viewport(1920, 1080), hd, 0));
  assert.ok(matchesAspect(viewport(1919, 1080), hd, 2));
  assert.ok(!matchesAspect(viewport(1024, 768), hd, 2));
  assert.ok(matchesAspect(viewport(1024, 768), ASPECT_RATIOS[1], 0));
  // A degenerate viewport matches nothing, however generous the tolerance.
  assert.ok(!matchesAspect(viewport(1, 1), hd, 4));
  assert.ok(!matchesAspect(viewport(0, 0), hd, 4));
});

test("a zero-sized viewport does not divide by zero", () => {
  const vp = viewport(0, 0);
  assert.deepEqual(vp, viewport(1, 1));
  assert.deepEqual(resolveRect(RECT_FULL, vp, "letterbox"), { x: 0, y: 0, w: 1, h: 1 });
  assert.ok(Number.isFinite(viewportAspect(vp)));
});

test("a degenerate viewport never yields negative bounds", () => {
  for (const [w, h] of [[0, 1080], [1920, 0], [-1, -1], [1, 1]]) {
    const vp = viewport(w, h);
    for (const mode of ["fill", "letterbox"] as const) {
      const p = resolveRect({ x: -100, y: -100, w: 50_000, h: 50_000 }, vp, mode);
      assert.ok(p.x >= 0 && p.y >= 0 && p.w >= 0 && p.h >= 0);
      assert.ok(p.x + p.w <= vp.width && p.y + p.h <= vp.height);
    }
  }
});

test("text size is proportional to the screen, in the 1000 space", () => {
  const vp = viewport(1920, 1080);
  // 70 units is 7% of the mapped edge (changelog 1.26's default).
  assert.ok(close(textSize(70, vp, "letterbox"), 75.6, 0.01), `got ${textSize(70, vp, "letterbox")}`);
  // Doubling the output doubles the glyph size.
  const big = textSize(70, viewport(3840, 2160), "letterbox");
  assert.ok(close(big, textSize(70, vp, "letterbox") * 2, 0.01));
  // Fill measures against the output height.
  assert.ok(close(textSize(70, vp, "fill"), 75.6, 0.01));
});

test("percent rects are stable and report fullness", () => {
  const vp = viewport(1920, 1080);
  const full = pixelRectToPercent(resolveRect(RECT_FULL, vp, "fill"), vp);
  assert.ok(close(full.x, 0) && close(full.y, 0) && close(full.w, 100) && close(full.h, 100));
  // Letterboxed, the square is 56.25% wide and centred.
  const boxed = pixelRectToPercent(resolveRect(RECT_FULL, vp, "letterbox"), vp);
  assert.ok(close(boxed.w, 56.25, 0.01), `w was ${boxed.w}`);
  assert.ok(close(boxed.x, 21.875, 0.01), `x was ${boxed.x}`);
});

test("canvasAspect is 1 under letterbox and the output aspect under fill", () => {
  const vp = viewport(1920, 1080);
  assert.equal(canvasAspect("letterbox", vp), 1);
  assert.ok(close(canvasAspect("fill", vp), 16 / 9));
});
