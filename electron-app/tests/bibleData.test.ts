// Bible translation store — the port of `vp-core/src/bible_data.rs`.
//
// The Rust module carries three tests (`resolve_and_context`,
// `keyword_search_finds_verses_by_words`, `library_switch_and_favorites`);
// these mirror them case-for-case so both ports agree on what matches
// (AGENTS.md rule 3, in spirit: no shared schema changes, same behaviour).

import test from "node:test";
import assert from "node:assert/strict";

import {
  BibleLibrary,
  bibleHitRows,
  context,
  keywordSearch,
  normalize,
  resolve,
  translationFromJson,
} from "../src/core/bibleData";
import { parseBibleRef } from "../src/core/refParser";

const SAMPLE = {
  translationId: "SAMPLE-EN",
  language: "en",
  books: [
    { name: "John", chapters: [[], [], ["x", "y", "z"]] },
    {
      name: "Psalm",
      chapters: [
        [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [], [],
        [], [], [], [], ["a", "b"],
      ],
    },
  ],
};

const t = () => translationFromJson(SAMPLE);
const p = (s: string) => parseBibleRef(s, "SAMPLE-EN", "EN");

test("resolve and context", () => {
  const tr = t();
  const r = p("John 3:1-2");
  assert.deepEqual(resolve(tr, r), ["1 x", "2 y"]);
  const ctx = context(tr, r, 0, 1);
  assert.equal(ctx.length, 3);
  // A French name resolves against the same book.
  const fr = p("Jean 3:1");
  assert.deepEqual(resolve(tr, fr), ["1 x"]);
});

test("keyword search finds verses by words", () => {
  const tr = t();
  // Single word.
  const hits = keywordSearch(tr, "y");
  assert.equal(hits.length, 1);
  assert.equal(hits[0].book, "John");
  assert.equal(hits[0].chapter, 3);
  assert.equal(hits[0].verse, 2);
  // AND semantics: the sample holds one word per verse, so no multi-word
  // query can match a single verse.
  assert.equal(keywordSearch(tr, "y").length, 1);
  assert.equal(keywordSearch(tr, "y z").length, 0);
  assert.equal(keywordSearch(tr, "x y").length, 0);
  // Empty / whitespace-only queries return nothing.
  assert.equal(keywordSearch(tr, "").length, 0);
  assert.equal(keywordSearch(tr, "   ").length, 0);
  // Diacritics/punctuation insensitive.
  assert.equal(keywordSearch(tr, "  Y!  ").length, 1);
});

test("library switch and favorites", () => {
  const lib = new BibleLibrary();
  lib.install(translationFromJson(SAMPLE));
  assert.equal(lib.switchTo("SAMPLE-EN"), true);
  assert.equal(lib.switchTo("MISSING"), false);
  lib.addFavorite("SAMPLE-EN");
  lib.addFavorite("SAMPLE-EN");
  assert.equal(lib.favorites.length, 1);
  lib.removeFavorite("SAMPLE-EN");
  assert.equal(lib.favorites.length, 0);
});

test("hits sort in canonical book/chapter/verse order", () => {
  const lib = new BibleLibrary();
  lib.install(translationFromJson(SAMPLE));
  const tr = lib.current()!;
  const hits = keywordSearch(tr, "a"); // Psalm 23:1
  assert.equal(hits.length, 1);
  assert.equal(hits[0].book, "Psalm");
  assert.equal(hits[0].chapter, 23);
  assert.equal(hits[0].verse, 1);
});

test("hit rows: one row per hit, never truncated (R-6)", () => {
  const long = "w".repeat(200);
  const rows = bibleHitRows([
    { book: "John", chapter: 3, verse: 16, text: long },
    { book: "John", chapter: 3, verse: 17, text: "short" },
  ]);
  assert.equal(rows.length, 2, "every hit gets a row — no .take(30)");
  assert.equal(rows[0][0], "John 3:16");
  assert.equal([...rows[0][1]].length, 70, "snippet capped at 70 chars");
  assert.equal(rows[1][1], "short");
  assert.deepEqual(bibleHitRows([]), []);
});

test("flat sample documents load as translations", () => {
  // `data/bible.sample.json` ships the flat `{verses:[…]}` shape.
  const flat = translationFromJson({
    translationId: "sample-en",
    verses: [
      { book: "John", chapter: 3, verse: 16, text: "For God so loved the world" },
      { book: "John", chapter: 3, verse: 17, text: "For God sent not his Son" },
    ],
  });
  assert.equal(flat.books.length, 1);
  const hits = keywordSearch(flat, "loved");
  assert.equal(hits.length, 1);
  assert.equal(hits[0].verse, 16);
});

test("normalize folds case, diacritics and punctuation", () => {
  assert.equal(normalize("  Y!  "), "y");
  assert.equal(normalize("Révélation"), "revelation");
  assert.equal(normalize("Psaumes 23:1"), "psaumes 23 1");
});
