// Shared JSON schemas — SPEC section 4. Both targets MUST share these shapes.
// Clean-room implementation: field names/types only, no proprietary code.
//
// AGENTS.md rule 3 ("one schema, two ports")
// ------------------------------------------
// `gpui-app/vp-core/src/schema.rs` is the reference definition of every
// literal in this file, and the cross-port fixture at
// `tests/fixtures/cross-port-schema.json` is parsed by **both** ports in
// tests (`schema.rs`'s `#[cfg(test)]` block and `tests/crossPort.test.ts`).
// Adding a literal to one port without the other fails a test rather than a
// user's file.
//
// | concept            | canonical literals                                             |
// |--------------------|----------------------------------------------------------------|
// | `ChordNotation`    | `ABC`, `DoReMi`                                                |
// | `FlowType`         | `default`, `manual`, `auto`, `AutoAdvance`, `Loop`              |
// | `AgendaItemKind`   | the 17 literals in `AGENDA_KINDS`                              |
// | `Versification`    | `EN`, `RU`, `FR`                                               |
// | `StyleBody.align`  | `left`, `center`, `right`                                      |
// | `BackgroundKind`   | `color`, `image`, `videoLoop`, `camera`                        |
// | `Style.transition` | `none`, `fade`, `slide`, `zoom`                                |
//
// Legacy spellings stay **accepted on read** (see `CHORD_NOTATION_ALIASES`
// and `FLOW_TYPE_ALIASES`); nothing is renamed, so files already on disk
// keep opening. `chordTranspose.ts` still uses `latin`/`doremi`
// internally, which is why those aliases exist at all.

export interface SongVerse {
  id: string;
  lines: string[];
  order?: number;
  /**
   * Native VideoPsalm verse label: `V`, `C`, `P`, `B`, `T`, `E`, `O`, `S`,
   * `I`, or `R`. Absent on songs that never carried a native tag.
   */
  tag?: string;
  /** Native VideoPsalm verse number (`V1` → tag `V`, tagId `1`). */
  tagId?: number;
}

export interface ChordOverlay {
  verseId: string;
  line: number;
  chord: string;
  pos: number;
}

/**
 * Chord notation as it appears on the wire. Canonical output is always
 * `ABC` / `DoReMi`; `latin` / `doremi` are accepted for backward
 * compatibility (see module header).
 */
export type ChordNotation = "ABC" | "DoReMi" | "latin" | "doremi";

/** The canonical spellings, in Rust's declaration order. */
export const CHORD_NOTATIONS: readonly ChordNotation[] = ["ABC", "DoReMi"];

/** Legacy spellings and the canonical literal each normalises to. */
export const CHORD_NOTATION_ALIASES: Readonly<Record<string, ChordNotation>> = {
  latin: "ABC",
  Latin: "ABC",
  doremi: "DoReMi",
  DoReMiSolfege: "DoReMi",
};

/** Normalise any accepted notation literal to its canonical spelling. */
export function canonicalChordNotation(n: ChordNotation): "ABC" | "DoReMi" {
  return (CHORD_NOTATION_ALIASES[n] ?? n) as "ABC" | "DoReMi";
}

export function isChordNotation(n: string): n is ChordNotation {
  return n in CHORD_NOTATION_ALIASES || (CHORD_NOTATIONS as readonly string[]).includes(n);
}

export interface SongChords {
  notation: ChordNotation;
  transpose: number; // semitones applied to base
  overlays: ChordOverlay[];
  chordpro?: string; // raw ChordPro source when imported as such
}

/**
 * Where a song came from. Only the native VideoPsalm format uses this today:
 * the file path lets the main process write lyric edits straight back into
 * the operator's real `Songbooks/*.json` file, while `songbookGuid`/`guid`
 * identify the song inside the (unknown-field-preserving) native AST.
 */
export interface SongSource {
  format: "videopsalm";
  file: string;
  songbookGuid: string;
  songbookName?: string;
  songId?: string;
  guid?: string;
}

