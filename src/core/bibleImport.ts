// Bible translation import — the formats VideoPsalm 1.29 itself imports.
//
// Clean-room port of `gpui-app/vp-core/src/bible_import.rs`; that module's
// header documents the decompiled originals line for line
// (`msi-work/decompiled/videopsalm/VideoPsalm.Import/{Osis,Zefania}BibleImportFormat.cs`).
// Both ports must accept the same files and produce the same verses — same
// error messages too, since the operator sees them.
//
//   * OSIS      — root <osis><osisText xml:lang osisIDWork>, <verse osisID
//                 "Gen.1.1">, text child nodes only, nbsp→space, space runs
//                 collapsed, trim, `*` removed, chapter text before the first
//                 verse becomes verse 1, book index ≥ 39 = New Testament.
//   * Zefania   — root <XMLBIBLE biblename>, INFORMATION metadata,
//                 BIBLEBOOK/CHAPTER/VERS ids, adjustSpaces cleanup with the
//                 FRE→fr pass, chapters/verses sorted by number.
//   * Local XML — the per-book <root><info><translation> shape our tooling
//                 exports; superscript verse numbers are stripped.

import { attr, child, childrenNamed, descendants, directText, innerText, parseXmlDoc, XmlElement } from "./xml";
import type { Translation, TranslationBook } from "./bibleData";
import { BOOKS, findBook } from "./refParser";

/** Import failure; the message is the original's operator-facing text. */
export class BibleImportError extends Error {}

/** The 66 OSIS book codes in canonical order (the original's own array). */
const OSIS_BOOKS = [
  "Gen", "Exod", "Lev", "Num", "Deut", "Josh", "Judg", "Ruth", "1Sam", "2Sam",
  "1Kgs", "2Kgs", "1Chr", "2Chr", "Ezra", "Neh", "Esth", "Job", "Ps", "Prov",
  "Eccl", "Song", "Isa", "Jer", "Lam", "Ezek", "Dan", "Hos", "Joel", "Amos",
  "Obad", "Jonah", "Mic", "Nah", "Hab", "Zeph", "Hag", "Zech", "Mal", "Matt",
  "Mark", "Luke", "John", "Acts", "Rom", "1Cor", "2Cor", "Gal", "Eph", "Phil",
  "Col", "1Thess", "2Thess", "1Tim", "2Tim", "Titus", "Phlm", "Heb", "Jas",
  "1Pet", "2Pet", "1John", "2John", "3John", "Jude", "Rev",
];

/** `ImportFormat.GetNodeText` — the OSIS verse-text cleanup. */
export function cleanOsisText(raw: string): string {
  const s = raw.replace(/\u00a0/g, " ");
  let out = "";
  let space = false;
  for (const c of s) {
    if (c === "*") continue;
    if (c !== "\n" && /\s/.test(c)) {
      if (out.length > 0) space = true;
      continue;
    }
    if (space) {
      out += " ";
      space = false;
    }
    out += c;
  }
  return out.trim();
}

