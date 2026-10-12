// Bible translation store: resolve / keyword search / context / registry.
//
// Clean-room port of `gpui-app/vp-core/src/bible_data.rs`. The GPUI side is
// the reference implementation of the original's **third Bible search mode**
// (the magnifier swaps the reference box for a keyword box), so the two ports
// must agree on what matches — AGENTS.md rule 3 in spirit: same input, same
// hits, same order.
//
// Two JSON shapes are accepted because the two ports ship their sample
// translation differently and both must keep loading:
//   * nested — `{translationId, language, books:[{name, chapters:[[text]]}]}`
//     (`gpui-app/assets/samples/bible/*.json`)
//   * flat   — `{translationId, verses:[{book, chapter, verse, text}]}`
//     (`electron-app/data/bible.sample.json`)

import { BOOKS, findBook } from "./refParser";
import type { BibleRef } from "./schemas";

export interface TranslationBook {
  name: string;
  /** `chapters[c][v]` is chapter c+1, verse v+1. */
  chapters: string[][];
  /** `bsname` from an imported Zefania file (the original's abbreviation). */
  abbreviation?: string;
}

export interface Translation {
  translationId: string;
  language?: string;
  books: TranslationBook[];
  // Metadata carried from the import dialogs (Zefania INFORMATION, local
  // XML info) — mirrors the optional fields on the Rust `Translation` so
  // both ports write the same JSON shape.
  title?: string;
  abbreviation?: string;
  publisher?: string;
  copyright?: string;
  description?: string;
  /** `"osis" | "zefania" | "videopsalm-xml"` — the original's BibleSource. */
  source?: string;
}

/** One verse returned by [`keywordSearch`]. */
export interface VerseHit {
  book: string;
  chapter: number;
  verse: number;
  text: string;
}

/**
 * Fold case + Latin diacritics, drop punctuation/hyphens, collapse spaces.
 *
 * `vp-core/src/bible_ref.rs::normalize` is the shared normalizer behind
 * diacritics-aware book matching and punctuation-insensitive search; this is
 * its port (NFD strips the combining marks the Rust table folds by hand).
 */
