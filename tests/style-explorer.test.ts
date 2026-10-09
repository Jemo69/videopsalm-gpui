/**
 * Style explorer tree view — **definition-of-done #6 UI**.
 *
 * Drives the real view module (`src/renderer/style-explorer.ts`), not the
 * cascade model directly: the tree must render rows with correct
 * source/inherited flags for a seeded cascade, reset must flip a local value
 * back to inherited, and the selector must list the original's five names
 * most-specific-first. Rendering is string-based (`renderExplorerHtml`) so
 * these run headless under `node --test` with no DOM.
 */
import test from "node:test";
import assert from "node:assert/strict";
import {
  CHAIN_LABELS,
  CHAIN_PATHS,
  LEVEL_CAPTIONS,
  SELECTOR_LEVELS,
  buildExplorerTree,
  chainKey,
  clearAttribute,
  createExplorerState,
  inspectedFromStyleLevel,
  levelKey,
  renderExplorerHtml,
  seedFromPrefs,
  selectorCaptions,
} from "../src/renderer/style-explorer.js";
import {
  attributeSet,
  chainLevels,
  emptyCascade,
  resetLevel,
  setLevel,
} from "../src/core/cascade.js";

/** font from Base + size on Song — the DoD's canonical seed. */
function seeded(): ReturnType<typeof emptyCascade> {
  let c = emptyCascade();
  c = setLevel(c, "base", { font: "Times" });
  c = setLevel(c, "song", { size: 64 });
  return c;
}

test("the selector lists the original's five names most-specific-first", () => {
  assert.deepEqual(selectorCaptions(), [
    "Verse",
    "Song",
    "Songbook",
    "Songbooks",
    "Base",
  ]);
  assert.deepEqual([...SELECTOR_LEVELS], [
    "verse",
    "song",
    "songbook",
    "songbooks",
    "base",
  ]);
  // Media levels are reachable through the gallery path, not the selector.
  for (const media of ["images", "image", "videos", "video", "powerPoints", "powerPoint", "slides"]) {
    assert.ok(
      !(SELECTOR_LEVELS as readonly string[]).includes(media),
      `${media} must not be in the selector`,
    );
  }
});

test("the tree has five chains with their own levels only", () => {
  const tree = buildExplorerTree(seeded());
  assert.equal(tree.length, 5);
  assert.deepEqual(
    tree.map((t) => t.chain),
    ["songs", "bibles", "images", "videos", "powerPoint"],
  );
  const byChain = Object.fromEntries(tree.map((t) => [t.chain, t]));
  assert.deepEqual(
    byChain["songs"].levels.map((l) => l.level),
    ["base", "songbooks", "songbook", "song", "songVerse"],
  );
  assert.deepEqual(
    byChain["bibles"].levels.map((l) => l.level),
    ["base", "bibles", "bible", "testament", "book", "chapter", "verse"],
  );
  assert.equal(byChain["images"].levels.length, 3);
  assert.equal(byChain["videos"].levels.length, 3);
  assert.equal(byChain["powerPoint"].levels.length, 4);
  // No cross-kind inheritance is SHOWN: no Bible level under Songs, and the
  // song chain never appears inside the Bible chain.
  const songLevels = byChain["songs"].levels.map((l) => l.level);
  for (const l of chainLevels("bibles")) {
    if (l === "base") continue;
    assert.ok(!songLevels.includes(l), `${l} must not render under Songs`);
  }
  for (const l of chainLevels("songs")) {
    if (l === "base") continue;
    assert.ok(
      !byChain["bibles"].levels.some((x) => x.level === l),
      `${l} must not render under Bibles`,
    );
  }
  // Every level carries its 11 leaves in the original's order.
  for (const ch of tree) {
    for (const ln of ch.levels) {
      assert.equal(ln.rows.length, 11, `${ch.chain}/${ln.level} leaf count`);
      assert.equal(ln.rows[0].attr, "font");
      assert.equal(ln.rows[1].attr, "size");
      assert.equal(ln.rows[10].attr, "luminosity");
    }
  }
});

test("rows render with correct source and inherited flags for a seeded cascade", () => {
  const tree = buildExplorerTree(seeded());
  const songs = tree.find((t) => t.chain === "songs")!;
  const atSong = songs.levels.find((l) => l.level === "song")!;
  const font = atSong.rows.find((r) => r.attr === "font")!;
  const size = atSong.rows.find((r) => r.attr === "size")!;
  assert.equal(font.value, "Times");
  assert.equal(font.source, "base");
  assert.equal(font.inherited, true, "font came from Base: inherited at Song");
  assert.equal(size.value, "64");
  assert.equal(size.source, "song");
  assert.equal(size.inherited, false, "size is local to Song");
  assert.deepEqual(atSong.local, ["size"]);
});