/** `Extension.AdjustSpaces(text, isFrench, isSongBook: false)`. */
export function adjustSpaces(raw: string, french: boolean): string {
  if (raw.length === 0) return "";
  let s = "";
  let space = false;
  for (const c of raw.replace(/\u00a0/g, " ")) {
    if (c === " " || c === "\t") {
      if (s.length > 0) space = true;
      continue;
    }
    if (space) {
      s += " ";
      space = false;
    }
    s += c;
  }
  s = s.replace(/\.\.\./g, "\u2026").replace(/\u2026\./g, "\u2026");
  if (french) {
    s = s
      .replace(/\*/g, "")
      .replace(/oe/g, "\u0153")
      .replace(/m\u0153ll/g, "moell")
      .replace(/c\u0153ff/g, "coeff")
      .replace(/' /g, "'")
      .replace(/\u2019/g, "'");
    // isShir = false in this path: punctuation-spacing regexes skipped.
  }
  if (s.startsWith("\u00b6 ")) s = s.slice(2);
  for (const marker of ["\u00b6", "\\par", "<p>"]) {
    s = s.split(marker).join("\n").split(" " + marker).join("\n");
  }
  for (const marker of ["</p>", "</h2>"]) s = s.split(marker).join("");
  return s.replace(/'"/g, "' \"").replace(/"'/g, "\" '").trim();
}

/** Local XML glues verse numbers in as superscripts; drop the leading runs. */
export function stripLeadingVerseNumber(text: string): string {
  let s = text.trimStart();
  for (;;) {
    const m = s.match(/^[¹²³⁰⁴-⁹]+/);
    if (!m) break;
    s = s.slice(m[0].length).trimStart();
  }
  return s.trimEnd();
}

// ---------------------------------------------------------------- builder

interface BookData {
  name: string;
  abbreviation?: string;
  /** chapter → verse → text */
  chapters: Map<number, Map<number, string>>;
}

class Builder {
  books = new Map<number, BookData>();

  book(number: number, name: string, abbreviation?: string): BookData {
    let b = this.books.get(number);
    if (!b) {
      const canonical = BOOKS[number - 1]?.key ?? "";
      b = { name: name.length > 0 ? name : canonical, chapters: new Map() };
      this.books.set(number, b);
    }
    if (b.abbreviation === undefined && abbreviation !== undefined) {
      b.abbreviation = abbreviation;
    }
    return b;
  }

  /** Dense chapters (missing verses = "") — the shape both ports share. */
  build(translationId: string, language?: string): Translation {
    const numbers = [...this.books.keys()].sort((a, b) => a - b);
    const books: TranslationBook[] = [];
    for (const n of numbers) {
      const b = this.books.get(n)!;
      const chapterNums = [...b.chapters.keys()].sort((a, b2) => a - b2);
      const maxChapter = chapterNums[chapterNums.length - 1] ?? 0;
      const rows: string[][] = [];
      for (let c = 1; c <= maxChapter; c++) {
        const vs = b.chapters.get(c);
        if (!vs) {
          rows.push([]);
          continue;
        }
        const verseNums = [...vs.keys()].sort((a, b2) => a - b2);
        const maxVerse = verseNums[verseNums.length - 1] ?? 0;
        const lines: string[] = [];
        for (let v = 1; v <= maxVerse; v++) lines.push(vs.get(v) ?? "");
        rows.push(lines);
      }
      const book: TranslationBook = { name: b.name, chapters: rows };
      if (b.abbreviation !== undefined) book.abbreviation = b.abbreviation;
      books.push(book);
    }
    return { translationId, language, books };
  }
}

function pushVerse(b: BookData, chapter: number, verse: number, text: string): void {
  let vs = b.chapters.get(chapter);
  if (!vs) {
    vs = new Map();
    b.chapters.set(chapter, vs);
  }
  if (!vs.has(verse)) vs.set(verse, text);
}

// --------------------------------------------------------------- dispatch

/**
 * Parse `xml`, sniff the root element, import. `fallbackName` is the name an
 * operator would have typed in the original's import dialog (used when the
 * file carries none of its own).
 */
export function importBibleXml(xml: string, fallbackName: string): Translation {
  const doc = parseXmlDoc(xml);
  switch (doc.name) {
    case "osis":
      return importOsis(doc, fallbackName);
    case "XMLBIBLE":
      return importZefania(doc, fallbackName);
    case "root":
      return importLocalXml(doc, fallbackName);
    default:
      throw new BibleImportError(`Unrecognised Bible XML root element: ${doc.name}`);
  }
}

// ----------------------------------------------------------------- OSIS

/** `osisID` → [book, chapter, verse]; split on `.` and space, first three. */
function parseOsisId(id: string): [number, number, number] | undefined {
  const parts = id.split(/[. ]/);
  if (parts.length < 3) return undefined;
  const idx = OSIS_BOOKS.indexOf(parts[0]);
  if (idx < 0) return undefined;
  const chapter = parseInt(parts[1], 10);
  const verse = parseInt(parts[2], 10);
  if (!Number.isFinite(chapter) || !Number.isFinite(verse)) return undefined;
  return [idx + 1, chapter, verse];
}

export function importOsis(doc: XmlElement, fallbackName: string): Translation {
  if (doc.name !== "osis") throw new BibleImportError("Missing 'osis' tag.");
  const osisText = child(doc, "osisText");
  if (!osisText) throw new BibleImportError("Missing 'osisText' tag");

  const language = attr(osisText, "xml:lang") ?? "";
  const work = attr(osisText, "osisIDWork") ?? "";
  const translationId = work.length > 0 ? work : fallbackName;

  const verses = descendants(osisText, "verse");
  if (verses.length === 0) throw new BibleImportError("No Bible books found.");

  const builder = new Builder();
  // The first verse of a parent carries any chapter text preceding it.
  const leadingTextBefore = (verse: XmlElement): string | undefined => {
    const parent = findParent(osisText, verse);
    if (!parent) return undefined;
    let out = "";
    for (const c of parent.children) {
      if ((c as XmlElement) === verse) break;
      if ((c as { text?: string }).text !== undefined) out += (c as { text: string }).text;
    }
    return out.length > 0 ? out : undefined;
  };

  for (const verse of verses) {
    const parsed = parseOsisId(attr(verse, "osisID") ?? "");
    if (!parsed) continue; // the original builds a junk book -1; we skip
    const [bookNo, chapter, verseNo] = parsed;
    const b = builder.book(bookNo, "", undefined);
    const chapterIsNew = !b.chapters.has(chapter);
    if (chapterIsNew && verseNo !== 1) {
      const intro = leadingTextBefore(verse);
      if (intro !== undefined) {
        const text = cleanOsisText(intro);
        if (text.length > 0) pushVerse(b, chapter, 1, text);
      }
    }
    let text = cleanOsisText(directText(verse));
    if (text.length === 0) {
      // Milestone style (<verse …/>text): take the run until the next
      // element so modern OSIS files import with content; container-style
      // files always hit the branch above.
      const parent = findParent(osisText, verse);
      if (parent) text = cleanOsisText(textRunAfter(parent, verse));
    }
    pushVerse(b, chapter, verseNo, text);
  }

  const t = builder.build(translationId, language);
  if (t.books.length === 0) throw new BibleImportError("No Bible books found.");
  t.source = "osis";
  return t;
}

function findParent(root: XmlElement, needle: XmlElement): XmlElement | undefined {
  for (const c of root.children) {
    if ((c as XmlElement) === needle) return root;
    if ((c as XmlElement).name !== undefined) {
      const hit = findParent(c as XmlElement, needle);
      if (hit) return hit;
    }
  }
  return undefined;
}

/** Milestone verse: the text run after it until the next element sibling. */
function textRunAfter(parent: XmlElement, verse: XmlElement): string {
  let out = "";
  let seen = false;
  for (const c of parent.children) {
    if ((c as XmlElement) === verse) {
      seen = true;
    } else if ((c as XmlElement).name !== undefined) {
      if (seen) break;
    } else if (seen) {
      out += (c as { text: string }).text;
    }
  }
  return out;
}

// --------------------------------------------------------------- Zefania

export function importZefania(doc: XmlElement, fallbackName: string): Translation {
  if (doc.name !== "XMLBIBLE") {
    throw new BibleImportError(
      'This is not a Zefania Bible: Missing "XMLBIBLE" tag.'
    );
  }
  const info = child(doc, "INFORMATION");
  const fromInfo = (key: string): string =>
    (info ? innerText(child(info, key) ?? { name: key, attrs: [], children: [] }) : "").trim();
  const languageRaw = fromInfo("language");
  const french = languageRaw === "FRE";
  const language = french ? "fr" : languageRaw;
  const title = fromInfo("title");
  const identifier = fromInfo("identifier");
  const translationId =
    identifier.length > 0 ? identifier : title.length > 0 ? title : fallbackName;

  const builder = new Builder();
  const books = childrenNamed(doc, "BIBLEBOOK");
  if (books.length === 0) throw new BibleImportError("No Bible books found.");
  let seqBook = 0;
  books.forEach((book, i) => {
    const bnumber = attr(book, "bnumber");
    const number = bnumber !== undefined && Number.isFinite(parseInt(bnumber, 10))
      ? Math.max(0, parseInt(bnumber, 10) - 1)
      : Math.max(seqBook, i);
    seqBook = number + 1;
    if (number >= 66) return; // the original skips out-of-range books
    const bname = attr(book, "bname") ?? "";
    const bsname = attr(book, "bsname");
    const b = builder.book(number + 1, bname, bsname);
    for (const chapter of childrenNamed(book, "CHAPTER")) {
      const cnumber = parseInt(attr(chapter, "cnumber") ?? "", 10);
      if (!Number.isFinite(cnumber)) continue;
      for (const verse of childrenNamed(chapter, "VERS")) {
        const vnumber = parseInt(attr(verse, "vnumber") ?? "", 10);
        if (!Number.isFinite(vnumber)) continue;
        pushVerse(b, cnumber, vnumber, adjustSpaces(innerText(verse), french));
      }
    }
  });

  const t = builder.build(translationId, language);
  if (t.books.length === 0) throw new BibleImportError("No Bible books found.");
  if (title.length > 0) t.title = title;
  const publisher = fromInfo("publisher");
  if (publisher.length > 0) t.publisher = publisher;
  const rights = fromInfo("rights");
  if (rights.length > 0) t.copyright = rights;
  const biblename = attr(doc, "biblename") ?? "";
  const description = biblename.length > 0 ? biblename : fromInfo("description");
  if (description.length > 0) t.description = description;
  t.source = "zefania";
  return t;
}

// --------------------------------------------------------- local per-book

export function importLocalXml(doc: XmlElement, fallbackName: string): Translation {
  const info = child(doc, "info");
  const fromInfo = (key: string): string =>
    (info ? innerText(child(info, key) ?? { name: key, attrs: [], children: [] }) : "").trim();
  const language = fromInfo("language");
  const named = fromInfo("translation");
  const translationId = named.length > 0 ? named : fallbackName;

  const builder = new Builder();
  const books = childrenNamed(doc, "book");
  if (books.length === 0) throw new BibleImportError("No Bible books found.");
  for (const book of books) {
    const name = attr(book, "name");
    if (!name) continue;
    const canonical = findBook(name);
    if (!canonical) continue; // unknown book: skip rather than invent one
    const number = BOOKS.findIndex((b) => b.key === canonical.key) + 1;
    if (number <= 0) continue;
    const b = builder.book(number, canonical.key, undefined);
    for (const chapter of childrenNamed(book, "chapter")) {
      const cid = parseInt(attr(chapter, "number") ?? "", 10);
      if (!Number.isFinite(cid)) continue;
      for (const passage of childrenNamed(chapter, "passage")) {
        const pid = parseInt(attr(passage, "number") ?? "", 10);
        if (!Number.isFinite(pid)) continue;
        pushVerse(b, cid, pid, stripLeadingVerseNumber(innerText(passage)));
      }
    }
  }
  if (builder.books.size === 0) throw new BibleImportError("No Bible books found.");

  const t = builder.build(translationId, language);
  if (t.books.length === 0) throw new BibleImportError("No Bible books found.");
  const copyright = fromInfo("copyright");
  if (copyright.length > 0) t.copyright = copyright;
  t.source = "videopsalm-xml";
  return t;
}
