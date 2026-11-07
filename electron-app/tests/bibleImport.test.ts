// Bible XML import — the formats VideoPsalm 1.29 itself imports.
//
// Clean-room mirror of `vp-core/src/bible_import.rs`'s test corpus: same
// fixtures, same assertions, same operator-facing error strings, so an XML
// file imports identically in both ports (AGENTS.md rule 3 in spirit — the
// shared shape is the verse list the projector renders).

import test from "node:test";
import assert from "node:assert/strict";
import {
  adjustSpaces,
  BibleImportError,
  cleanOsisText,
  importBibleXml,
  importOsis,
  importZefania,
  stripLeadingVerseNumber,
} from "../src/core/bibleImport";
import { parseXmlDoc } from "../src/core/xml";
import { resolve } from "../src/core/bibleData";
import type { BibleRef } from "../src/core/schemas";

const OSIS = `<?xml version="1.0" encoding="UTF-8"?>
<osis xmlns="http://www.bibleorg.org/osis" version="2.1.1">
  <osisText xml:lang="en" osisIDWork="KJV" osisRefWork="Bible">
    <chapter osisID="Gen.1">
      <verse osisID="Gen.1.1">In the  beginning God created *the heavens</verse>
      <verse osisID="Gen.1.2">And the earth was&nbsp;without form</verse>
    </chapter>
    <chapter osisID="Gen.2">
      <verse osisID="Gen.2.1">Second chapter.</verse>
    </chapter>
    <chapter osisID="John.3">
      <verse osisID="John.3.16">For God so loved the world</verse>
    </chapter>
  </osisText>
</osis>`;

const ZEFANIA = `<?xml version="1.0" encoding="UTF-8"?>
<XMLBIBLE biblename="Sample German Bible">
  <INFORMATION>
    <title>Sample</title>
    <identifier>SMB</identifier>
    <language>GER</language>
    <publisher>Test House</publisher>
    <rights>Public domain</rights>
    <description>A sample</description>
  </INFORMATION>
  <BIBLEBOOK bnumber="1" bname="Genesis" bsname="Gen">
    <CHAPTER cnumber="1">
      <VERS vnumber="2">Second verse ... with ellipsis</VERS>
      <VERS vnumber="1">First verse</VERS>
    </CHAPTER>
  </BIBLEBOOK>
  <BIBLEBOOK bnumber="43" bname="Johannes" bsname="Joh">
    <CHAPTER cnumber="3">
      <VERS vnumber="16">Also text</VERS>
    </CHAPTER>
  </BIBLEBOOK>
</XMLBIBLE>`;

const LOCAL = `<?xml version="1.0" encoding="utf-8"?>
<root>
  <info>
    <language>English</language>
    <translation>NIV</translation>
    <copyright>https://example.invalid</copyright>
    <timestamp>2026-05-25T22:49:01</timestamp>
    <meaningless>1.4.0</meaningless>
  </info>
  <book name="Genesis" tag="_Genesis">
    <chapter number="1" tag="_1">
      <passage number="1">&#185; In the beginning God created the heavens and the earth. </passage>
      <passage number="2">&#185;&#185; Now the earth was formless and empty.</passage>
    </chapter>
  </book>
  <book name="John" tag="_John">
    <chapter number="3" tag="_3">
      <passage number="16">&#185;&#186; For God so loved the world.</passage>
    </chapter>
  </book>
</root>`;

function ref(book: string, chapter: number, verses: number[]): BibleRef {
  return {
    translationId: "T",
    book,
    chapter,
    verses,
    versification: "EN",
  };
}

test("osis imports metadata, books, and cleanup", () => {
  const t = importBibleXml(OSIS, "fallback");
  assert.equal(t.translationId, "KJV");
  assert.equal(t.language, "en");
  assert.equal(t.source, "osis");
  assert.equal(t.books.length, 2); // Genesis + John
  assert.equal(t.books[0].name, "Genesis");
  assert.equal(t.books[1].name, "John");
  assert.deepEqual(resolve(t, ref("Genesis", 1, [1, 2])), [
    "1 In the beginning God created the heavens",
    "2 And the earth was without form",
  ]);
});

test("osis missing-root messages match the original", () => {
  // The format fns carry the original's own checks (as in the Rust corpus);
  // the dispatcher's job is sniffing, so these call the format directly.
  assert.throws(
    () => importOsis(parseXmlDoc("<nope/>"), "x"),
    (e: Error) => e.message === "Missing 'osis' tag."
  );
  assert.throws(
    () => importOsis(parseXmlDoc("<osis><x/></osis>"), "x"),
    (e: Error) => e.message === "Missing 'osisText' tag"
  );
});

test("osis chapter intro becomes verse one", () => {
  const xml = `<osis><osisText xml:lang="en" osisIDWork="X">
    <chapter osisID="Ps.23">The LORD is my shepherd, says the psalmist.
      <verse osisID="Ps.23.2">He makes me lie down</verse>
    </chapter>
  </osisText></osis>`;
  const t = importBibleXml(xml, "x");
  const lines = resolve(t, ref("Psalm", 23, [1, 2]));
  assert.equal(lines.length, 2, JSON.stringify(lines));
  assert.match(lines[0], /^1 The LORD is my shepherd/);
  assert.match(lines[1], /^2 He makes me/);
});