export function normalize(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/ß/g, "s")
    .replace(/œ/g, "o")
    .replace(/æ/g, "a")
    .toLowerCase()
    .replace(/['\u2019\u2018.,;:!?"()[\]]/g, " ")
    .replace(/[-_/]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** Canonical book number (1..66) for any accepted spelling, 0 when unknown. */
function bookNumber(name: string): number {
  const hit = findBook(name);
  if (!hit) return 0;
  return BOOKS.findIndex((b) => b.key === hit.key) + 1;
}

/** Flat sample shape → the nested shape both ports share. */
function fromFlat(doc: {
  translationId?: string;
  language?: string;
  name?: string;
  verses?: { book: string; chapter: number; verse: number; text: string }[];
}): Translation {
  const books = new Map<string, Map<number, string[]>>();
  for (const v of doc.verses ?? []) {
    const chapters = books.get(v.book) ?? new Map<number, string[]>();
    const arr = chapters.get(v.chapter) ?? [];
    // Verse numbers are 1-based and dense in the samples; pad so the index
    // lines up exactly as the nested shape requires.
    while (arr.length < v.verse - 1) arr.push("");
    arr[v.verse - 1] = v.text;
    chapters.set(v.chapter, arr);
    books.set(v.book, chapters);
  }
  const out: TranslationBook[] = [];
  for (const [name, chapters] of books) {
    const nums = [...chapters.keys()].sort((a, b) => a - b);
    const rows: string[][] = [];
    for (let c = 1; c <= (nums[nums.length - 1] ?? 0); c++) {
      rows.push(chapters.get(c) ?? []);
    }
    out.push({ name, chapters: rows });
  }
  return {
    translationId: doc.translationId ?? "sample",
    language: doc.language,
    books: out,
  };
}

/** Parse either sample shape. Throws on a document with no verses at all. */
export function translationFromJson(doc: unknown): Translation {
  const d = doc as Record<string, unknown>;
  if (Array.isArray(d["books"])) return doc as Translation;
  if (Array.isArray(d["verses"])) return fromFlat(d as never);
  throw new Error("not a translation document");
}

/** Look up chapter verses by book name (EN/FR/abbreviation). */
export function chapter(t: Translation, book: string, ch: number): string[] | undefined {
  const wanted = findBook(book);
  if (!wanted) return undefined;
  const hit = t.books.find((b) => findBook(b.name)?.key === wanted.key);
  if (!hit) return undefined;
  return hit.chapters[ch - 1];
}

/**
 * Resolve a `BibleRef` to display lines (`"16 text…"`, verses past the
 * chapter as `[…]`, and verses that never existed in the source — an OSIS
 * gap — omitted entirely rather than shown with the wrong text; parity with
 * `bible_data.rs::resolve`). An empty verse list means "the whole chapter".
 */
export function resolve(t: Translation, r: BibleRef): string[] {
  const ch = chapter(t, r.book, r.chapter);
  const verses: number[] =
    r.verses.length > 0 ? r.verses : ch ? ch.map((_, i) => i + 1) : [];
  const out: string[] = [];
  for (const v of verses) {
    const text = ch?.[v - 1];
    if (text === undefined) {
      out.push(`${v} […]`);
    } else if (text.length > 0) {
      out.push(`${v} ${text}`);
    }
  }
  return out;
}

/**
 * Find verses containing specific words or a phrase — the original's third
 * Bible search mode. Every whitespace-separated word must appear in the
 * verse (AND), punctuation- and diacritics-insensitive via `normalize`.
 * Results come back in canonical book/chapter/verse order.
 */
export function keywordSearch(t: Translation, words: string): VerseHit[] {
  const terms = normalize(words).split(" ").filter(Boolean);
  if (terms.length === 0) return [];
  const out: VerseHit[] = [];
  for (const book of t.books) {
    book.chapters.forEach((chapterVerses, cPos) => {
      chapterVerses.forEach((text, vPos) => {
        const hay = normalize(text);
        if (terms.every((term) => hay.includes(term))) {
          out.push({
            book: book.name,
            chapter: cPos + 1,
            verse: vPos + 1,
            text,
          });
        }
      });
    });
  }
  out.sort(
    (a, b) =>
      bookNumber(a.book) - bookNumber(b.book) ||
      a.chapter - b.chapter ||
      a.verse - b.verse
  );
  return out;
}

/** Surrounding-verse context (SPEC 2.3): `before`/`after` extra verses. */
export function context(
  t: Translation,
  r: BibleRef,
  before: number,
  after: number
): string[] {
  const wide: BibleRef = { ...r, verses: [...r.verses] };
  if (wide.verses.length > 0) {
    const lo = Math.max(1, Math.min(...wide.verses) - before);
    const hi = Math.max(...wide.verses) + after;
    wide.verses = [];
    for (let v = lo; v <= hi; v++) wide.verses.push(v);
  }
  return resolve(t, wide);
}

/** Registry of installed translations with favorites (SPEC 2.3). */
export class BibleLibrary {
  private translations = new Map<string, Translation>();
  favorites: string[] = [];
  currentId?: string;

  install(t: Translation): void {
    if (this.currentId === undefined) this.currentId = t.translationId;
    this.translations.set(t.translationId, t);
  }

  current(): Translation | undefined {
    return this.currentId !== undefined
      ? this.translations.get(this.currentId)
      : undefined;
  }

  /** Ids of every installed translation, in install order. */
  ids(): string[] {
    return [...this.translations.keys()];
  }

  /** One-click translation switch: false when unknown. */
  switchTo(id: string): boolean {
    if (this.translations.has(id)) {
      this.currentId = id;
      return true;
    }
    return false;
  }

  addFavorite(id: string): void {
    if (this.translations.has(id) && !this.favorites.includes(id)) {
      this.favorites.push(id);
    }
  }

  removeFavorite(id: string): void {
    this.favorites = this.favorites.filter((f) => f !== id);
  }
}

/**
 * Display rows for Bible keyword hits: `(reference, snippet)` pairs.
 *
 * Port of `app/src/ux.rs::bible_hit_rows`, pure for the same reason: every
 * hit in gets exactly one row out — the render layer must never `.take(n)`
 * them (R-6: the old 30-row cap silently dropped matches).
 */
export function bibleHitRows(hits: VerseHit[]): [string, string][] {
  return hits.map((h) => [
    `${h.book} ${h.chapter}:${h.verse}`,
    [...h.text].slice(0, 70).join(""),
  ]);
}
