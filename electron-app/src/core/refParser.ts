// Bible reference parser: "book chapter:verses" with diacritics-aware,
// punctuation-insensitive book matching (EN + FR names/abbreviations).
// Keyboard range adjust (Ctrl+*/+/-) modeled by expandRange/shrinkRange.

import type { BibleRef } from "./schemas";

interface BookAlias {
  key: string; // canonical english key
  names: string[]; // accepted spellings incl. FR + abbreviations
  chapters: number;
}

/**
 * The canon: 66 books with their English name, French name and abbreviations.
 *
 * Generated from `vp-core/src/bible_ref.rs`'s `BOOKS` so the two ports cannot
 * drift — they must resolve the same typed reference identically (AGENTS.md
 * rule 3). Chapter counts come from `vp-core`'s `chapter_count` table, which
 * is used to reject `John 99`.
 *
 * The `key` is the **canonical English name** and is what lands in the saved
 * `BibleRef`. It is the singular form used by vp-core (`Psalm`, not `Psalms`),
 * so an agenda written by one port resolves in the other.
 */
export const BOOKS: BookAlias[] = [
  { key: 'Genesis', chapters: 50, names: ['genesis', 'genèse', 'gen', 'gn', 'ge'] },
  { key: 'Exodus', chapters: 40, names: ['exodus', 'exode', 'ex', 'exo'] },
  { key: 'Leviticus', chapters: 27, names: ['leviticus', 'lévitique', 'lev', 'le', 'lv'] },
  { key: 'Numbers', chapters: 36, names: ['numbers', 'nombres', 'num', 'nb'] },
  { key: 'Deuteronomy', chapters: 34, names: ['deuteronomy', 'deutéronome', 'deut', 'dt', 'de'] },
  { key: 'Joshua', chapters: 24, names: ['joshua', 'josué', 'josh', 'jos'] },
  { key: 'Judges', chapters: 21, names: ['judges', 'juges', 'judg', 'jg'] },
  { key: 'Ruth', chapters: 4, names: ['ruth', 'ru', 'rt'] },
  { key: '1 Samuel', chapters: 31, names: ['1 samuel', '1 sam', '1 sa', '1s'] },
  { key: '2 Samuel', chapters: 24, names: ['2 samuel', '2 sam', '2 sa', '2s'] },
  { key: '1 Kings', chapters: 22, names: ['1 kings', '1 rois', '1 kgs', '1 r'] },
  { key: '2 Kings', chapters: 25, names: ['2 kings', '2 rois', '2 kgs', '2 r'] },
  { key: '1 Chronicles', chapters: 29, names: ['1 chronicles', '1 chroniques', '1 chron', '1 ch', '1 chr'] },
  { key: '2 Chronicles', chapters: 36, names: ['2 chronicles', '2 chroniques', '2 chron', '2 ch', '2 chr'] },
  { key: 'Ezra', chapters: 10, names: ['ezra', 'esdras', 'ezr', 'esd'] },
  { key: 'Nehemiah', chapters: 13, names: ['nehemiah', 'néhémie', 'neh', 'ne', 'nehem'] },
  { key: 'Esther', chapters: 10, names: ['esther', 'est', 'es'] },
  { key: 'Job', chapters: 42, names: ['job', 'jb'] },
  { key: 'Psalm', chapters: 150, names: ['psalm', 'psaume', 'ps', 'psa', 'pss', 'psaumes', 'psalms'] },
  { key: 'Proverbs', chapters: 31, names: ['proverbs', 'proverbes', 'prov', 'pr', 'pro'] },
  { key: 'Ecclesiastes', chapters: 12, names: ['ecclesiastes', 'ecclésiaste', 'eccl', 'ec', 'ecc'] },
  { key: 'Song of Solomon', chapters: 8, names: ['song of solomon', 'cantique des cantiques', 'song', 'sos', 'cant', 'sg'] },
  { key: 'Isaiah', chapters: 66, names: ['isaiah', 'ésaïe', 'isa', 'es', 'esaie'] },
  { key: 'Jeremiah', chapters: 52, names: ['jeremiah', 'jérémie', 'jer', 'jr', 'jeremie'] },
  { key: 'Lamentations', chapters: 5, names: ['lamentations', 'lam', 'la'] },
  { key: 'Ezekiel', chapters: 48, names: ['ezekiel', 'ézéchiel', 'ezek', 'ez', 'ezechiel'] },
  { key: 'Daniel', chapters: 12, names: ['daniel', 'dan', 'da', 'dn'] },
  { key: 'Hosea', chapters: 14, names: ['hosea', 'osée', 'hos', 'os', 'osee'] },
  { key: 'Joel', chapters: 3, names: ['joel', 'joël', 'jl', 'joe'] },
  { key: 'Amos', chapters: 9, names: ['amos', 'am'] },
  { key: 'Obadiah', chapters: 1, names: ['obadiah', 'abdias', 'obad', 'ab', 'ob'] },
  { key: 'Jonah', chapters: 4, names: ['jonah', 'jonas', 'jon'] },
  { key: 'Micah', chapters: 7, names: ['micah', 'michée', 'mic', 'mi', 'michee'] },
  { key: 'Nahum', chapters: 3, names: ['nahum', 'nah', 'na'] },
  { key: 'Habakkuk', chapters: 3, names: ['habakkuk', 'habakuk', 'hab', 'ha'] },
  { key: 'Zephaniah', chapters: 3, names: ['zephaniah', 'sophonie', 'zeph', 'so', 'sop'] },
  { key: 'Haggai', chapters: 2, names: ['haggai', 'aggée', 'hag', 'ag', 'aggee'] },
  { key: 'Zechariah', chapters: 14, names: ['zechariah', 'zacharie', 'zech', 'za', 'zac'] },
  { key: 'Malachi', chapters: 4, names: ['malachi', 'malachie', 'mal', 'ml'] },
  { key: 'Matthew', chapters: 28, names: ['matthew', 'matthieu', 'matt', 'mt', 'mat'] },
  { key: 'Mark', chapters: 16, names: ['mark', 'marc', 'mk', 'mr', 'mc'] },
  { key: 'Luke', chapters: 24, names: ['luke', 'luc', 'lk', 'lu', 'lc'] },
  { key: 'John', chapters: 21, names: ['john', 'jean', 'jn', 'joh', 'je'] },
  { key: 'Acts', chapters: 28, names: ['acts', 'actes', 'ac', 'act'] },
  { key: 'Romans', chapters: 16, names: ['romans', 'romains', 'rom', 'rm', 'ro'] },
  { key: '1 Corinthians', chapters: 16, names: ['1 corinthians', '1 corinthiens', '1 cor', '1 co'] },
  { key: '2 Corinthians', chapters: 13, names: ['2 corinthians', '2 corinthiens', '2 cor', '2 co'] },
  { key: 'Galatians', chapters: 6, names: ['galatians', 'galates', 'gal', 'ga'] },
  { key: 'Ephesians', chapters: 6, names: ['ephesians', 'éphésiens', 'eph', 'ep', 'ephesiens'] },
  { key: 'Philippians', chapters: 4, names: ['philippians', 'philippiens', 'phil', 'php', 'ph'] },
  { key: 'Colossians', chapters: 4, names: ['colossians', 'colossiens', 'col'] },
  { key: '1 Thessalonians', chapters: 5, names: ['1 thessalonians', '1 thessaloniciens', '1 thess', '1 th', '1 thes'] },
  { key: '2 Thessalonians', chapters: 3, names: ['2 thessalonians', '2 thessaloniciens', '2 thess', '2 th', '2 thes'] },
  { key: '1 Timothy', chapters: 6, names: ['1 timothy', '1 timothée', '1 tim', '1 ti', '1 timothee'] },
  { key: '2 Timothy', chapters: 4, names: ['2 timothy', '2 timothée', '2 tim', '2 ti', '2 timothee'] },
  { key: 'Titus', chapters: 3, names: ['titus', 'tite', 'tit', 'tt'] },
  { key: 'Philemon', chapters: 1, names: ['philemon', 'philémon', 'phlm', 'phm', 'filemon'] },
  { key: 'Hebrews', chapters: 13, names: ['hebrews', 'hébreux', 'heb', 'he', 'hebreux'] },
  { key: 'James', chapters: 5, names: ['james', 'jacques', 'jas', 'jc', 'jac', 'jm'] },
  { key: '1 Peter', chapters: 5, names: ['1 peter', '1 pierre', '1 pet', '1 p', '1 pi'] },
  { key: '2 Peter', chapters: 3, names: ['2 peter', '2 pierre', '2 pet', '2 p', '2 pi'] },
  { key: '1 John', chapters: 5, names: ['1 john', '1 jean', '1 jn', '1 j', '1 joh'] },
  { key: '2 John', chapters: 1, names: ['2 john', '2 jean', '2 jn', '2 j', '2 joh'] },
  { key: '3 John', chapters: 1, names: ['3 john', '3 jean', '3 jn', '3 j', '3 joh'] },
  { key: 'Jude', chapters: 1, names: ['jude', 'jud', 'jd'] },
  { key: 'Revelation', chapters: 22, names: ['revelation', 'apocalypse', 'rev', 'ap', 'apo', 'apoc'] },
];

