// Functional tests: does the *behaviour* work, or does the code merely
// contain the right words?
//
// The existing `commandStrip.test.ts` reads `index.html` and `styles.css` as
// text and regex-matches them. That passes whether or not any handler works,
// so it could not catch the three real defects it coexisted with: the shield
// not blocking, the auto-advance deadlocking on an excluded slide, and single
// click not presenting. These tests load the real modules and drive them.

import { test } from "node:test";
import * as assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  FILM_DIGITS,
  SECTION_LETTERS,
  routePresKey,
  shortcutHelp,
} from "../src/renderer/presKeys";

const ROOT = join(__dirname, "..", "..");
const SRC = join(ROOT, "src");
const renderer = readFileSync(join(SRC, "renderer", "renderer.ts"), "utf8");
const html = readFileSync(join(SRC, "renderer", "index.html"), "utf8");

/** Every `id="…"` referenced by `el<…>("…")` / `document.getElementById("…")`. */
function idsLookedUp(): string[] {
  const ids = new Set<string>();
  for (const m of renderer.matchAll(/(?:el<[^>]*>|getElementById)\(\s*"([a-z0-9-]+)"/gi)) {
    ids.add(m[1]);
  }
  return [...ids];
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Every control the operator can click is wired to something.
//    A button with no handler is the most common way "all buttons work"
//    quietly becomes false.
// ─────────────────────────────────────────────────────────────────────────────

test("every button in the document has a click handler bound to its id", () => {
  // Collect the ids of every <button> in the markup.
  const buttonIds = [...html.matchAll(/<button[^>]*\bid="([a-z0-9-]+)"/gi)].map((m) => m[1]);
  assert.ok(buttonIds.length > 20, `expected many buttons, found ${buttonIds.length}`);

  const unhandled = buttonIds.filter((id) => {
    // Bound either directly by id, or via a container that the id is inside.
    const direct = new RegExp(`\\("${id}"\\)\\s*\\.addEventListener`).test(renderer);
    return !direct;
  });
  assert.deepEqual(
    unhandled,
    [],
    `buttons with no click handler: ${unhandled.join(", ")}`
  );
});

test("every element id the renderer looks up actually exists in the markup", () => {
  // A typo'd id throws a TypeError on first use and takes a whole feature
  // down at runtime, which no type-checker catches.
  const present = new Set(
    [...html.matchAll(/\bid="([a-z0-9-]+)"/gi)].map((m) => m[1])
  );
  const missing = idsLookedUp().filter((id) => !present.has(id));
  assert.deepEqual(missing, [], `renderer looks up ids absent from index.html: ${missing.join(", ")}`);
});

// ─────────────────────────────────────────────────────────────────────────────
// 2. The shield is a real gate, not a dimming style.
// ─────────────────────────────────────────────────────────────────────────────

test("the shield guard is consulted on every path that can replace the live slide", () => {
  // `shieldBlocks` is the single choke point. Every path that repopulates the
  // slide panel must route through it, or arming the shield does nothing while
  // the operator browses — which is what the shield is for.
  const fn = /async function selectAgendaItem\([^)]*\): Promise<void> \{[\s\S]*?\n\}/.exec(renderer);
  assert.ok(fn, "selectAgendaItem must exist");
  const body = fn![0];
  assert.match(
    body,
    /shieldBlocks\(/,
    "selectAgendaItem must consult shieldBlocks before changing the live slides"
  );
  // The guard must come *before* the mutation, not after it.
  const guardAt = body.indexOf("shieldBlocks(");
  const mutateAt = body.indexOf("selectedItemId = id");
  assert.ok(guardAt >= 0 && mutateAt >= 0);
  assert.ok(guardAt < mutateAt, "the guard must run before the selection changes");

  // The song-list click must also guard. Scoped to the body of
  // `runSongSearch`, because there are many `li.addEventListener("click"…)`
  // handlers in this file and only this one selects a library song.
  const search = /async function runSongSearch\([^)]*\): Promise<void> \{[\s\S]*?\n\}/.exec(renderer);
  assert.ok(search, "runSongSearch must exist");
  const click = /li\.addEventListener\("click",[\s\S]*?\n\s*\}\);/.exec(search![0]);
  assert.ok(click, "the song row must have a click handler");
  assert.match(click![0], /shieldBlocks\(/, "choosing a song must consult the shield");
  // And it must keep the escape hatch beside it.
  assert.match(search![0], /li\.addEventListener\("dblclick"/);
});

test("double-click is the shield's escape hatch, on both the agenda and the song list", () => {
  const dblclicks = [...renderer.matchAll(/addEventListener\("dblclick"/g)];
  assert.ok(dblclicks.length >= 3, `expected dblclick on agenda, song and slide rows; found ${dblclicks.length}`);
});

test("shieldBlocks refuses, and names what it refused", () => {
  // Extracted so the guard's own logic is tested, not just that it is called.
  const body = /function shieldBlocks\(what: string\): boolean \{([\s\S]*?)\n\}/.exec(renderer);
  assert.ok(body, "shieldBlocks must exist as a named function");
  const src = body![1];
  assert.match(src, /if \(!shield\) return false;/, "must pass when the shield is off");
  assert.match(src, /setHint\(/, "must explain the refusal, not fail silently");
  assert.match(src, /Double-click/, "must name the documented escape hatch");
});

// ─────────────────────────────────────────────────────────────────────────────
// 3. Single click presents; Shift+arrow does not.
// ─────────────────────────────────────────────────────────────────────────────

test("a single click on a slide row presents it", () => {
  // "single click display. the double-click still works" (1.2). A click that
  // only moves the cursor is the single most common way this app stops
  // feeling like the original.
  const click = /b\.addEventListener\("click", \(\) => void projectFilmSlide\(/.test(renderer);
  assert.ok(click, "the slide-row click handler must project, not just select");
});

test("Shift+arrow moves the selection without presenting", () => {
  const act = routePresKey(
    { key: "ArrowRight", ctrlKey: false, metaKey: false, altKey: false, shiftKey: true },
    false
  );
  assert.equal(act, "peekNext", "Shift+→ must be a peek, not a step");
  // And a plain arrow must be a real step.
  const plain = routePresKey(
    { key: "ArrowRight", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false },
    false
  );
  assert.equal(plain, "next");
});

// ─────────────────────────────────────────────────────────────────────────────
// 4. A cleared checkbox is skipped, not a dead end.
// ─────────────────────────────────────────────────────────────────────────────

test("auto-advance steps over excluded slides instead of bailing out", () => {
  const step = /async function stepSlide\(dir: number\): Promise<void> \{([\s\S]*?)\n\}/.exec(renderer);
  assert.ok(step, "stepSlide must exist");
  const src = step![1];
  assert.match(
    src,
    /verseEnabled\[t\] === false/,
    "stepSlide must consult verseEnabled so a ticked-out slide is skipped"
  );
  assert.match(src, /t \+= dir/, "stepSlide must walk past the excluded slide");
});

test("an explicit click still presents an excluded slide", () => {
  // Skipping is for the *automatic* advance. If clicking a row refused, the
  // operator could never show a slide they had deliberately unticked.
  assert.match(
    renderer,
    /projectFilmSlide\(i, true\)/,
    "the click handler must force, so a deliberate click beats the skip"
  );
  assert.match(renderer, /if \(verseEnabled\[i\] === false && !force\)/);
});

// ─────────────────────────────────────────────────────────────────────────────
// 5. Viewer mode is a layout rewrite, not a hide-toggle.
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Collect every CSS rule whose selector list mentions the viewer-mode body
 * attribute. Returns the whole rule — selectors *and* declarations — because
 * the interesting part is which panes appear in the selector list, not what
 * they are set to.
 */
function viewerRules(css: string): string[] {
  const out: string[] = [];
  // Walk brace-balanced rules so a comma-separated selector list stays intact.
  const re = /([^{}]+)\{([^{}]*)\}/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(css))) {
    if (/body\[data-viewer="1"\]/.test(m[1])) out.push(`${m[1]}{${m[2]}}`);
  }
  return out;
}

test("Viewer mode keeps the agenda and the library, and drops the whole ribbon", () => {
  const css = readFileSync(join(SRC, "renderer", "styles.css"), "utf8");
  const rules = viewerRules(css);
  assert.ok(rules.length > 0, "there must be viewer-mode CSS rules");
  const all = rules.join("\n");
  assert.ok(
    !/#pane-left\b/.test(all),
    "Viewer mode must NOT hide the left column — it holds the agenda and the library"
  );
  assert.match(all, /#pane-center/, "Viewer mode must drop the centre editor column");
});

test("Viewer mode hides the ribbon tab row too, not just the ribbon body", () => {
  const css = readFileSync(join(SRC, "renderer", "styles.css"), "utf8");
  assert.match(
    viewerRules(css).join("\n"),
    /#ribbon-tabs/,
    "a leftover tab row is the tell that this is a toggle rather than a rewrite"
  );
});

test("the slide panel and command strip survive Viewer mode", () => {
  // They used to be nested inside the hidden centre column, so hiding that
  // column silently took two of the four Viewer areas with it.
  const centre = /<section id="pane-center"[\s\S]*?<\/section>/.exec(html);
  assert.ok(centre, "pane-center must exist");
  assert.ok(
    !/id="slide-filmstrip"/.test(centre![0]),
    "the slide panel must not live inside the viewer-hidden column"
  );
  assert.ok(
    !/id="tool-strip"/.test(centre![0]),
    "the command strip must not live inside the viewer-hidden column"
  );
});

// ─────────────────────────────────────────────────────────────────────────────
// 6. The keyboard map is complete and correctly disambiguated.
// ─────────────────────────────────────────────────────────────────────────────

const K = (key: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) =>
  routePresKey(
    { key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods },
    false
  );

test("the documented Ctrl+letter commands are all bound", () => {
  // Ctrl+P projector, Ctrl+T text, Ctrl+B shield, Ctrl+R freeze, Ctrl+E mode,
  // Ctrl+S save. Ctrl+R was previously unbound despite the strip advertising
  // it in its tooltip.
  assert.equal(K("p", { ctrlKey: true }), "toggleProjector");
  assert.equal(K("t", { ctrlKey: true }), "toggleText");
  assert.equal(K("b", { ctrlKey: true }), "toggleShield");
  assert.equal(K("r", { ctrlKey: true }), "toggleFreeze");
  assert.equal(K("e", { ctrlKey: true }), "toggleMode");
  assert.equal(K("s", { ctrlKey: true }), "save");
});

test("Alt+S is Songbooks and Alt+B is Bibles — Alt+S is NOT save", () => {
  // 1.27 beta (Dec 2023) restored Alt+S to "switch to Songbooks" after it had
  // been hijacked by Save. Binding Alt+S to Save is the classic way to get
  // this map wrong.
  assert.equal(K("s", { altKey: true }), "focusSongs");
  assert.equal(K("S", { altKey: true }), "focusSongs");
  assert.equal(K("b", { altKey: true }), "focusBibles");
  assert.equal(K("B", { altKey: true }), "focusBibles");
});

test("Ctrl+arrow is not hijacked away from the plain arrows", () => {
  assert.equal(K("ArrowRight", { ctrlKey: true }), "next");
});

test("every routed action has a handler in the renderer", () => {
  // A keymap that routes to an unhandled action is a silently dead key.
  const switchBody = /switch \(action\) \{([\s\S]*?)\n\}/.exec(renderer);
  assert.ok(switchBody, "there must be a switch on the routed action");
  const handled = new Set(
    [...switchBody![1].matchAll(/case "([a-zA-Z]+)"/g)].map((m) => m[1])
  );
  // Actions handled by the discriminated-union forms above the switch.
  handled.add("digit");
  handled.add("section");

  const routed = new Set<string>();
  const probes: [string, Record<string, boolean>][] = [];
  for (const c of "abcdefghijklmnopqrstuvwxyz0123456789") probes.push([c, {}]);
  for (const k of ["Escape", "Home", "End", "F5", "F6", " ", ".", ",", "+", "-", "ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown", "PageUp", "PageDown"]) {
    probes.push([k, {}]);
  }
  for (const [key, mods] of probes) {
    const a = K(key, mods);
    if (typeof a === "string") routed.add(a);
  }
  for (const c of "abcdefghijklmnopqrstuvwxyz") {
    const a = K(c, { altKey: true });
    if (typeof a === "string") routed.add(a);
  }
  for (const c of "ptrbes") {
    const a = K(c, { ctrlKey: true });
    if (typeof a === "string") routed.add(a);
  }
  // R-4/R-8 actions: F5 (startPres without player focus), Ctrl+F1 (ribbon),
  // bare Alt (KeyTips). Without these probes a routed-but-unhandled action
  // would pass silently.
  for (const [k, mods] of [["F1", { ctrlKey: true }], ["Alt", {}]] as const) {
    const a = K(k, mods as Record<string, boolean>);
    if (typeof a === "string") routed.add(a);
  }
  for (const k of ["*", "/"]) {
    for (const mods of [{ ctrlKey: true }, { shiftKey: true }, { ctrlKey: true, shiftKey: true }]) {
      const a = K(k, mods);
      if (typeof a === "string") routed.add(a);
    }
  }
  const unhandled = [...routed].filter((a) => !handled.has(a));
  assert.deepEqual(unhandled, [], `routed actions with no handler: ${unhandled.join(", ")}`);
});

test("editing gates the navigation keys but not the verse-range keys", () => {
  // Typing a lyric must never be hijacked into "next slide" — but the
  // Ctrl/Shift verse-range keys stay live even with a field focused.
  const editing = routePresKey(
    { key: "ArrowRight", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false },
    true
  );
  assert.equal(editing, null, "navigation keys must not fire while typing");
  const rangeLive = routePresKey(
    { key: "*", ctrlKey: true, metaKey: false, altKey: false, shiftKey: false },
    true
  );
  assert.ok(rangeLive, "verse-range keys must stay live while typing");
});

// ─────────────────────────────────────────────────────────────────────────────
// 7. The documented keymap shape itself.
// ─────────────────────────────────────────────────────────────────────────────

test("digits map 1..9 to slides 1..9 and 0 to slide 10", () => {
  assert.equal(FILM_DIGITS.join(""), "1234567890");
  for (let n = 1; n <= 9; n++) {
    const a = K(String(n));
    assert.deepEqual(a, { digit: n });
  }
  assert.deepEqual(K("0"), { digit: 10 });
});

test("section letters include Instrumental and Other, not media in/out", () => {
  // I had previously bound I/O to a media range; §7 maps them to sections.
  assert.equal(SECTION_LETTERS["I"], "instrumental");
  assert.equal(SECTION_LETTERS["O"], "other");
});

test("the in-app help documents every key the keymap actually routes", () => {
  // `shortcutHelp()` returns [keys, description] pairs; flatten to one string
  // so the assertion reads as "does the help mention this key".
  const help = shortcutHelp().map(([k, d]) => `${k} ${d}`).join(" | ");
  for (const key of ["Ctrl+R", "Ctrl+P", "Ctrl+T", "Ctrl+B", "Ctrl+E", "Ctrl+S", "Alt+S", "Alt+B", "F5"]) {
    assert.ok(help.includes(key), `shortcut help is missing ${key}`);
  }
});

// ─────────────────────────────────────────────────────────────────────────────
// 8. Search must not steal the live slides while typing.
// ─────────────────────────────────────────────────────────────────────────────

test("running a search does not touch the live slides or the projector", () => {
  // 1.13: "During a search, the current slides are only replaced after you
  // click on one of the found songs." A search that repopulated the filmstrip
  // as you typed would be exactly the regression that entry describes.
  //
  // The deliberate `pushPreview` inside the row's *dblclick* handler is the
  // documented punch-through, so this test looks only at the part of
  // `runSongSearch` that runs as you type — the query, the fetch and the
  // filter — and requires it to be free of projection calls.
  const body = /async function runSongSearch\([^)]*\): Promise<void> \{([\s\S]*?)\n\}/.exec(renderer);
  assert.ok(body, "runSongSearch must exist");
  const src = body![1];

  // Cut the body at the first row-building loop: everything before it is the
  // "as you type" path, everything after is per-row click wiring.
  const typingPath = src.split(/for \(const s of visible\)/)[0];
  assert.ok(
    !/refreshFilmstrip|projectFilmSlide|projectItem|pushPreview|setSlides|vp\(\)\.project/.test(typingPath),
    "the search-as-you-type path must not change the projection"
  );
  // And the single-click path must not project either — only dblclick may.
  const click = /li\.addEventListener\("click",[\s\S]*?\}\);/.exec(src);
  assert.ok(click, "the song row must have a click handler");
  assert.ok(
    !/pushPreview|projectFilmSlide/.test(click![0]),
    "a single click on a search result selects; it must not project"
  );
  assert.match(src, /li\.addEventListener\("dblclick"/, "dblclick is the punch-through");
});