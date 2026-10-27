import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  documentFromVerses,
  hasVerseTagLine,
  insertVerseTag,
  parseLyricsDocument,
  renderLyricsDocument,
  slidesFromLyricsDocument,
  slidesFromVerses,
  verseLabel,
  versesFromLyricsDocument,
} from "../src/core/lyrics-document";
import type { SongVerse } from "../src/core/schemas";

describe("VideoPsalm lyrics document", () => {
  it("uses tag lines for verse boundaries and blank lines for slide breaks", () => {
    const doc = "V1\nFirst line\n\nSecond slide\n\nC\nChorus line";
    const blocks = parseLyricsDocument(doc, true);
    assert.equal(blocks.length, 2);
    assert.equal(blocks[0].tag, "V");
    assert.equal(blocks[0].tagId, 1);
    assert.deepEqual(blocks[0].lines, ["First line", "", "Second slide"]);
    assert.equal(blocks[1].tag, "C");
    assert.deepEqual(blocks[1].lines, ["Chorus line"]);

    const slides = slidesFromLyricsDocument(doc, true);
    assert.deepEqual(slides.map((s) => s.label), ["V1", "V1", "C"]);
    assert.deepEqual(slides.map((s) => s.body), ["First line", "Second slide", "Chorus line"]);
  });

  it("keeps one native verse while writing an in-verse slide break as NBSP", () => {
    const existing: SongVerse[] = [
      { id: "1", lines: ["old"], order: 1, tag: "V", tagId: 1 },
      { id: "2", lines: ["old chorus"], order: 2, tag: "C", tagId: 0 },
    ];
    const verses = versesFromLyricsDocument("V1\none\n\ntwo\n\nC\nchorus", existing, true);
    assert.equal(verses.length, 2);
    assert.deepEqual(verses[0].lines, ["one", "\u00a0", "two"]);
    assert.equal(verses[0].tag, "V");
    assert.equal(verses[0].tagId, 1);
    assert.equal(verses[0].id, "1");
    assert.equal(verses[1].tag, "C");
    assert.equal(verses[1].tagId, 0);
  });

  it("preserves local-song stanza splitting when the song has no tags", () => {
    const blocks = parseLyricsDocument("Amazing grace\nhow sweet\n\nThat saved\n", false);
    assert.deepEqual(blocks.map((b) => b.lines), [
      ["Amazing grace", "how sweet"],
      ["That saved"],
    ]);
    const slides = slidesFromLyricsDocument("Amazing grace\nhow sweet\n\nThat saved", false);
    assert.deepEqual(slides.map((s) => s.body), ["Amazing grace\nhow sweet", "That saved"]);
  });

  it("treats a one-space line as content and a NBSP-only line as a break", () => {
    const parsed = parseLyricsDocument("V1\none\n \n\u00a0\ntwo", true);
    assert.deepEqual(parsed[0].lines, ["one", " ", "\u00a0", "two"]);
    const slides = slidesFromLyricsDocument("V1\none\n \n\u00a0\ntwo", true);
    assert.deepEqual(slides.map((s) => s.body), ["one\n ", "two"]);
  });

  it("round-trips the model document with headings and strips headings for preview", () => {
    const verses: SongVerse[] = [
      { id: "1", lines: ["line one", "\u00a0", "line two"], order: 1, tag: "V", tagId: 1 },
      { id: "2", lines: ["chorus"], order: 2, tag: "C", tagId: 0 },
    ];
    const doc = documentFromVerses(verses);
    assert.equal(doc, "V1\nline one\n\u00a0\nline two\n\nC\nchorus");
    assert.equal(renderLyricsDocument(doc, true), "line one\n\u00a0\nline two\n\nchorus");
    assert.equal(verseLabel("C", 0), "C");
    assert.equal(verseLabel("V", 2), "V2");
    assert.equal(hasVerseTagLine(doc), true);
    assert.equal(hasVerseTagLine("Amazing grace"), false);
  });

  it("labels both slides of a split verse with the same native tag", () => {
    const slides = slidesFromVerses([
      { id: "1", lines: ["one", "\u00a0", "two"], order: 1, tag: "V", tagId: 1 },
      { id: "2", lines: ["chorus"], order: 2, tag: "C", tagId: 1 },
    ]);
    assert.deepEqual(slides.map((s) => s.label), ["V1", "V1", "C1"]);
  });

  it("inserts a standalone verse heading and never duplicates a native one", () => {
    // Untagged local song: the heading starts the stanza holding the caret.
    const local = insertVerseTag("one\n\nchorus", 8, "C", false);
    assert.equal(local.changed, true);
    assert.equal(local.text, "one\n\nC\nchorus");
    assert.equal(local.cursor, 7);

    // Native document: the verse already has a heading, so nothing is added.
    const native = insertVerseTag("V1\none\n\nC\nchorus", 4, "C", true);
    assert.equal(native.changed, false);
    assert.equal(native.text, "V1\none\n\nC\nchorus");

    // Native document, caret before the first heading: label that block.
    const first = insertVerseTag("one\n\nC\nchorus", 0, "V1", true);
    assert.equal(first.changed, true);
    assert.equal(first.text, "V1\none\n\nC\nchorus");

    // The heading is always its own line, never inline with lyric text.
    assert.equal(insertVerseTag("Amazing grace", 0, "V1", false).text, "V1\nAmazing grace");
  });

  it("expands a native Sequence, repeating a chorus between verses", () => {
    const verses: SongVerse[] = [
      { id: "1", lines: ["verse one"], order: 1, tag: "V", tagId: 1 },
      { id: "2", lines: ["chorus"], order: 2, tag: "C", tagId: 0 },
      { id: "3", lines: ["verse two"], order: 3, tag: "V", tagId: 2 },
    ];
    const slides = slidesFromVerses(verses, "V1 C V2 C", 0);
    assert.deepEqual(slides.map((s) => s.label), ["V1", "C", "V2", "C"]);
    assert.deepEqual(slides.map((s) => s.body), ["verse one", "chorus", "verse two", "chorus"]);

    const alt = slidesFromVerses(verses, "V1 C V2 C, C V1", 1);
    assert.deepEqual(alt.map((s) => s.label), ["C", "V1"]);

    // A missing/negative index is VideoPsalm's default (-1): document order.
    assert.deepEqual(
      slidesFromVerses(verses, "V1 C V2 C", undefined).map((s) => s.label),
      ["V1", "C", "V2"]
    );
    // Malformed sequences must not blank the projector.
    assert.deepEqual(
      slidesFromVerses(verses, "X9 Q2", 0).map((s) => s.label),
      ["V1", "C", "V2"]
    );
  });

  it("repeats every slide of a multi-slide verse in Sequence order", () => {
    const doc = "V1\none\n\nsecond\n\nC\nchorus";
    const slides = slidesFromLyricsDocument(doc, true, "V1 C V1", 0);
    assert.deepEqual(slides.map((s) => s.label), ["V1", "V1", "C", "V1", "V1"]);
    assert.deepEqual(slides.map((s) => s.body), ["one", "second", "chorus", "one", "second"]);
  });
});
