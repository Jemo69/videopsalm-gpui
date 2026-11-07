/**
 * The style cascade — **definition-of-done #6**.
 *
 * These mirror `gpui-app/vp-core/src/cascade.rs`'s test block one-for-one. Rule
 * 3 of AGENTS.md: one schema, two ports, and the two must actually agree — the
 * last two tests here pin the pieces `tests/crossPort.test.ts` compares against
 * the Rust side.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  ATTRIBUTES,
  CHAINS,
  LEVELS,
  Cascade,
  Chain,
  Level,
  LevelValues,
  attributeSet,
  chainContains,
  chainDepth,
  chainLevels,
  emptyCascade,
  explorerRows,
  formatValue,
  getLevel,
  isInheritedFrom,
  isSetHere,
  levelIsEmpty,
  resetLevel,
  resolve,
  resolveAttr,
  selectorCaption,
  setLevel,
  setRect,
} from "../src/core/cascade.js";

function lv(p: Partial<LevelValues>): LevelValues {
  return p;
}

test("there are five chains each rooted at base", () => {
  assert.equal(CHAINS.length, 5);
  for (const chain of CHAINS) {
    assert.equal(chainLevels(chain)[0], "base", `${chain} must start at base`);
    const depths = chainLevels(chain).map((l) => chainDepth(chain, l)!);
    for (let i = 1; i < depths.length; i++) {
      assert.ok(depths[i] > depths[i - 1], `${chain} depths must ascend`);
    }
  }
});

test("chains do not overlap except at base", () => {
  // The whole point of five chains: a Bible verse must not inherit from the
  // Songbook, and an Image must not inherit from the Bible.
  const songs = chainLevels("songs");
  for (const other of ["bibles", "images", "videos", "powerPoint"] as Chain[]) {
    for (const l of chainLevels(other)) {
      if (l === "base") continue;
      assert.ok(!songs.includes(l), `${l} is in both the song chain and ${other}`);
    }
  }
});

test("the song chain is five rungs and the bible chain seven", () => {
  assert.deepEqual(chainLevels("songs"), [
    "base",
    "songbooks",
    "songbook",
    "song",
    "songVerse",
  ]);
  assert.deepEqual(chainLevels("bibles"), [
    "base",
    "bibles",
    "bible",
    "testament",
    "book",
    "chapter",
    "verse",
  ]);
});

test("there are eighteen levels and every one is in some chain", () => {
  assert.equal(LEVELS.length, 18);
  const covered = new Set(CHAINS.flatMap((c) => chainLevels(c)));
  assert.deepEqual(new Set(LEVELS), covered);
  // The chains are disjoint apart from `base`, so membership is unambiguous.
  for (const l of LEVELS) {
    const chains = CHAINS.filter((c) => chainContains(c, l));
    assert.equal(chains.length, l === "base" ? 5 : 1, `${l} belongs to ${chains.length} chain(s)`);
  }
});

test("an unset attribute falls through to the built-in default", () => {
  const c = emptyCascade();
  const r = resolve(c, "songs");
  assert.equal(r.font.value, "Candara");
  assert.equal(r.size.value, 70, "1.26: default body size is 70");
  assert.equal(r.font.source, undefined, "nothing set it, so there is no source");
});

test("a more specific level wins and is reported as the source", () => {
  let c = emptyCascade();
  c = setLevel(c, "base", lv({ size: 40 }));
  c = setLevel(c, "song", lv({ size: 90 }));

  const r = resolve(c, "songs");
  assert.equal(r.size.value, 90, "the song overrides base");
  assert.equal(r.size.source, "song");
  assert.ok(isSetHere(r.size, "song"));
  assert.ok(!isInheritedFrom(r.size, "song"));
});

test("provenance is per attribute not per level", () => {
  // This is the distinction the original's badges exist to show: the font came
  // from Base, the size from this song.
  let c = emptyCascade();
  c = setLevel(c, "base", lv({ font: "Times" }));
  c = setLevel(c, "song", lv({ size: 64 }));

  const r = resolve(c, "songs");
  assert.equal(r.font.source, "base");
  assert.equal(r.size.source, "song");
  // Asked "at the song level", the font is inherited and the size is local.
  assert.ok(isInheritedFrom(r.font, "song"));
  assert.ok(!isInheritedFrom(r.size, "song"));
});

test("a bible verse never inherits from the song chain", () => {
  let c = emptyCascade();
  c = setLevel(c, "songbook", lv({ size: 20 }));
  c = setLevel(c, "bible", lv({ size: 55 }));

  const bible = resolve(c, "bibles");
  assert.equal(bible.size.value, 55, "the songbook must not leak into the Bible chain");
  assert.equal(bible.size.source, "bible");
});

test("an image style never inherits the bible or song values", () => {
  let c = emptyCascade();
  c = setLevel(c, "song", lv({ color: "#ff0000" }));
  c = setLevel(c, "image", lv({ luminosity: -0.5 }));

  const img = resolve(c, "images");
  assert.notEqual(img.color.value, "#ff0000", "an image's colour must not come from the song chain");
  assert.equal(img.luminosity.source, "image");
});

test("reset removes the override so the value re-inherits", () => {
  let c = emptyCascade();
  c = setLevel(c, "base", lv({ size: 50 }));
  c = setLevel(c, "song", lv({ size: 80 }));
  assert.equal(resolve(c, "songs").size.value, 80);

  c = resetLevel(c, "song");
  const r = resolve(c, "songs");
  assert.equal(r.size.value, 50, "reset restores the inherited value");
  assert.equal(r.size.source, "base");
});

test("an empty level is not stored at all", () => {
  const c = setLevel(emptyCascade(), "song", {});
  assert.equal(getLevel(c, "song"), undefined, "an empty level must not be stored");
  assert.ok(levelIsEmpty(undefined));
  assert.ok(levelIsEmpty({}));
});

test("resolveAttr walks most-specific first and stops at the first setter", () => {
  let c = emptyCascade();
  c = setLevel(c, "base", lv({ size: 10 }));
  c = setLevel(c, "songbook", lv({ size: 20 }));
  c = setLevel(c, "song", lv({ size: 30 }));

  assert.equal(resolveAttr(c, "songs", "size", 70).value, 30);
  // `songVerse` sets nothing, so it inherits from `song` rather than `songbook`.
  assert.equal(resolveAttr(c, "songs", "size", 70).source, "song");
  assert.equal(resolveAttr(emptyCascade(), "songs", "size", 70).source, undefined);
});

test("the selector shows the original's five names most specific first", () => {
  assert.equal(selectorCaption("verse"), "Verse");
  assert.equal(selectorCaption("song"), "Song");
  assert.equal(selectorCaption("songbook"), "Songbook");
  assert.equal(selectorCaption("songbooks"), "Songbooks");
  assert.equal(selectorCaption("base"), "Base");
  // Media levels have no short caption; the literal stands in.
  assert.equal(selectorCaption("powerPoint"), "powerPoint");
});

test("the explorer has eleven rows and carries a source for every attribute", () => {
  const c = setLevel(emptyCascade(), "base", lv({ font: "Times" }));
  const rows = explorerRows(c, "songs");
  assert.equal(rows.length, 11);
  assert.equal(rows.length, ATTRIBUTES.length);
  for (const row of rows) {
    assert.ok(row.label.length > 0, `${row.attr} needs a label`);
    assert.ok(row.value.length > 0, `${row.attr} needs a value`);
  }
  // Every attribute resolves, so the tree always has a value to show.
  assert.equal(rows.find((r) => r.attr === "font")!.source, "base");
  assert.equal(rows.find((r) => r.attr === "size")!.source, undefined);
});

test("the explorer marks inherited leaves against the inspected level", () => {
  let c = emptyCascade();
  c = setLevel(c, "base", lv({ font: "Times" }));
  c = setLevel(c, "song", lv({ size: 64 }));

  const atSong = explorerRows(c, "songs", "song");
  const font = atSong.find((r) => r.attr === "font")!;
  const size = atSong.find((r) => r.attr === "size")!;
  assert.equal(font.inherited, true, "the font came from base, so it is inherited");
  assert.equal(size.inherited, false, "the size is local to the song");

  // Without an inspected level nothing is "inherited" — there is nothing to be
  // inherited *from*.
  for (const row of explorerRows(c, "songs")) assert.equal(row.inherited, false);
});

test("values format the way the explorer shows them", () => {
  assert.equal(formatValue("size", 70), "70");
  assert.equal(formatValue("size", 70.5), "70.50");
  assert.equal(formatValue("bold", true), "yes");
  assert.equal(formatValue("bold", false), "no");
  assert.equal(formatValue("font", "Candara"), "Candara");
  assert.equal(formatValue("bgColor", "#000000"), "#000000");
});

test("a cascade round-trips through JSON", () => {
  let c = emptyCascade();
  c = setLevel(c, "song", lv({ size: 77 }));
  c = setLevel(c, "base", lv({ font: "Candara" }));
  const back: Cascade = JSON.parse(JSON.stringify(c));
  assert.deepEqual(back, c);
  assert.equal(resolve(back, "songs").size.source, "song");
});

test("an absent cascade resolves rather than throwing", () => {
  // Existing library files have no cascade at all, so this is what a real read
  // looks like: a bare object with no `values` key. It must resolve to the
  // built-in defaults, not throw. Rust gets this for free via
  // `#[serde(default)]`; TypeScript does not, so every reader guards.
  const c: Cascade = JSON.parse("{}");
  assert.equal(resolve(c, "songs").size.value, 70);
  assert.equal(getLevel(c, "song"), undefined);
  assert.deepEqual(attributeSet(c, "song"), []);
  const rows = explorerRows(c, "songs", "song");
  assert.equal(rows.length, 11);
  // A setLevel on a bare object must not drop the seed.
  const seeded = setLevel(c, "song", lv({ size: 90 }));
  assert.equal(resolve(seeded, "songs").size.value, 90);
  assert.equal(resolve(seeded, "songs").size.source, "song");
});

test("a level rect is stored per level and cleared by reset", () => {
  let c = setRect(emptyCascade(), "song", { x: 0, y: 1000, w: 9000, h: 8000 });
  c = setRect(c, "base", { x: 0, y: 0, w: 10000, h: 10000 });
  assert.deepEqual(c.rects!["song"], { x: 0, y: 1000, w: 9000, h: 8000 });

  c = resetLevel(c, "song");
  assert.equal(c.rects!["song"], undefined, "reset clears the rect too");
  assert.deepEqual(c.rects!["base"], { x: 0, y: 0, w: 10000, h: 10000 });
});

// ── Cross-port pins ─────────────────────────────────────────────────────────
// These are the exact facts `tests/crossPort.test.ts` asserts against the Rust
// side, so a divergence fails rather than drifting.

test("CROSS-PORT: every level literal matches the Rust enum", () => {
  // Rust: vp-core/src/cascade.rs `Level::ALL` + `Level::literal()`.
  assert.deepEqual([...LEVELS], [
    "base",
    "songbooks",
    "songbook",
    "song",
    "songVerse",
    "bibles",
    "bible",
    "testament",
    "book",
    "chapter",
    "verse",
    "images",
    "image",
    "videos",
    "video",
    "powerPoints",
    "powerPoint",
    "slides",
  ]);
});

test("CROSS-PORT: the five chains and their rungs match the Rust enum", () => {
  const expected: Record<Chain, Level[]> = {
    songs: ["base", "songbooks", "songbook", "song", "songVerse"],
    bibles: ["base", "bibles", "bible", "testament", "book", "chapter", "verse"],
    images: ["base", "images", "image"],
    videos: ["base", "videos", "video"],
    powerPoint: ["base", "powerPoints", "powerPoint", "slides"],
  };
  assert.deepEqual(Object.keys(expected), [...CHAINS]);
  for (const [chain, levels] of Object.entries(expected)) {
    assert.deepEqual([...chainLevels(chain as Chain)], levels, chain);
  }
});

test("CROSS-PORT: attribute names and defaults match the Rust struct", () => {
  assert.deepEqual([...ATTRIBUTES], [
    "font",
    "size",
    "color",
    "bold",
    "italic",
    "underline",
    "strike",
    "vAlign",
    "bgColor",
    "transition",
    "luminosity",
  ]);
  const d = resolve(emptyCascade(), "songs");
  assert.equal(d.font.value, "Candara");
  assert.equal(d.size.value, 70);
  assert.equal(d.color.value, "#ffffff");
  assert.equal(d.vAlign.value, "center");
  assert.equal(d.bgColor.value, "#000000");
  assert.equal(d.transition.value, "none");
  assert.equal(d.luminosity.value, 0);
  assert.equal(d.bold.value, false);
});

test("CROSS-PORT: attributeSet reports exactly the attributes a level sets", () => {
  const c = setLevel(emptyCascade(), "song", { font: "Arial", bold: true });
  assert.deepEqual(attributeSet(c, "song").sort(), ["bold", "font"]);
  assert.deepEqual(attributeSet(c, "base"), []);
});