// Presentation keymap tests — the shortcut vocabulary an operator drives the
// whole service from. Mirrors gpui-app/app/src/ux.rs so both targets behave
// alike; `presKeys.ts` is DOM-free precisely so it can be tested headless.
import test from "node:test";
import assert from "node:assert/strict";
import { FILM_DIGITS, SECTION_LETTERS, presentationEndBlocked, routePresKey, shortcutHelp } from "../src/renderer/presKeys";
import type { KeyLike, PresAction } from "../src/renderer/presKeys";

function key(k: string, mods: Partial<KeyLike> = {}): PresAction | null {
  return routePresKey(
    { key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods },
    false,
  );
}
function editing(k: string, mods: Partial<KeyLike> = {}): PresAction | null {
  return routePresKey(
    { key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods },
    true,
  );
}
function mediaKey(k: string, mods: Partial<KeyLike> = {}): PresAction | null {
  // The video/audio player owns focus: F5 must mean play/pause (§1.6).
  return routePresKey(
    { key: k, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods },
    false,
    true,
  );
}

test("arrows, space, Enter and page keys step slides", () => {
  for (const k of [" ", "ArrowRight", "ArrowDown", "PageDown", "Enter"]) {
    assert.equal(key(k), "next", k);
  }
  for (const k of ["Backspace", "ArrowLeft", "ArrowUp", "PageUp"]) {
    assert.equal(key(k), "prev", k);
  }
});

test("shift+arrows move the selection without displaying it", () => {
  assert.equal(key("ArrowRight", { shiftKey: true }), "peekNext");
  assert.equal(key("ArrowDown", { shiftKey: true }), "peekNext");
  assert.equal(key("ArrowLeft", { shiftKey: true }), "peekPrev");
  assert.equal(key("ArrowUp", { shiftKey: true }), "peekPrev");
});

test("punctuation, escape, home/end and +/- follow the original", () => {
  assert.equal(key("."), "toggleBase");
  assert.equal(key(","), "toggleBase");
  assert.equal(key("Escape"), "endPres");
  assert.equal(key("Home"), "songStart");
  assert.equal(key("End"), "songEnd");
  assert.equal(key("+"), "peekItemNext");
  assert.equal(key("-"), "peekItemPrev");
});

test("Ctrl toggles the projector, text, shield and mode", () => {
  assert.equal(key("p", { ctrlKey: true }), "toggleProjector");
  assert.equal(key("t", { ctrlKey: true }), "toggleText");
  assert.equal(key("b", { ctrlKey: true }), "toggleShield");
  assert.equal(key("e", { ctrlKey: true }), "toggleMode");
  assert.equal(key("s", { ctrlKey: true }), "save");
  assert.equal(key("r", { ctrlKey: true }), "toggleFreeze");
});

test("Ctrl+digit adds the Nth search result, not a view switch", () => {
  // Changelog 1.10: "When VideoPsalm finds several songs that match the search
  // criteria, you can add one of those songs by pressing Ctrl+song position."
  //
  // These were bound to Control / Projector / Stage, which the original does
  // not have — so the digits were stolen from the operator mid-search, exactly
  // when the rule matters.
  assert.deepEqual(key("1", { ctrlKey: true }), { addResult: 1 });
  assert.deepEqual(key("2", { ctrlKey: true }), { addResult: 2 });
  assert.deepEqual(key("3", { ctrlKey: true }), { addResult: 3 });
});

test("digits map 1..9 to slides and 0 to slide ten", () => {
  assert.deepEqual(key("1"), { digit: 1 });
  assert.deepEqual(key("9"), { digit: 9 });
  assert.deepEqual(key("0"), { digit: 10 });
  assert.deepEqual(FILM_DIGITS, ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]);
});

test("section letters cover the original's named sections", () => {
  assert.deepEqual(key("b"), { section: "bridge" });
  assert.deepEqual(key("C"), { section: "chorus" });
  assert.deepEqual(key("v"), { section: "verse" });
  assert.deepEqual(key("u"), { section: "outro" });
  for (const [letter, name] of Object.entries(SECTION_LETTERS)) {
    assert.deepEqual(key(letter), { section: name }, letter);
    assert.deepEqual(key(letter.toLowerCase()), { section: name }, letter);
  }
});

test("verse-range keys work with Ctrl and Shift", () => {
  assert.equal(key("*", { ctrlKey: true }), "rangeExtendEnd");
  assert.equal(key("/", { ctrlKey: true }), "rangeShrinkEnd");
  assert.equal(key("*", { shiftKey: true }), "rangeExtendStart");
  assert.equal(key("/", { shiftKey: true }), "rangeShrinkStart");
});

test("typing is never hijacked, but verse-range keys stay live", () => {
  // Ordinary characters and navigation are suppressed while a field has focus.
  assert.equal(editing("b"), null);
  assert.equal(editing("1"), null);
  assert.equal(editing(" "), null);
  assert.equal(editing("ArrowRight"), null);
  assert.equal(editing("Escape"), null);
  // Verse-range adjustment stays available, as in the original.
  assert.equal(editing("*", { ctrlKey: true }), "rangeExtendEnd");
  assert.equal(editing("*", { shiftKey: true }), "rangeExtendStart");
});

test("unrelated keys do not route", () => {
  for (const k of ["q", "z", "!", "F7", "Tab"]) {
    assert.equal(key(k), null, k);
  }
});

test("media and slide-panel keys", () => {
  // F5 is context-dependent (RESEARCH-weebly §1.6): restart-from-beginning
  // on the slide panel, play/pause when the player owns focus (1.27b, R400).
  assert.equal(key("F5"), "startPres");
  assert.equal(mediaKey("F5"), "playPause");
  assert.equal(key("F6"), "focusSlides");
  assert.equal(key("x"), "pptx");
  assert.equal(key("s", { altKey: true }), "focusSongs");
  assert.equal(key("b", { altKey: true }), "focusBibles");
});

test("Ctrl+F1 collapses the ribbon and bare Alt reveals KeyTips", () => {
  // show-or-hide-the-toolbar (§8.4) and 1.5 ("Press Alt to discover them").
  assert.equal(key("F1", { ctrlKey: true }), "toggleRibbon");
  assert.equal(key("Alt"), "keyTips");
  // Bare F1 stays unbound, as before — only the Ctrl chord is new.
  assert.equal(key("F1"), null);
});

test("the single-screen guard blocks ending only with two or more screens", () => {
  // Vendor `*` remark (UX-RESEARCH §7, RESEARCH-weebly §1.1).
  assert.equal(presentationEndBlocked(1), false);
  assert.equal(presentationEndBlocked(2), true);
  assert.equal(presentationEndBlocked(3), true);
});

test("the help table documents every group", () => {
  const rows = shortcutHelp();
  assert.ok(rows.length >= 20);
  const text = rows.map(([k, d]) => `${k} ${d}`).join("\n");
  for (const needle of ["Space", "PgDn", "1…9", "B C I O P N S T U V E", "Escape",
    "Ctrl+P", "Ctrl+T", "Ctrl+B", "Ctrl+E", "Ctrl+R", "Ctrl+F1", "Alt",
    "Shift+→", "+ / -", "Home / End", "F5"]) {
    assert.ok(text.includes(needle), `help table missing ${needle}`);
  }
});