export interface Song {
  id: string;
  title: string;
  verses: SongVerse[];
  authors: string[];
  composer?: string;
  ccli?: string;
  songId?: string; // up to 8 digits
  songbookId?: string;
  color?: string;
  icon?: string;
  chords?: SongChords;
  /**
   * Native VideoPsalm `Sequence` / `VerseOrderIndex`. `sequence` is a
   * comma-separated list of alternative verse orders (`V1 C V2 C`), and the
   * index selects one. A missing index (-1 in VideoPsalm) means document
   * order; the fields are preserved even when no order is selected.
   */
  sequence?: string;
  verseOrderIndex?: number;
  /**
   * Searchable metadata. Every one of these is a **named scope** in the
   * original's "Find words in:" panel (`song-search-results.png`) — twelve
   * scopes, of which these seven are per-song fields. They are optional
   * because most songs do not carry all of them.
   */
  copyright?: string;   // scope: Copyright
  themes?: string[];    // scope: Themes
  references?: string[];// scope: References (e.g. "Isaiah 53:5")
  memo1?: string;       // scope: Memo 1
  memo2?: string;       // scope: Memo 2
  memo3?: string;       // scope: Memo 3
  createdAt: string;
  /** Emitted by `vp-core`; undeclared here until now, which is why a song
   *  re-saved by Rust could not be typed on this side. */
  updatedAt: string;
  /** Native VideoPsalm origin (write-through editing). */
  source?: SongSource;
}

export interface BibleRef {
  translationId: string;
  /**
   * Canonical **singular** English book key, e.g. `"John"` / `"Psalm"`.
   * Generated from `vp-core/src/bible_ref.rs`'s `BOOKS` (see `refParser.ts`),
   * so both ports write `"Psalm"`, not `"Psalms"`. The plural still resolves
   * through `findBook()` as an accepted abbreviation, which is what keeps
   * `data/bible.sample.json` (stored under `"Psalms"`) loadable.
   */
  book: string;
  chapter: number;
  verses: number[];
  versification?: Versification;
}

/** Versification system switch (SPEC 2.3). Mirrors Rust `Versification`. */
export type Versification = "EN" | "RU" | "FR";

export const VERSIFICATIONS: readonly Versification[] = ["EN", "RU", "FR"];

export type AgendaItemKind =
  | "Song"
  | "BibleVerses"
  | "Bible"
  | "Image"
  | "Video"
  | "Audio"
  | "Pdf"
  | "PowerPoint"
  | "PowerPointSlide"
  | "SongPowerPoint"
  | "Excel"
  | "Word"
  | "YouTube"
  | "WebSite"
  | "CCLI"
  | "Alert"
  | "Clock";

/**
 * How an item leaves the screen.
 *
 * Canonical output is `default` / `manual` / `auto` / `AutoAdvance` / `Loop`.
 * `AutoAdvance` and `Loop` were declared here and missing from Rust, which
 * made an Electron agenda fail to open in the GPUI port; they are now in both.
 * The capitalised TS-only spellings (`Manual`, `Auto`) survive as accepted
 * aliases because `data/agenda.sample.json` and older saves use them.
 */
export type FlowType =
  | "default"
  | "manual"
  | "auto"
  | "AutoAdvance"
  | "Loop"
  | "Default"
  | "Manual"
  | "Auto"
  | "autoAdvance"
  | "loop";

/** The canonical spellings, in Rust's declaration order. */
export const FLOW_TYPES: readonly FlowType[] = [
  "default",
  "manual",
  "auto",
  "AutoAdvance",
  "Loop",
];

/** Legacy spellings and the canonical literal each normalises to. */
export const FLOW_TYPE_ALIASES: Readonly<Record<string, FlowType>> = {
  Default: "default",
  Manual: "manual",
  Auto: "auto",
  autoAdvance: "AutoAdvance",
  loop: "Loop",
};

/** Normalise any accepted flow type to its canonical spelling. */
export function canonicalFlowType(f: FlowType): (typeof FLOW_TYPES)[number] {
  return (FLOW_TYPE_ALIASES[f] ?? f) as (typeof FLOW_TYPES)[number];
}

