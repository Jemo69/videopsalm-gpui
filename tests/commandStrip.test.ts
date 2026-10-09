import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const SRC = join(__dirname, "..", "..", "src", "renderer");
const html = readFileSync(join(SRC, "index.html"), "utf8");
const css = readFileSync(join(SRC, "styles.css"), "utf8");

/**
 * The strip markup, as a list of ids in DOM order.
 *
 * Scans from just after the container's own id so the container is not picked
 * up as one of its own buttons, and matches buttons only.
 */
function stripIds(): string[] {
  const start = html.indexOf('id="tool-strip"');
  const end = html.indexOf("</div>", start);
  const inner = html.slice(start, end).replace(/id="tool-strip"/, "");
  return [...inner.matchAll(/<button id="tool-([a-z-]+)"/g)].map((m) => m[1]);
}

test("the command strip is in the original's order", () => {
  // UX-RESEARCH 3.3: next · base · hide-text · shield · freeze · project,
  // then the pencil separated below a gap.
  assert.deepEqual(stripIds(), [
    "next", "blackout", "text", "shield", "freeze", "project", "edit",
  ]);
});

test("alert and stage-view are not in the command strip", () => {
  const ids = stripIds();
  // They live in the ribbon / projector panel instead.
  assert.ok(!ids.includes("alert"), "alert does not belong in the command strip");
  assert.ok(!ids.includes("stage"), "stage view does not belong in the command strip");
  assert.ok(!ids.includes("slides"), "focus-slide-panel does not belong in the strip");
});

test("the pencil is separated by a visible gap", () => {
  const start = html.indexOf('id="tool-strip"');
  const end = html.indexOf("</div>", start);
  const strip = html.slice(start, end);
  const gapAt = strip.indexOf("tool-gap");
  const pencilAt = strip.indexOf('id="tool-edit"');
  assert.ok(gapAt > 0, "a gap must precede the pencil");
  assert.ok(pencilAt > gapAt, "the pencil renders below the gap");
  assert.match(css, /\.tool-gap\s*\{[^}]*border-bottom/s);
});

test("every command button has a title explaining it", () => {
  for (const id of stripIds()) {
    assert.match(html, new RegExp(`id="tool-${id}"[^>]*title="`), `${id} needs a title`);
  }
});

test("the shield lights RED, not the generic accent", () => {
  assert.match(css, /#tool-strip button#tool-shield\.on\s*\{[^}]*background:\s*#dc2626/s);
  // The generic `.on` style must not be the only rule, or the shield would be
  // indistinguishable from the other states.
  assert.match(css, /#tool-strip button\.on\s*\{/);
});

test("the live slide is styled distinctly from the selected row", () => {
  // The row on screen right now is blue; the keyboard cursor is subtler.
  assert.match(css, /\.slide-block\.live\s*\{[^}]*background/s);
  assert.match(css, /\.slide-block\.selected\s*\{/);
  // And the renderer must set both classes rather than collapsing them.
  const ts = readFileSync(join(SRC, "renderer.ts"), "utf8");
  assert.match(ts, /i === activeFilm \? " live"/);
  assert.match(ts, /i === selectedSlide \? " selected"/);
});