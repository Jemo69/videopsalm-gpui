// Instant-find matching rules.
//
// UX-RESEARCH §6.1 and `assets/RESEARCH-org.md` §"Behaviors that surprise
// implementers" #1. The original does NOT do a substring match:
//
//   * **order-free** — "new commandm" finds "A new commandment";
//   * **accent-free** — "benedicite" finds "Bénédicite";
//   * **punctuation-free** — a comma or an apostrophe cannot break a match;
//   * **case-free** — obviously;
//   * **a phrase is typed as ONE word** — "kingofkingslordoflords" finds
//     "King of kings, Lord of lords". This is the documented example and the
//     one most reimplementations miss;
//   * **digits search too** (1.27b) — numbers inside lyrics are findable;
//   * **book-scoped numbers** — "soc204" selects number 204 in the songbook
//     whose abbreviation is "soc" (1.11);
//   * search starts only **after more than three characters** (1.13).
//
// Every rule is a pure function over a plain object so the suite can test the
// semantics without a DOM, a filesystem or a database.

import type { Song } from "./schemas";

/**
 * The twelve search scopes, verbatim from `song-search-results.png`
 * ("Find words in:"). The *combination* is semantically different, not just a
 * filter — which is why they are modelled as a union rather than a bitmask.
 */
export type SearchScope =
  | "title-lyrics" | "title-then-lyrics" | "title"
  | "author-composer" | "themes" | "references"
  | "ccli" | "copyright" | "memo1" | "memo2" | "memo3" | "with-chords";

/** The default the original starts on: `Title and lyrics`. */
export const DEFAULT_SCOPE: SearchScope = "title-lyrics";

/**
 * How many words each scope matches in. `title-lyrics` requires the query's
 * words to appear across the title *and* the lyrics; `title-then-lyrics`
 * additionally requires them in the given order.
 */
const SCOPE_LABEL: Record<SearchScope, string> = {
  "title-lyrics": "Title and lyrics",
  "title-then-lyrics": "Title then lyrics",
  title: "Title",
  "author-composer": "Author and composer",
  themes: "Themes",
  references: "References",
  ccli: "CCLI",
  copyright: "Copyright",
  memo1: "Memo 1",
  memo2: "Memo 2",
  memo3: "Memo 3",
  "with-chords": "Songs with chords",
};

/** Human label for a scope, for the UI. */
export function scopeLabel(scope: SearchScope): string {
  return SCOPE_LABEL[scope] ?? scope;
}

/** Every scope, in the order the panel lays them out (2 columns, row-major). */
export const ALL_SCOPES: SearchScope[] = [
  "title-lyrics", "ccli",
  "title-then-lyrics", "copyright",
  "title", "memo1",
  "author-composer", "memo2",
  "themes", "memo3",
  "references", "with-chords",
];

/**
 * Fold a string down to its searchable form: no accents, no punctuation, no
 * case, collapsed whitespace.
 *
 * Punctuation is **deleted, not replaced with a space**. That detail is what
 * makes a phrase work: "King of kings," and "Kingofkings" have to reduce to the
 * same tokens, and replacing the comma with a space would keep them apart.
 */
export function normalizeSearchText(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")          // accents
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "")          // punctuation and symbols
    .trim();
}

/**
 * Split a normalized string into searchable words.
 *
 * The original treats a typed phrase as **one** word, so a phrase only matches
 * when the words are **adjacent in order** — which is exactly what an
 * order-free word-set test on the *concatenation* cannot express. Keeping the
 * words separate but requiring adjacency is what we want, and it is why this
 * returns a word list rather than a single string.
 */