test("osis verse gap resolves as absent, not wrong", () => {
  const xml = `<osis><osisText xml:lang="en" osisIDWork="X">
    <verse osisID="Ps.107.1">v one</verse>
    <verse osisID="Ps.107.3">v three</verse>
  </osisText></osis>`;
  const t = importBibleXml(xml, "x");
  assert.deepEqual(resolve(t, ref("Psalm", 107, [1, 2, 3])), [
    "1 v one",
    "3 v three",
  ]);
});

test("osis empty verse list reports no books", () => {
  assert.throws(
    () => importBibleXml(`<osis><osisText xml:lang="en" osisIDWork="X"/></osis>`, "x"),
    (e: Error) => e.message === "No Bible books found."
  );
});

test("zefania imports metadata, sorted verses, and ellipsis", () => {
  const t = importBibleXml(ZEFANIA, "fallback");
  assert.equal(t.translationId, "SMB");
  assert.equal(t.language, "GER"); // only exact "FRE" is remapped
  assert.equal(t.publisher, "Test House");
  assert.equal(t.copyright, "Public domain");
  assert.equal(t.description, "Sample German Bible");
  assert.equal(t.source, "zefania");
  // Out-of-order VERS elements come out sorted by number.
  assert.deepEqual(resolve(t, ref("Genesis", 1, [1, 2])), [
    "1 First verse",
    "2 Second verse \u2026 with ellipsis",
  ]);
  // Book 43 kept its file name (Johannes), matched by number.
  assert.equal(t.books[1].name, "Johannes");
  assert.equal(t.books[1].abbreviation, "Joh");
});

test("zefania FRE becomes fr with the French cleanup", () => {
  const xml = `<XMLBIBLE biblename="B">
    <INFORMATION><language>FRE</language><title>T</title></INFORMATION>
    <BIBLEBOOK bnumber="1" bname="Genèse" bsname="Gn">
      <CHAPTER cnumber="1">
        <VERS vnumber="1">Il a dit: &#8217; &#8217;oeuvre&#8217; OK</VERS>
      </CHAPTER>
    </BIBLEBOOK>
  </XMLBIBLE>`;
  const t = importBibleXml(xml, "x");
  assert.equal(t.language, "fr");
  const line = resolve(t, ref("Genesis", 1, [1]))[0];
  assert.match(line, /œ/);
  assert.doesNotMatch(line, /oeuvre/);
});

test("zefania missing root message matches the original", () => {
  assert.throws(
    () => importZefania(parseXmlDoc("<root/>"), "x"),
    (e: Error) =>
      e.message === 'This is not a Zefania Bible: Missing "XMLBIBLE" tag.'
  );
});

test("local per-book imports and strips superscript numbers", () => {
  const t = importBibleXml(LOCAL, "fallback");
  assert.equal(t.translationId, "NIV");
  assert.equal(t.language, "English");
  assert.equal(t.copyright, "https://example.invalid");
  assert.equal(t.source, "videopsalm-xml");
  assert.deepEqual(resolve(t, ref("Genesis", 1, [1, 2])), [
    "1 In the beginning God created the heavens and the earth.",
    "2 Now the earth was formless and empty.",
  ]);
});

test("dispatch sniffs the three roots", () => {
  assert.equal(importBibleXml(OSIS, "x").source, "osis");
  assert.equal(importBibleXml(ZEFANIA, "x").source, "zefania");
  assert.equal(importBibleXml(LOCAL, "x").source, "videopsalm-xml");
  assert.throws(() => importBibleXml("<songs/>", "x"), BibleImportError);
});

test("bom, information-less zefania parses like the real-world files", () => {
  // Verbatim shape of Quelea's shipped KJV: BOM, no INFORMATION block, so
  // the fallback name (file stem) becomes the translation id.
  const xml =
    '\uFEFF<XMLBIBLE xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" ' +
    'biblename="King James Version" type="x-bible">' +
    '<BIBLEBOOK bnumber="1" bname="Genesis" bsname="Gen">' +
    '<CHAPTER cnumber="1">' +
    '<VERS vnumber="1">In the beginning God created the heaven and the earth.</VERS>' +
    '<VERS vnumber="2">And the earth was without form, and void.</VERS>' +
    "</CHAPTER></BIBLEBOOK></XMLBIBLE>";
  const t = importBibleXml(xml, "KJV");
  assert.equal(t.translationId, "KJV");
  assert.equal(t.description, "King James Version");
  assert.equal(t.books[0].name, "Genesis");
  assert.deepEqual(resolve(t, ref("Genesis", 1, [1])), [
    "1 In the beginning God created the heaven and the earth.",
  ]);
});

test("cleanup helpers match the decompiled rules", () => {
  assert.equal(cleanOsisText("a\u00a0\u00a0b *c "), "a b c");
  assert.equal(adjustSpaces("x  ...  y", false), "x \u2026 y");
  assert.equal(adjustSpaces("\u00b6 intro\nnext", false), "intro\nnext");
  assert.equal(stripLeadingVerseNumber("\u00b9 \u00b9\u00b2 Text"), "Text");
  assert.equal(stripLeadingVerseNumber("Plain"), "Plain");
});
