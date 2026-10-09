// Instant-find semantics (UX-RESEARCH §6.1).
//
// These are the *documented rules* rather than a description of the current
// implementation, so the tests read as a checklist against the research and
// fail loudly if someone simplifies the matching back to a substring test.

import { test } from "node:test";
import * as assert from "node:assert/strict";

import {
  ALL_SCOPES,
  DEFAULT_SCOPE,
  MIN_QUERY_CHARS,
  filterSongs,
  matchesSong,
  normalizeSearchText,
  parseBookScoped,
  scopeLabel,
  searchWords,
  shouldSearch,
} from "../src/core/search";
import type { Song } from "../src/core/schemas";

function song(over: Partial<Song> = {}): Song {
  return {
    id: "s1",
    title: "A new commandment",
    verses: [{ id: "v1", lines: ["Love one another", "as I have loved you"] }],
    authors: ["John"],
    createdAt: "",
    updatedAt: "",
    ...over,
  };
}

test("search does not start until more than three characters are typed", () => {
  // 1.13: "VideoPsalm starts searching after you type more than three
  // characters."
  assert.equal(shouldSearch(""), false);
  assert.equal(shouldSearch("a"), false);
  assert.equal(shouldSearch("ab"), false);
  assert.equal(shouldSearch("abc"), false);
  assert.equal(shouldSearch("abcd"), true);
  assert.equal(MIN_QUERY_CHARS, 4);
});

test("below the threshold the whole library is shown, not nothing", () => {
  const songs = [song(), song({ id: "s2", title: "Beta" })];
  assert.equal(filterSongs(songs, "ab").length, 2);
  assert.equal(filterSongs(songs, "commandment").length, 1);
});

test("word order does not matter", () => {
  // "new commandm" must find "A new commandment" — the documented example.
  assert.equal(matchesSong(song(), "new commandm", "title-lyrics"), true);
  assert.equal(matchesSong(song(), "commandm new", "title-lyrics"), true);
});

test("a phrase is typed as one word and matches in order", () => {
  // "kingofkingslordoflords" must find "King of kings, Lord of lords".
  const k = song({
    title: "King of kings, Lord of lords",
    verses: [{ id: "v1", lines: ["Divided by the cross"] }],
  });
  assert.equal(matchesSong(k, "kingofkingslordoflords", "title-lyrics"), true);
  // Same words, wrong order, is a different phrase and must not match.
  assert.equal(matchesSong(k, "lordsofkingskingof", "title-lyrics"), false);
});

test("accents, case and punctuation are ignored", () => {
  // Accents fold, case folds, and punctuation is *deleted* — so the comma
  // and the space around it vanish rather than becoming a separator.
  assert.equal(normalizeSearchText("Bénédicite, Seigneur!"), "benediciteseigneur");
  assert.equal(matchesSong(song({ title: "Bénédicite" }), "benedicite", "title"), true);
  // "Genèse" is findable as "genese".
  assert.equal(matchesSong(song({ title: "Genèse des忽视了" }), "genese", "title"), true);
  // A phrase survives a comma mid-way: punctuation must not break adjacency.
  assert.equal(
    matchesSong(song({ title: "Jesus, the Christ" }), "jesusthechrist", "title-lyrics"),
    true
  );
});

test("digits inside lyrics are findable", () => {
  // 1.27b added digit search.
  const s = song({ title: "CwmRhondda", verses: [{ id: "v1", lines: ["verse 66"] }] });
  assert.equal(matchesSong(s, "66", "title-lyrics"), true);
});

test("a songbook-scoped number selects one song", () => {
  // 1.11: "soc204" selects number 204 in the songbook abbreviated "soc".
  assert.deepEqual(parseBookScoped("soc204"), { abbrev: "soc", number: "204" });
  assert.equal(parseBookScoped("204"), null);
  const s = song({ title: "Anything", songbookId: "soc", songId: "204" });
  assert.equal(matchesSong(s, "soc204"), true);
  assert.equal(matchesSong(s, "soc205"), false);
  // The same number in a different songbook is not a match.
  assert.equal(matchesSong(song({ songbookId: "other", songId: "204" }), "soc204"), false);
});

test("the default scope is Title and lyrics, and it spans both fields", () => {
  assert.equal(DEFAULT_SCOPE, "title-lyrics");
  assert.equal(scopeLabel("title-lyrics"), "Title and lyrics");
  const s = song({ title: "Title only", verses: [{ id: "v1", lines: ["lyricword"] }] });
  assert.equal(matchesSong(s, "titleonly lyricword"), true);
  // Not in the title alone, not in the lyrics alone → no match.
  assert.equal(matchesSong(s, "titleonly lyricword", "title"), false);
});

test("there are exactly twelve scopes, labelled as the original labels them", () => {
  assert.equal(ALL_SCOPES.length, 12);
  assert.equal(new Set(ALL_SCOPES).size, 12);
  const labels = ALL_SCOPES.map(scopeLabel);
  for (const expected of [
    "Title and lyrics", "Title then lyrics", "Title", "Author and composer",
    "Themes", "References", "CCLI", "Copyright", "Memo 1", "Memo 2", "Memo 3",
    "Songs with chords",
  ]) {
    assert.ok(labels.includes(expected), `missing scope label: ${expected}`);
  }
});

test("Title then lyrics differs from Title and lyrics", () => {
  // The distinction is the whole reason both scopes exist: one requires the
  // lyric words in order and adjacent, the other does not.
  const s = song({
    title: "Great is thy faithfulness",
    verses: [{ id: "v1", lines: ["Faithful to me", " morning by morning"] }],
  });
  assert.equal(matchesSong(s, "faithful great thy is", "title-then-lyrics"), false);
  assert.equal(matchesSong(s, "faithful great thy is", "title-lyrics"), true);
});

test("CCLI, copyright, memos, themes and references are searchable fields", () => {
  const s = song({
    ccli: "1234567",
    copyright: "Public Domain",
    memo1: "morning",
    themes: ["hope"],
    references: ["Isaiah 53:5"],
  });
  assert.equal(matchesSong(s, "1234567", "ccli"), true);
  assert.equal(matchesSong(s, "public domain", "copyright"), true);
  assert.equal(matchesSong(s, "morning", "memo1"), true);
  assert.equal(matchesSong(s, "hope", "themes"), true);
  assert.equal(matchesSong(s, "isaiah 53 5", "references"), true);
});

test("Songs with chords only matches songs that have chords", () => {
  const withChords = song({ title: "X", chords: { notation: "latin", transpose: 0, overlays: [], chordpro: "[C]Amazing" } });
  const without = song({ title: "Y" });
  assert.equal(matchesSong(withChords, "amazing", "with-chords"), true);
  assert.equal(matchesSong(without, "amazing", "with-chords"), false);
});

test("an empty query matches everything", () => {
  assert.equal(matchesSong(song(), ""), true);
  assert.equal(matchesSong(song(), "   "), true);
});

test("filterSongs never mutates the input", () => {
  const songs = [song(), song({ id: "s2", title: "Other" })];
  const before = JSON.stringify(songs);
  filterSongs(songs, "commandment");
  assert.equal(JSON.stringify(songs), before);
});