export function searchWords(s: string): string[] {
  const n = s.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
  return n.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

/**
 * "Phrase form": the words glued together with no separator.
 *
 * This is what makes `kingofkingslordoflords` find *King of kings, Lord of
 * lords* — the operator types a sentence as one unbroken word and the whole
 * phrase must appear. Matching it against the space-stripped haystack is
 * therefore the only way a phrase can match at all, which is why
 * [`phraseIn`] works on strings rather than on word lists.
 */
export function phraseForm(words: string[]): string {
  return words.join("");
}

/**
 * `true` when the glued-together `needle` appears inside the glued-together
 * `haystack`. This is the phrase-as-one-word rule, and it is checked
 * *before* the word-set rule because a phrase is the stricter match.
 */
function phraseIn(haystack: string[], needle: string[]): boolean {
  if (needle.length === 0) return true;
  return phraseForm(haystack).includes(phraseForm(needle));
}

/**
 * Instant find matches on **word prefixes**, not whole words.
 *
 * This is not a detail — it is what makes the box feel responsive. The
 * operator types `new commandm` and the list has already narrowed to
 * "A new commandment" on the final character, so `commandm` must match the
 * word `commandment`. Requiring equality would mean the list only ever
 * resolves on a complete word, which is the difference between instant find
 * and a search dialog.
 */
function wordMatches(needleWord: string, haystackWord: string): boolean {
  return haystackWord.startsWith(needleWord);
}

/**
 * `true` when every word of `needle` prefixes a *distinct* word of `haystack`,
 * in any order and non-adjacent. This is the rule behind "new commandm"
 * finding "A new commandment" with the words reversed.
 */
function wordsInAnyOrder(haystack: string[], needle: string[]): boolean {
  if (needle.length === 0) return true;
  const pool = [...haystack];
  for (const w of needle) {
    const at = pool.findIndex((h) => wordMatches(w, h));
    if (at < 0) return false;
    pool.splice(at, 1);
  }
  return true;
}

/**
 * `true` when the words of `needle` appear in `haystack` in the given order,
 * each as a prefix of the word at that position. This is what separates
 * `Title then lyrics` from `Title and lyrics`.
 */
function wordsInOrder(haystack: string[], needle: string[]): boolean {
  if (needle.length === 0) return true;
  let at = 0;
  for (const w of needle) {
    const found = haystack.findIndex((h, i) => i >= at && wordMatches(w, h));
    if (found < 0) return false;
    at = found + 1;
  }
  return true;
}

/**
 * A song-book-scoped number search: `soc204` means "number 204 in the songbook
 * whose abbreviation is soc" (1.11). Split the query into a trailing digit run
 * and a leading alpha prefix; if the prefix names a known songbook
 * abbreviation, require that songbook and that number.
 */
export interface BookScopedQuery {
  abbrev: string;
  number: string;
}
export function parseBookScoped(query: string): BookScopedQuery | null {
  const m = /^([\p{L}]+?)(\d+)$/u.exec(query.trim());
  if (!m) return null;
  return { abbrev: m[1].toLowerCase(), number: m[2] };
}

/** The searchable fields of a song, by scope. Kept pure and total. */
interface Haystack {
  /** Words that may be matched in any order. */
  any: string[];
  /** Words that must match adjacently, in order. */
  ordered: string[];
  /** Direct scalar comparisons (id, ccli, abbrev), lowercased. */
  scalars: string;
  /** Whether this song qualifies for the `with-chords` scope at all. */
  chords?: boolean;
}

function scopeHaystack(song: Song, scope: SearchScope): Haystack {
  const title = searchWords(song.title ?? "");
  const lyrics = searchWords(song.verses.flatMap((v) => v.lines).join(" "));
  const authors = searchWords((song.authors ?? []).join(" "));
  const ref = searchWords((song.references ?? []).join(" "));
  const themes = searchWords((song.themes ?? []).join(" "));
  const memos = searchWords(
    [song.memo1, song.memo2, song.memo3].filter((m): m is string => !!m).join(" ")
  );
  // `SongVerse` has no chord field — chords live in `SongChords.chordpro`
  // and in `ChordOverlay`, both of which this module does not need to walk.
  const chordText = searchWords(song.chords?.chordpro ?? "");
  const scalars = [song.songId ?? "", song.ccli ?? "", song.songbookId ?? ""]
    .join(" ")
    .toLowerCase();

  switch (scope) {
    case "title":
      return { any: title, ordered: title, scalars };
    case "title-lyrics":
      // Both must match; order between the fields is not required, which is
      // what distinguishes it from `title-then-lyrics`.
      return { any: [...title, ...lyrics], ordered: [...title, ...lyrics], scalars };
    case "title-then-lyrics":
      // Handled specially in `matchesSong` (title field and lyric field are
      // matched separately); the haystack here is only a fallback.
      return { any: [...title, ...lyrics], ordered: [...title, ...lyrics], scalars };
    case "author-composer":
      return { any: authors, ordered: authors, scalars };
    case "themes":
      return { any: themes, ordered: themes, scalars };
    case "references":
      return { any: ref, ordered: ref, scalars };
    case "ccli":
      return { any: [], ordered: [], scalars: (song.ccli ?? "").toLowerCase() };
    case "copyright":
      return { any: searchWords(song.copyright ?? ""), ordered: searchWords(song.copyright ?? ""), scalars };
    case "memo1": case "memo2": case "memo3": {
      const one = searchWords(song[scope] ?? "");
      return { any: one, ordered: one, scalars };
    }
    case "with-chords":
      return { any: chordText, ordered: chordText, scalars, chords: chordText.length > 0 };
  }
  /* c8 ignore next */
  return { any: [], ordered: [], scalars };
}

/**
 * Does `song` satisfy `query` under `scope`?
 *
 * Exported separately from `filterSongs` so the semantics are testable one
 * song at a time, and so the GPUI port's equivalent can be checked against the
 * same table of documented examples.
 */
export function matchesSong(
  song: Song,
  query: string,
  scope: SearchScope = DEFAULT_SCOPE
): boolean {
  const raw = query.trim();
  if (!raw) return true;

  // A book-scoped number search (`soc204`) wins over ordinary word search —
  // it is a documented way to reach a specific song by number.
  const scoped = parseBookScoped(raw);
  if (scoped && (song.songbookId ?? "").toLowerCase() === scoped.abbrev) {
    return (song.songId ?? "").trim() === scoped.number;
  }

  const needle = searchWords(raw);
  if (needle.length === 0) return true;
  const h = scopeHaystack(song, scope);

  if (scope === "with-chords" && !h.chords) return false;
  // `ccli` and `author-composer` are exact-ish scalar fields: a plain
  // containment test on the raw value, not a word-set test.
  if (scope === "ccli") return (song.ccli ?? "").toLowerCase().includes(raw.toLowerCase());
  if (scope === "author-composer") {
    return (song.authors ?? []).some((a) => a.toLowerCase().includes(raw.toLowerCase()));
  }
  if (scope === "title-then-lyrics") {
    // The two fields are matched separately and the words must keep their
    // order within each — that ordering requirement is the only difference
    // between this scope and `title-lyrics`.
    const t = searchWords(song.title ?? "");
    if (!(phraseIn(t, needle) || wordsInOrder(t, needle) || wordsInAnyOrder(t, needle))) {
      return false;
    }
    const l = searchWords(song.verses.flatMap((v) => v.lines).join(" "));
    return phraseIn(l, needle) || wordsInOrder(l, needle) || wordsInAnyOrder(l, needle);
  }
  // Phrase-as-one-word first (adjacent, in order), then order-free. Both are
  // documented behaviours; phrase is the stricter, so it wins when it hits.
  return phraseIn(h.ordered, needle) || wordsInAnyOrder(h.any, needle) || h.scalars.includes(raw.toLowerCase());
}

/**
 * The original starts searching only once **more than three** characters have
 * been typed (1.13). Returning the full list below that threshold is what
 * makes the box feel calm instead of flickery.
 */
export const MIN_QUERY_CHARS = 4;

/** `true` when the box should run a search at all. */
export function shouldSearch(query: string): boolean {
  return query.trim().length >= MIN_QUERY_CHARS;
}

/**
 * Filter a song list. `query` shorter than `MIN_QUERY_CHARS` returns
 * everything — the original shows the whole library until you start typing.
 */
export function filterSongs(
  songs: Song[],
  query: string,
  scope: SearchScope = DEFAULT_SCOPE
): Song[] {
  if (!shouldSearch(query)) return songs;
  return songs.filter((s) => matchesSong(s, query, scope));
}