test("reset flips a local value back to inherited", () => {
  let c = seeded();
  assert.equal(
    buildExplorerTree(c)
      .find((t) => t.chain === "songs")!
      .levels.find((l) => l.level === "song")!
      .rows.find((r) => r.attr === "size")!.inherited,
    false,
  );
  c = resetLevel(c, "song");
  const atSong = buildExplorerTree(c)
    .find((t) => t.chain === "songs")!
    .levels.find((l) => l.level === "song")!;
  const size = atSong.rows.find((r) => r.attr === "size")!;
  assert.equal(size.source, undefined, "nothing sets size now: built-in default");
  assert.equal(size.value, "70", "1.26 default body size");
  assert.deepEqual(atSong.local, []);
  // And when Base DOES set it, reset re-inherits from Base.
  c = setLevel(c, "base", { font: "Times", size: 50 });
  c = setLevel(c, "song", { size: 80 });
  c = resetLevel(c, "song");
  const size2 = buildExplorerTree(c)
    .find((t) => t.chain === "songs")!
    .levels.find((l) => l.level === "song")!
    .rows.find((r) => r.attr === "size")!;
  assert.equal(size2.value, "50");
  assert.equal(size2.source, "base");
  assert.equal(size2.inherited, true);
});

test("per-attribute clear leaves the level's other values alone", () => {
  let c = emptyCascade();
  c = setLevel(c, "base", { font: "Times", size: 50 });
  c = setLevel(c, "song", { size: 80, color: "#ff0000" });
  c = clearAttribute(c, "song", "size");
  const atSong = buildExplorerTree(c)
    .find((t) => t.chain === "songs")!
    .levels.find((l) => l.level === "song")!;
  const size = atSong.rows.find((r) => r.attr === "size")!;
  const color = atSong.rows.find((r) => r.attr === "color")!;
  assert.equal(size.source, "base", "size re-inherits from Base");
  assert.equal(size.inherited, true);
  assert.equal(color.source, "song", "color stays local");
  assert.equal(color.inherited, false);
  assert.deepEqual(attributeSet(c, "song"), ["color"]);
  // Clearing the last local attribute drops the level entry entirely.
  c = clearAttribute(c, "song", "color");
  assert.deepEqual(attributeSet(c, "song"), []);
});

test("the HTML marks sources and inherited leaves", () => {
  const html = renderExplorerHtml(buildExplorerTree(seeded()), "song");
  assert.match(html, /data-attr="font" data-source="base" data-inherited="true"/);
  assert.match(html, /data-attr="size" data-source="song" data-inherited="false"/);
  assert.match(html, />inherited</, "inherited leaves carry the marker");
  assert.match(html, />set here</, "local leaves say so");
  assert.match(html, /aria-pressed="true"[^>]*>✓ Song</, "selector marks the inspected level");
  assert.match(html, /data-level="songbook"/, "every chain level renders");
  // Collapse/expand boxes match the original's ⊞/⊟ (OBSERVATIONS.md §8).
  const openHtml = renderExplorerHtml(
    buildExplorerTree(seeded()),
    "song",
    new Set([chainKey("songs"), levelKey("songs", "song")]),
  );
  assert.match(openHtml, /⊟ Songs/, "open chains show ⊟");
  assert.match(openHtml, /⊞ Bibles/, "shut chains show ⊞");
  assert.match(openHtml, /⊟ Song /, "open levels show ⊟");
});

test("chain and level keys are stable and distinct", () => {
  assert.equal(chainKey("songs"), "chain:songs");
  assert.equal(levelKey("songs", "song"), "level:songs:song");
  assert.notEqual(levelKey("songs", "song"), levelKey("bibles", "verse"));
  const state = createExplorerState();
  assert.equal(state.inspected, "song");
  assert.ok(state.expanded.has(chainKey("songs")), "chains open by default");
  assert.ok(!state.expanded.has(levelKey("songs", "song")), "levels shut by default");
});

test("the legacy styleLevel pref maps onto cascade levels", () => {
  assert.equal(inspectedFromStyleLevel("base"), "base");
  assert.equal(inspectedFromStyleLevel("songbooks"), "songbooks");
  assert.equal(inspectedFromStyleLevel("song"), "song");
  assert.equal(inspectedFromStyleLevel("verse"), "songVerse");
  assert.equal(inspectedFromStyleLevel("nonsense"), "song");
});

test("seeding adopts prefs at Base without clobbering", () => {
  const c = seedFromPrefs(emptyCascade(), { font: "Arial", size: 48, color: "#ffffff" });
  const tree = buildExplorerTree(c);
  const atBase = tree
    .find((t) => t.chain === "songs")!
    .levels.find((l) => l.level === "base")!;
  assert.equal(atBase.rows.find((r) => r.attr === "font")!.value, "Arial");
  assert.equal(atBase.rows.find((r) => r.attr === "size")!.value, "48");
  // A Base value already in the cascade wins over the prefs seed.
  const c2 = seedFromPrefs(setLevel(emptyCascade(), "base", { font: "Times" }), {
    font: "Arial",
  });
  assert.equal(
    buildExplorerTree(c2)
      .find((t) => t.chain === "songs")!
      .levels.find((l) => l.level === "base")!
      .rows.find((r) => r.attr === "font")!.value,
    "Times",
  );
  // NaN sizes never seed.
  const c3 = seedFromPrefs(emptyCascade(), { size: Number("oops") });
  assert.deepEqual(c3, emptyCascade());
});

test("tree captions cover every chain and level", () => {
  for (const ch of ["songs", "bibles", "images", "videos", "powerPoint"] as const) {
    assert.ok(CHAIN_LABELS[ch].length > 0);
    assert.ok(CHAIN_PATHS[ch].startsWith("Base"));
  }
  for (const lv of chainLevels("songs")) assert.ok(LEVEL_CAPTIONS[lv].length > 0);
  for (const lv of chainLevels("bibles")) assert.ok(LEVEL_CAPTIONS[lv].length > 0);
});
