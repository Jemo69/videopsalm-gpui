import test from "node:test";
import assert from "node:assert/strict";
import { formatTemplate, SlideTemplateContext } from "../src/core/template.js";

test("song tokens expansion in formatTemplate", () => {
  const ctx: SlideTemplateContext = {
    title: "Amazing Grace",
    author: "John Newton",
    songbook: "Hymns",
    songbookAbbrev: "HYM",
    verseRank: 3,
    verseCount: 5,
    verseTag: "Verse 3",
    ccli: "12345",
  };

  const legacy = "[SongTitle] ([SongBookAbbreviation]) - [SongVerseTag] ([SongVerseRank]/[SongVerseCount])";
  assert.equal(formatTemplate(legacy, ctx), "Amazing Grace (HYM) - Verse 3 (3/5)");

  const modern = "{title} — {position 3/5} | {author}";
  assert.equal(formatTemplate(modern, ctx), "Amazing Grace — 3/5 | John Newton");

  assert.equal(formatTemplate("{position}", ctx), "3/5");
  assert.equal(formatTemplate("{verse position}", ctx), "verse 3/5");
});

test("bible tokens expansion in formatTemplate", () => {
  const ctx: SlideTemplateContext = {
    bibleBook: "John",
    bibleChapter: 3,
    bibleVerse: "16",
    bibleName: "KJV",
    reference: "John 3:16",
  };

  const legacy = "[BibleBookName] [BibleChapterID]:[BibleVerseID] ([BibleName])";
  assert.equal(formatTemplate(legacy, ctx), "John 3:16 (KJV)");

  const modern = "{ref} — {bible}";
  assert.equal(formatTemplate(modern, ctx), "John 3:16 — KJV");
});

test("empty and missing tokens produce empty strings safely", () => {
  const ctx: SlideTemplateContext = {};
  assert.equal(formatTemplate("{title} {position 3/5}", ctx), " ");
  assert.equal(formatTemplate("", ctx), "");
});