export function isFlowType(f: string): f is FlowType {
  return f in FLOW_TYPE_ALIASES || (FLOW_TYPES as readonly string[]).includes(f);
}

export interface AgendaItem {
  id: string;
  kind: AgendaItemKind;
  title: string;
  ref?: {
    file?: string;
    songId?: string;
    bibleRef?: BibleRef;
    url?: string;
    text?: string;
  };
  flowType?: FlowType;
  /** Rust emits explicit `null` when no auto-advance is set; accept it as well as absent. */
  autoAdvanceMs?: number | null;
  overrides?: Record<string, unknown>;
}

/** A songbook the library can file songs under. Mirrors Rust `Songbook`. */
export interface Songbook {
  id: string;
  name: string;
  abbrev: string;
  readOnly: boolean;
}

export interface Agenda {
  version: 1;
  title: string;
  items: AgendaItem[];
  settings?: Record<string, unknown>;
}

export type Align = "left" | "center" | "right";

/** Vertical placement of the lyric block in its frame. Mirrors Rust `VAlign` (`top`/`center`/`bottom`, default `center`, key skipped on write when center). */
export type VAlign = "top" | "center" | "bottom";

export const VALIGNS: readonly VAlign[] = ["top", "center", "bottom"];

export function isVAlign(s: string): s is VAlign {
  return (VALIGNS as readonly string[]).includes(s);
}

export const ALIGNS: readonly Align[] = ["left", "center", "right"];

/**
 * The normalized space a slide frame is authored in: `0..=10000`.
 *
 * Mirrors `NORMALIZED_SPACE` in `gpui-app/vp-core/src/schema.rs`. The divisor
 * is 10000, not 1000 — the vendor's own style explorer prints
 * `Recta… 0;10000;9300`, `0;10000;700` (`OBSERVATIONS.md` §8). Our earlier
 * `UX-RESEARCH.md` §5.4 note of 0–1000 was wrong; the screenshot wins.
 */
export const NORMALIZED_SPACE = 10_000;

/**
 * The space a **text size** lives in: `0..=1000`.
 *
 * Deliberately different from `NORMALIZED_SPACE`, because the vendor uses two
 * different integer spaces for two different things and reading either as the
 * other makes a documented default unusable. Frames are `0..=10000`; text
 * sizes are `0..=1000`, because changelog 1.26's *"default is now 70 (100 was
 * too big)"* only reads correctly if 70 is 7% of the mapped edge — 75.6 px on
 * a 1080p projector. Divided by `NORMALIZED_SPACE` it would be 7.56 px, which
 * nobody in the room can read.
 */
export const TEXT_SIZE_SPACE = 1_000;

/**
 * A slide frame rectangle in the normalized `NORMALIZED_SPACE` square.
 *
 * Mirrors `Rect` in `vp-core/src/schema.rs`. Field names `x`, `y`, `w`, `h` and
 * their order are the vendor's, as are the vendor's own printed values.
 *
 * Integer-typed on purpose: the vendor prints whole numbers, and
 * *out-of-range values are clamped, not rejected*, so a hand-edited library
 * file still opens. Resolution to pixels lives in the geometry module; this is
 * pure data, so neither port resolves geometry while deserializing.
 */
export interface Rect {
  /** Left edge, 0 = flush to the frame's left. */
  x: number;
  /** Top edge, 0 = flush to the frame's top. */
  y: number;
  /** Width. `0` means "derive from `h`". */
  w: number;
  /** Height. `0` means "derive from `w`". */
  h: number;
}

/** The full normalized frame: the whole slide, `0;0;10000;10000`. */
export const RECT_FULL: Rect = { x: 0, y: 0, w: NORMALIZED_SPACE, h: NORMALIZED_SPACE };

/**
 * Default body frame, inferred from the vendor's printed
 * `Recta… 0;10000;9300` — full width, 93% tall, top-anchored so the 700-unit
 * footer strip stays below it.
 */
export const RECT_DEFAULT_BODY: Rect = { x: 0, y: 0, w: NORMALIZED_SPACE, h: 9_300 };