export function normalizeBookName(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "") // strip diacritics (e.g. Genèse -> Genese)
    .toLowerCase()
    .replace(/[.\-_]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Match a book by any accepted spelling, then fall back to a **unique
 * prefix**. The prefix fallback is what makes `He` → Hebrews and `Jn` → John
 * work while typing, and it refuses to guess when the prefix is ambiguous
 * (`P` could be Psalms, Proverbs or Peter) so the caller can offer a choice
 * instead of silently resolving to the wrong book.
 */
export function findBook(input: string): BookAlias | undefined {
  const n = normalizeBookName(input);
  if (!n) return undefined;
  const exact = BOOKS.find((b) => b.names.some((a) => normalizeBookName(a) === n));
  if (exact) return exact;
  const byPrefix = BOOKS.filter((b) =>
    b.names.some((a) => normalizeBookName(a).startsWith(n))
  );
  return byPrefix.length === 1 ? byPrefix[0] : undefined;
}

function parseVerseList(spec: string): number[] {
  const out = new Set<number>();
  for (const part of spec.split(",")) {
    const p = part.trim();
    if (!p) continue;
    const m = p.match(/^(\d+)(?:\s*[-–]\s*(\d+))?$/);
    if (!m) throw new Error(`invalid verse spec: ${p}`);
    const a = parseInt(m[1], 10);
    const b = m[2] ? parseInt(m[2], 10) : a;
    if (a < 1 || b < a) throw new Error(`invalid verse range: ${p}`);
    for (let v = a; v <= b; v++) out.add(v);
  }
  return [...out].sort((x, y) => x - y);
}

/**
 * Parse `"John 3:16"`, `"Jean 3:16-18"`, `"Ps 23"`, `"Ps 23:1,3-4"`,
 * `"Romans 3:23-25, 30"`.
 *
 * §6.2 accepts **three** separators between chapter and verses — `:`, `.` and
 * `v` — and a verse list may carry spaces after the commas, because the
 * original's own placeholder is `Romans 3:23-25, 30`. Both were missing: the
 * regex only allowed `:vV`, and `.` in a reference was silently unsupported.
 *
 * The book is split off by trying each whitespace boundary from the right and
 * keeping the first that yields a known book plus a reference-shaped tail,
 * which is the same rule `vp-core::bible_ref` uses. Keeping the two ports'
 * parsers in step is required by AGENTS.md rule 3 — the same typed string must
 * resolve identically in both.
 */
export function parseBibleRef(
  text: string,
  translationId = "sample",
  versification: BibleRef["versification"] = "EN"
): BibleRef {
  const input = text.trim();
  // Right-to-left over the whitespace boundaries: `Romans 3:23-25, 30` has to
  // split at the space after the comma, not at the space before `3`.
  const parts = [...input.matchAll(/\s+/g)].map((m) => m.index!);
  for (let i = parts.length - 1; i >= 0; i--) {
    const at = parts[i];
    const rawBook = input.slice(0, at).trim();
    const tail = input.slice(at).trim();
    if (!rawBook || !tail) continue;
    // The tail must be a chapter, optionally followed by verses.
    const m = tail.match(/^(\d+)(?:\s*[:.vV]\s*(.*))?$/);
    if (!m) continue;
    const book = findBook(rawBook);
    if (!book) continue;
    const chapter = parseInt(m[1], 10);
    if (chapter < 1 || chapter > book.chapters) {
      throw new Error(`chapter out of range: ${chapter} (${book.key} has ${book.chapters})`);
    }
    const verses = m[2] ? parseVerseList(m[2]) : [];
    return { translationId, book: book.key, chapter, verses, versification };
  }
  // No book+chapter split worked: the whole string may still be a bare book
  // name, which the original also accepts (whole book).
  const book = findBook(input);
  if (book) {
    return { translationId, book: book.key, chapter: 1, verses: [], versification };
  }
  throw new Error(`cannot parse reference: ${text}`);
}

/** Expand the verse range by n on the end (Ctrl+* behavior). */
export function expandRange(ref: BibleRef, n = 1): BibleRef {
  if (ref.verses.length === 0) return { ...ref, verses: [1] };
  const last = Math.max(...ref.verses);
  const add: number[] = [];
  for (let v = last + 1; v <= last + n; v++) add.push(v);
  return { ...ref, verses: [...ref.verses, ...add] };
}

/** Shrink the verse range by n from the end (Ctrl+/ behavior, min 1 verse). */
export function shrinkRange(ref: BibleRef, n = 1): BibleRef {
  if (ref.verses.length <= 1) return { ...ref };
  return { ...ref, verses: ref.verses.slice(0, Math.max(1, ref.verses.length - n)) };
}

export function formatRef(ref: BibleRef): string {
  const base = `${ref.book} ${ref.chapter}`;
  return ref.verses.length ? `${base}:${ref.verses.join(",")}` : base;
}
