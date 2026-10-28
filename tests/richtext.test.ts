// Markdown-lite subset for lyric/slide notes: markers, prefix toggles,
// escaping contract (no raw HTML — the projector renders generated HTML,
// so song files must not inject it), and chord coexistence.
import test from "node:test";
import assert from "node:assert/strict";

import { prefixLines, renderRichLine, wrapSelection } from "../src/core/richtext";

test("inline markers render, chords untouched", () => {
  assert.equal(renderRichLine("**bold**"), '<span class="rl-b">bold</span>');
  assert.equal(renderRichLine("*italic*"), '<span class="rl-i">italic</span>');
  assert.equal(renderRichLine("_italic_"), '<span class="rl-i">italic</span>');
  assert.equal(renderRichLine("__underline__"), '<span class="rl-u">underline</span>');
  assert.equal(renderRichLine("~~gone~~"), '<span class="rl-s">gone</span>');
  // Chord brackets are not formatting.
  assert.equal(renderRichLine("[C]"), "[C]");
  assert.equal(renderRichLine("[Am7]"), "[Am7]");
  // Markers combine left to right.
  assert.equal(
    renderRichLine("**bold** and *italic*"),
    '<span class="rl-b">bold</span> and <span class="rl-i">italic</span>'
  );
});

test("block prefixes render as spans, not layout elements", () => {
  assert.equal(renderRichLine("# Title"), '<span class="rl-h1">Title</span>');
  assert.equal(renderRichLine("## Smaller"), '<span class="rl-h2">Smaller</span>');
  assert.equal(renderRichLine("- point"), '<span class="rl-li">point</span>');
  assert.equal(renderRichLine("3. third"), '<span class="rl-li num">3. third</span>');
  assert.equal(renderRichLine("plain"), "plain");
  // A lone # is text, not a heading.
  assert.equal(renderRichLine("#"), "#");
});

test("already-escaped input stays escaped (no raw HTML)", () => {
  assert.equal(
    renderRichLine("**&lt;b&gt;**"),
    '<span class="rl-b">&lt;b&gt;</span>'
  );
  assert.equal(
    renderRichLine("&lt;script&gt;alert(1)&lt;/script&gt;"),
    "&lt;script&gt;alert(1)&lt;/script&gt;"
  );
});

test("unmatched markers are left as text", () => {
  assert.equal(renderRichLine("**open"), "**open");
  assert.equal(renderRichLine("a * b"), "a * b");
  assert.equal(renderRichLine("2 + 2"), "2 + 2");
});

test("wrapSelection wraps and keeps the inner text selected", () => {
  const r = wrapSelection("hello world", 6, 11, "**", "**");
  assert.equal(r.text, "hello **world**");
  assert.deepEqual([r.cursorStart, r.cursorEnd], [8, 13]);
  const empty = wrapSelection("hi", 1, 1, "*", "*");
  assert.equal(empty.text, "h**i");
  assert.deepEqual([empty.cursorStart, empty.cursorEnd], [2, 2]);
});

test("prefixLines toggles whole intersecting lines", () => {
  // Collapsed cursor in "two" toggles just that line.
  const one = prefixLines("one\ntwo\nthree", 4, 4, "## ");
  assert.equal(one.text, "one\n## two\nthree");
  // A range covering all three lines marks them all, and toggling the
  // uniform result restores the original.
  const t = prefixLines("one\ntwo\nthree", 0, 14, "## ");
  assert.equal(t.text, "## one\n## two\n## three");
  const back = prefixLines(t.text, 0, t.text.length, "## ");
  assert.equal(back.text, "one\ntwo\nthree");
  // A marker inside a chord line is still just text.
  assert.equal(renderRichLine("la [C] la"), "la [C] la");
});
