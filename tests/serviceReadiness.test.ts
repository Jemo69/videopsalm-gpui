// Service-readiness guards.
//
// These are the behaviours a worship team depends on during a service, so
// they are pinned here: the projector must scale the lyrics to the screen
// and never clip them, a pending edit must survive the operator quitting,
// the chosen monitor must be used, a song must go on screen one verse at a
// time, and the Bible must be importable without knowing a file path.
//
// The checks read the sources (like `commandStrip.test.ts`) because the
// behaviour lives in the Electron main/renderer boundary, which has no
// headless test harness of its own; the live behaviour is verified by
// driving the real app over CDP (evidence E-006).

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { fileUrl } from "../src/core/fileUrl";
import { bibleItemText } from "../src/core/agendaItemText";

const ROOT = join(__dirname, "..", "..");
const SRC = join(ROOT, "src");
const rendererSrc = readFileSync(join(SRC, "renderer", "renderer.ts"), "utf8");
const projectorSrc = readFileSync(join(SRC, "renderer", "projector.ts"), "utf8");
const preloadSrc = readFileSync(join(SRC, "preload.ts"), "utf8");
const mainSrc = readFileSync(join(SRC, "main.ts"), "utf8");

test("the projector scales the lyrics to the screen instead of using a fixed size", () => {
  // A fixed pixel size is unreadable on a 4K projector and huge on a small
  // one: the operator's point size is scaled by the projector height.
  assert.match(projectorSrc, /REFERENCE_HEIGHT\s*=\s*1080/);
  assert.match(projectorSrc, /window\.innerHeight\s*\/\s*REFERENCE_HEIGHT/);
});

test("the projector shrinks a verse that does not fit instead of clipping it", () => {
  // The frame clips overflow (`overflow:hidden`), so a long hymn verse would
  // otherwise be cut off at the bottom.
  assert.match(projectorSrc, /bodyOverflows/);
  assert.match(projectorSrc, /size\s*=\s*Math\.max\(floor,\s*size\s*\*\s*0\.94\)/);
  assert.match(projectorSrc, /MIN_SCALE/);
});

test("re-fitting after a resize starts from the operator's size, not the scaled one", () => {
  // Scaling an already-scaled value would shrink the text twice.
  assert.match(projectorSrc, /dataset\["vpAuthored"\]/);
  assert.match(projectorSrc, /addEventListener\("resize"/);
});

test("the projector tells the control window when it had to shrink the lyrics", () => {
  assert.match(projectorSrc, /reportFit/);
  assert.match(preloadSrc, /reportFit/);
  assert.match(preloadSrc, /onFit/);
  assert.match(rendererSrc, /onFit\(/);
  assert.match(rendererSrc, /Lyrics shrunk to/);
});

test("a pending lyric edit is flushed when the operator closes the app", () => {
  // The editor saves on a 300ms debounce. Quitting inside that window used
  // to lose the edit — and the operator's real songbook with it.
  assert.match(rendererSrc, /beforeunload/);
  assert.match(rendererSrc, /saveSongSync/);
  assert.match(preloadSrc, /saveSongSync/);
  assert.match(mainSrc, /vp:save-song-sync/);
  // The write is the same one the debounced path uses.
  assert.match(mainSrc, /function persistSong\(/);
  assert.match(mainSrc, /ipcMain\.handle\("vp:save-song", \(_e, song: Song, edits\?: string\[\]\) => persistSong\(song, edits\)\)/);
});

test("the monitor the operator picks is the one the projector opens on", () => {
  assert.match(rendererSrc, /openProjector\(prefs\.projMonitor\)/);
  assert.match(rendererSrc, /project\([^)]*prefs\.projMonitor\)/);
  assert.match(mainSrc, /projectorMonitor/);
  assert.match(mainSrc, /screen\.getAllDisplays\(\)/);
});

test("a song goes on screen one verse at a time", () => {
  // Double-clicking a song in the agenda used to project the whole song on
  // one screen — a wall of text the congregation cannot read.
  assert.match(rendererSrc, /if \(it\.kind === "Song" && body\)/);
  assert.match(rendererSrc, /slideHtmlFor\(it\.kind, it\.title, slides\[0\]/);
});

test("a Bible translation can be imported without knowing the file path", () => {
  assert.match(preloadSrc, /pickBibleXml/);
  assert.match(mainSrc, /vp:pick-bible-xml/);
  assert.match(rendererSrc, /pickBibleXml\(\)/);
});

test("a plan's scripture reading resolves to the passage, not an empty slide", () => {
  // The agenda item carries only a reference; the text comes from the
  // installed translation. Without this the item projected "(empty slide)".
  // John 3 has verses 1..17; the passage is verses 16 and 17.
  const john3 = Array.from({ length: 15 }, () => "");
  john3.push("For God so loved the world.", "For God did not send his Son.");
  const store = {
    translationId: "KJV",
    books: [{ name: "John", chapters: [[], [], john3] }],
  };
  const ref = {
    translationId: "KJV",
    book: "John",
    chapter: 3,
    verses: [16, 17],
    versification: "EN" as const,
  };
  assert.equal(
    bibleItemText(ref, store),
    "16 For God so loved the world.\n17 For God did not send his Son."
  );
  // No translation for the passage: the plan's own text is used instead.
  assert.equal(bibleItemText(ref, undefined, "16 fallback"), "16 fallback");
});

test("a native service plan can be imported", () => {
  assert.match(mainSrc, /vp:open-videopsalm-agenda/);
  assert.match(preloadSrc, /openVideopsalmAgenda/);
  assert.match(rendererSrc, /openVideopsalmAgenda\(\)/);
  assert.match(rendererSrc, /agenda-open-vp/);
});

test("projecting an agenda item makes the slide panel show that item", () => {
  // The next-slide key steps through the filmstrip, so an item projected
  // without its own slides would step through the previous item's.
  assert.match(
    rendererSrc,
    /async function projectItem\([^)]*\)[^{]*\{[\s\S]{0,400}?if \(filmSourceId !== `item:\$\{it\.id\}`\) \{[\s\S]{0,200}?await refreshFilmstrip\(\);/
  );
});

test("a file URL survives being embedded in an HTML attribute", () => {
  // The projected slide is an HTML string: a raw `url("file:///…")` closed
  // the attribute at the first inner quote, so backgrounds and media never
  // rendered. The URL must not contain a quote or a space.
  const url = fileUrl("/tmp/VP Folder/Images/my bg\"name.png");
  assert.ok(!url.includes('"'), "no raw quote may survive");
  assert.ok(!url.includes(" "), "spaces must be encoded");
  assert.match(url, /^file:\/\/\/tmp\/VP%20Folder\//);
  // A Windows path keeps its drive colon and uses forward slashes.
  const win = fileUrl("C:\\Program Files\\VideoPsalm\\Images\\bg.png");
  assert.match(win, /^file:\/\/C:\/Program%20Files\//);
});

test("the projected slide uses the safe URL helper everywhere a file is embedded", () => {
  assert.match(rendererSrc, /background-image:url\('\$\{fileUrl\(/);
  assert.match(rendererSrc, /<img src="\$\{fileUrl\(/);
  assert.match(rendererSrc, /<video src="\$\{fileUrl\(/);
  assert.match(rendererSrc, /<embed src="\$\{fileUrl\(/);
  // No raw `file://` interpolation may survive in the projected HTML.
  assert.doesNotMatch(rendererSrc, /file:\/\/\$\{/);
});