export interface StyleBody {
  font: string;
  size: number;
  color: string;
  align: Align;
  /**
   * Vertical placement of the lyric block in its frame. Mirrors Rust
   * `BodyStyle.valign`: absent (or `center`) means centered, and Rust skips
   * the key on write when it is center — so an omitted key here must be
   * read as `center`, never as unaligned.
   */
  valign?: VAlign;
  margins: { top: number; right: number; bottom: number; left: number };
  /**
   * Normalized frame for the body text. Absent means "not authored at this
   * level", in which case `bodyRect()` supplies [`RECT_DEFAULT_BODY`] and the
   * style explorer reports the geometry as inherited.
   *
   * `#[serde(default)]` on the Rust side, so a file without one loads.
   */
  rect?: Rect;
}

export interface StyleHeaderFooter {
  template: string;
  font: string;
  size: number;
  /** Normalized frame, absent = not authored at this level. */
  rect?: Rect;
}

export type BackgroundKind = "color" | "image" | "videoLoop" | "camera";

/**
 * How a background image fills the screen. Mirrors Rust `BgFit`
 * (`cover` = crop to fill, the vendor's only background mode; `contain` =
 * whole image with bars, the vendor's slideshow mode). Model-only until
 * per-background fit editing lands; declared so the literal table cannot
 * drift between ports.
 */
export type BgFit = "cover" | "contain";

export const BGFITS: readonly BgFit[] = ["cover", "contain"];

/** Parse a wire/prefs literal; unknown (or empty) means the default. Mirrors Rust `BgFit::from_literal`. */
export function bgFitFromLiteral(s: string): BgFit | undefined {
  return (BGFITS as readonly string[]).includes(s) ? (s as BgFit) : undefined;
}

export const BACKGROUND_KINDS: readonly BackgroundKind[] = [
  "color",
  "image",
  "videoLoop",
  "camera",
];

export interface Style {
  level: string;
  body: StyleBody;
  header?: StyleHeaderFooter;
  footer?: StyleHeaderFooter;
  background: {
    kind: BackgroundKind;
    color: string;
    /** Rust emits explicit `null` (no skip); accept it as well as absent. */
    image?: string | null;
    /** Rust emits explicit `null` (no skip); accept it as well as absent. */
    videoLoop?: string | null;
    /**
     * Normalized frame for the background, absent = not authored at this level.
     *
     * This is the one that makes "prepare 4:3, project 16:9" true: a background
     * authored to fill the slide is the whole square, and the projector's
     * aspect no longer decides whether it crops.
     */
    rect?: Rect;
  };
  /**
   * Slide transition for this style level: `none` | `fade` | `slide` | `zoom`.
   * v1 stores the selection and projects hard cuts; animated GL transitions
   * are a documented future step.
   *
   * Declared here because Rust has always had it. Without the declaration the
   * renderer had nowhere to put the selection and stuffed it into
   * `agenda.settings.transition`, which Rust's `AgendaSettings` does not model.
   */
  transition?: string;
}

/** The transition values the UI offers. Mirrors Rust `TRANSITIONS`. */
export const TRANSITIONS: readonly string[] = ["none", "fade", "slide", "zoom"];

export const AGENDA_VERSION = 1 as const;

/**
 * Every agenda item kind, in Rust's `ItemKind::ALL` order. `Bible`,
 * `PowerPointSlide` and `SongPowerPoint` were missing from this list, so
 * `isAgendaItemKind()` threw on agendas the GPUI port could happily write.
 */
export const AGENDA_KINDS: AgendaItemKind[] = [
  "Song",
  "BibleVerses",
  "Bible",
  "Image",
  "Video",
  "Audio",
  "Pdf",
  "PowerPoint",
  "PowerPointSlide",
  "SongPowerPoint",
  "Excel",
  "Word",
  "YouTube",
  "WebSite",
  "CCLI",
  "Alert",
  "Clock",
];

export function isAgendaItemKind(k: string): k is AgendaItemKind {
  return (AGENDA_KINDS as string[]).includes(k);
}