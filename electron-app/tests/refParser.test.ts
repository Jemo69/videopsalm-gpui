// Bible reference parsing — the operator-facing contract.
//
// AGENTS.md rule 3 says the two ports must agree on the shared schema, and
// that agreement is only meaningful if the same typed string resolves the same
// way in both. `gpui-app/vp-core/src/bible_ref.rs` has a mirrored set of
// tests; if you change a rule here, change it there too.
//
// The cases below are the ones an operator actually types, including the
// original's own placeholder text.

import test from "node:test";
import assert from "node:assert/strict";

import {
  BOOKS,
  expandRange,
  findBook,
  formatRef,
  parseBibleRef,
  shrinkRange,
} from "../src/core/refParser";

const p = (s: string) => parseBibleRef(s, "sample", "EN");

test("a space after a comma in a verse list parses", () => {
  // The original's placeholder: "Reference to select, e.g.: Romans 3:23-25, 30"
  const r = p("Romans 3:23-25, 30");
  assert.equal(r.book, "Romans");
  assert.equal(r.chapter, 3);
  assert.deepEqual(r.verses, [23, 24, 25, 30]);
});

test("spaced and unspaced verse lists agree", () => {
  assert.deepEqual(p("Romans 3:23-25,30").verses, p("Romans 3:23-25, 30").verses);
});

test("several comma groups, each with a space", () => {
  assert.deepEqual(p("2 Timothy 3:16, 18, 23-25").verses, [16, 18, 23, 24, 25]);
});

test("spaces around the range dash are tolerated", () => {
  assert.deepEqual(p("John 3: 16 - 18").verses, [16, 17, 18]);
});

test("the dot separator is accepted", () => {
  const r = p("John 3.16");
  assert.equal(r.book, "John");
  assert.equal(r.chapter, 3);
  assert.deepEqual(r.verses, [16]);
});

test("the v separator is accepted in both cases", () => {
  assert.deepEqual(p("John 3v16").verses, [16]);
  assert.deepEqual(p("John 3V16").verses, [16]);
});

test("the dot separator works with a verse list", () => {
  assert.deepEqual(p("Hebrews 11.1,9").verses, [1, 9]);
});

test("a bare chapter means the whole chapter", () => {
  const r = p("Ps 23");
  // The canonical key is the singular English name, matching vp-core, so an
  // agenda written by either port resolves in the other (rule 3).
  assert.equal(r.book, "Psalm");
  assert.equal(r.chapter, 23);
  assert.deepEqual(r.verses, [], "an empty verse list is the whole chapter");
});

test("a trailing separator also means the whole chapter", () => {
  assert.deepEqual(p("John 3:").verses, []);
  assert.deepEqual(p("John 3.").verses, []);
  assert.equal(p("John 3:").chapter, 3);
});

test("a leading ordinal belongs to the book, not the chapter", () => {
  const r = p("1 John 2:1");
  assert.equal(r.book, "1 John");
  assert.equal(r.chapter, 2);
  assert.deepEqual(r.verses, [1]);
});

test("a bare book name is accepted", () => {
  const r = p("Revelation");
  assert.equal(r.book, "Revelation");
});

test("French book names and abbreviations resolve", () => {
  assert.equal(p("Jean 3:16").book, "John");
  assert.equal(p("Romains 3:23-25, 30").book, "Romans");
  assert.equal(p("Matthieu 5:14").book, "Matthew");
  assert.equal(p("Genèse 1:1").book, "Genesis");
  assert.equal(p("Esaïe 53:5").book, "Isaiah");
  assert.equal(p("Hébreux 11:1,9").book, "Hebrews");
});

test("accents are folded when matching book names", () => {
  assert.equal(findBook("Genese")?.key, "Genesis");
  assert.equal(findBook("Genèse")?.key, "Genesis");
  assert.equal(findBook("psaume")?.key, "Psalm");
});

test("a unique prefix resolves, an ambiguous one does not guess", () => {
  // Prefix matching is what makes typing feel like instant find.
  assert.equal(findBook("He")?.key, "Hebrews");
  // `P` is Psalms / Proverbs / Peter — guessing would silently pick wrong.
  assert.equal(findBook("P"), undefined);
  assert.equal(findBook("Notabook"), undefined);
});

test("an out-of-range chapter is rejected with a useful message", () => {
  assert.throws(() => p("John 99"), /out of range/);
  assert.throws(() => p("Jude 25"), /out of range/);
});

test("an unparseable string says so", () => {
  assert.throws(() => p("not a reference"), /cannot parse reference/);
});

test("verse lists are deduplicated and ordered", () => {
  assert.deepEqual(p("John 3:5,3,5,4").verses, [3, 4, 5]);
});

test("formatRef round-trips through the parser", () => {
  for (const src of ["John 3:16", "Romans 3:23-25,30", "Ps 23", "1 John 2:1"]) {
    const ref = p(src);
    assert.deepEqual(p(formatRef(ref)).verses, ref.verses, src);
    assert.equal(p(formatRef(ref)).book, ref.book, src);
  }
});

test("range adjust adds and removes at the end", () => {
  const r = p("John 3:16");
  assert.deepEqual(expandRange(r).verses, [16, 17]);
  assert.deepEqual(shrinkRange(expandRange(r)).verses, [16]);
  // Never shrinks below one verse.
  assert.deepEqual(shrinkRange(r).verses, [16]);
  // An empty selection starts at verse 1 rather than doing nothing.
  assert.deepEqual(expandRange(p("John 3")).verses, [1]);
});

test("the canon covers all 66 books with plausible chapter counts", () => {
  assert.equal(BOOKS.length, 66);
  for (const b of BOOKS) {
    assert.ok(b.chapters > 0 && b.chapters <= 150, `${b.key}: ${b.chapters}`);
    assert.ok(b.names.length > 0, `${b.key} has no accepted spelling`);
  }
});