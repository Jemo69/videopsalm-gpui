/**
 * The cascading style model — **definition-of-done #6**.
 *
 * "Open the style explorer and read **which level** a font size came from."
 *
 * That is not expressible with the flat `Style` struct we had: one blob with a
 * single `level: string` on it, where every value looks equally "the" value and
 * nothing records where it was set. This module supplies the missing two pieces:
 *
 * 1. **Five chains from `Base`**, not one line. From
 *    `videopsalm-ux-research/assets/OBSERVATIONS.md` §8 and
 *    `hierarchical-styles-bg.png`:
 *
 *    ```text
 *    Base ── Songs      ─ Songbooks ─ Songbook ─ Song ─ SongVerse
 *         ├─ Bibles    ─ Bibles    ─ Bible    ─ Testament ─ Book ─ Chapter ─ Verse
 *         ├─ Images    ─ Images    ─ Image
 *         ├─ Videos    ─ Videos    ─ Video
 *         └─ PowerPoint ─ PowerPoints ─ PowerPoint ─ Slides
 *    ```
 *
 *    There is **no cross-kind inheritance**: a Bible verse's style does not see
 *    the Songbook, and an Image's style does not see the Bible. Our previous
 *    single line `Base → Songbook → Song → Verse` was wrong for Bible items,
 *    Images, Videos and PowerPoints.
 *
 * 2. **Per-attribute provenance.** Each leaf resolves independently, and the
 *    resolver reports the level each value came from. The original marks an
 *    inherited leaf with a distinct glyph; we need the data to drive it.
 *
 * This is the TypeScript mirror of `gpui-app/vp-core/src/cascade.rs` — rule 3
 * of AGENTS.md: one schema, two ports. The Rust side is canonical for enum
 * literals; these must agree exactly, and `tests/crossPort.test.ts` asserts it.
 *
 * Everything here is pure. Resolution takes the chain's stored values and
 * answers "what is the effective value, and which level set it".
 */

/** One rung of a cascade chain. */
export type Level =
  // Most general. Every chain starts here.
  | "base"
  // Song chain
  | "songbooks"
  | "songbook"
  | "song"
  /** Named `Strophe` in the original's editor — see OBSERVATIONS-lyrics-editor.md. */
  | "songVerse"
  // Bible chain
  | "bibles"
  | "bible"
  | "testament"
  | "book"
  | "chapter"
  | "verse"
  // Media chains
  | "images"
  | "image"
  | "videos"
  | "video"
  | "powerPoints"
  | "powerPoint"
  | "slides";

/** Every level, most general first. */
export const LEVELS: readonly Level[] = [
  "base",
  "songbooks",
  "songbook",
  "song",
  "songVerse",
  "bibles",
  "bible",
  "testament",
  "book",
  "chapter",
  "verse",
  "images",
  "image",
  "videos",
  "video",
  "powerPoints",
  "powerPoint",
  "slides",
];

/**
 * The display name the original's level selector shows, most specific first
 * (`style-level-selector.png`): `Verse, Song, Songbook, Songbooks, Base`.
 *
 * Note this list is **not** the full chain — it is the names an operator picks
 * for songs and Bibles. The media levels are reachable only through the
 * gallery, which is why the selector is short.
 */
const SELECTOR_CAPTION: Partial<Record<Level, string>> = {
  verse: "Verse",
  song: "Song",
  songbook: "Songbook",
  songbooks: "Songbooks",
  base: "Base",
};

/** The caption the level selector shows for `level`. */
export function selectorCaption(level: Level): string {
  return SELECTOR_CAPTION[level] ?? level;
}

/** Which chain a piece of content belongs to. Determines **which** levels apply. */
export type Chain = "songs" | "bibles" | "images" | "videos" | "powerPoint";

export const CHAINS: readonly Chain[] = ["songs", "bibles", "images", "videos", "powerPoint"];

/**
 * The levels in each chain, **most general first**.
 *
 * Order matters: resolution walks this backwards, so the last entry is the most
 * specific and wins.
 */
const CHAIN_LEVELS: Record<Chain, readonly Level[]> = {
  songs: ["base", "songbooks", "songbook", "song", "songVerse"],
  bibles: ["base", "bibles", "bible", "testament", "book", "chapter", "verse"],
  images: ["base", "images", "image"],
  videos: ["base", "videos", "video"],
  powerPoint: ["base", "powerPoints", "powerPoint", "slides"],
};

/** The levels in `chain`, most general first. */
export function chainLevels(chain: Chain): readonly Level[] {
  return CHAIN_LEVELS[chain];
}

/** Position of `level` within `chain`, most general = 0. */
export function chainDepth(chain: Chain, level: Level): number | undefined {
  return CHAIN_LEVELS[chain].indexOf(level);
}

/** `true` when `level` participates in `chain`. */
export function chainContains(chain: Chain, level: Level): boolean {
  return CHAIN_LEVELS[chain].includes(level);
}

/**
 * A resolved value **and the level that set it** — the provenance the style
 * explorer exists to show.
 *
 * `source` is `undefined` only when nothing in the chain defined the attribute,
 * in which case `value` is the built-in default.
 */
export interface Provenance<T> {
  value: T;
  source?: Level;
}

/**
 * `true` when the value was set at this exact level rather than inherited.
 */
export function isSetHere<T>(p: Provenance<T>, level: Level): boolean {
  return p.source === level;
}

/**
 * `true` when the value came from a **more general** level than `level`.
 *
 * This is what the original marks with a distinct glyph on inherited leaves —
 * "where did this actually come from?"
 */
export function isInheritedFrom<T>(p: Provenance<T>, level: Level): boolean {
  return p.source !== undefined && p.source !== level;
}

/**
 * One level's stored values, per attribute. `undefined` = "not set here".
 *
 * A map of optionals rather than a required-shape object because provenance is
 * **per attribute**: the original marks individual leaves as inherited, and a
 * fixed shape cannot say "the font came from Base but the size came from this
 * song".
 */
export interface LevelValues {
  /** font family */
  font?: string;
  /** text size in the normalized 0..10000 space the geometry uses */
  size?: number;
  /** colour, `#rrggbb` */
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  /** vertical alignment within its frame */
  vAlign?: string;
  /** background colour for the level's own surface */
  bgColor?: string;
  /** transition for this level: `none | fade | slide | zoom` */
  transition?: string;
  /** luminosity/brightness adjustment for an image or video assigned here */
  luminosity?: number;
}

/** Attribute names, in the order the original's explorer lists them. */
export const ATTRIBUTES = [
  "font",
  "size",
  "color",
  "bold",
  "italic",
  "underline",
  "strike",
  "vAlign",
  "bgColor",
  "transition",
  "luminosity",
] as const;

export type Attribute = (typeof ATTRIBUTES)[number];

/**
 * A normalized frame rectangle, in the **0..10000** space.
 *
 * The vendor's style explorer prints `Recta… 0;10000;9300`, so the space is
 * 0..10000 — our earlier §5.4 note of 0..1000 was wrong (see
 * `assets/OBSERVATIONS.md` §8).
 */
export interface FrameRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * The whole cascade: which chain each level belongs to, and what it sets.
 *
 * This is what replaces the old flat `Style` + single `level` string. It is a
 * **new** type rather than a change to `Style`, so existing files keep loading —
 * the ports convert a flat style into a one-level cascade on read.
 */
export interface Cascade {
  /**
   * Stored values per level. Keyed by level literal.
   *
   * **Optional** because a cascade read straight off disk — or from a file that
   * has no cascade at all — arrives as `{}` with no `values` key. Rust gets
   * `#[serde(default)]` for free; TypeScript does not, so every reader here
   * tolerates it. Make it required and `resolve({})` throws.
   */
  values?: Record<string, LevelValues>;
  /** Per-level normalized frame rects, keyed the same way. */
  rects?: Record<string, FrameRect>;
}

export function emptyCascade(): Cascade {
  return { values: {}, rects: {} };
}

/**
 * The values map, or an empty one — never `undefined`.
 *
 * Every internal read goes through this so a raw `JSON.parse` result is safe.
 */
function valuesOf(c: Cascade | undefined): Record<string, LevelValues> {
  return c?.values ?? {};
}

/** `true` when a level stores nothing — the common case. */
export function levelIsEmpty(v: LevelValues | undefined): boolean {
  if (!v) return true;
  return ATTRIBUTES.every((a) => v[a] === undefined);
}

/** Read a level's stored values. */
export function getLevel(c: Cascade, level: Level): LevelValues | undefined {
  return valuesOf(c)[level];
}

/**
 * The attributes `level` sets locally — everything else it inherits.
 *
 * This is the subset the explorer draws as *set* rather than *inherited*, and
 * `tests/crossPort.test.ts` pins it against the Rust side.
 */
export function attributeSet(c: Cascade, level: Level): Attribute[] {
  const v = valuesOf(c)[level];
  if (!v) return [];
  return ATTRIBUTES.filter((a) => v[a] !== undefined);
}

/**
 * Write a level's stored values, removing the entry when it is empty so a file
 * does not carry `"song": {}` noise.
 */
export function setLevel(c: Cascade, level: Level, v: LevelValues): Cascade {
  const values = { ...valuesOf(c) };
  if (levelIsEmpty(v)) {
    delete values[level];
  } else {
    values[level] = v;
  }
  return { ...c, values, rects: c.rects ?? {} };
}

/**
 * Clear one level's stored values — the "reset" action in the style explorer,
 * which removes the local override so the value re-inherits.
 */
export function resetLevel(c: Cascade, level: Level): Cascade {
  const values = { ...valuesOf(c) };
  delete values[level];
  const rects = { ...(c.rects ?? {}) };
  delete rects[level];
  return { ...c, values, rects };
}

export function getRect(c: Cascade, level: Level): FrameRect | undefined {
  return c.rects?.[level];
}

export function setRect(c: Cascade, level: Level, r: FrameRect): Cascade {
  return { ...c, rects: { ...(c.rects ?? {}), [level]: r } };
}

/**
 * The built-in defaults, used when nothing in the chain defines a value.
 *
 * Default text size is **70** — changelog 1.26: *"Default text size ('Base'
 * style) is now 70 (100 was too big)."*
 */
function defaults(): Required<Omit<LevelValues, never>> {
  return {
    font: "Candara",
    size: 70,
    color: "#ffffff",
    bold: false,
    italic: false,
    underline: false,
    strike: false,
    vAlign: "center",
    bgColor: "#000000",
    transition: "none",
    luminosity: 0,
  };
}

/**
 * Resolve one attribute across a chain, returning the value **and** the level
 * that set it.
 *
 * Walks most-specific → most-general and stops at the first level that defines
 * the attribute, which is exactly the inheritance rule: *"If a style attribute
 * of a verse is undefined, VideoPsalm will extract the value of this attribute
 * in the enclosing elements."*
 */
export function resolveAttr<T extends Attribute>(
  c: Cascade,
  chain: Chain,
  attr: T,
  fallback: NonNullable<LevelValues[T]>,
): Provenance<NonNullable<LevelValues[T]>> {
  const levels = CHAIN_LEVELS[chain];
  for (let i = levels.length - 1; i >= 0; i--) {
    const v = valuesOf(c)[levels[i]];
    const got = v?.[attr];
    if (got !== undefined) {
      return { value: got as NonNullable<LevelValues[T]>, source: levels[i] };
    }
  }
  return { value: fallback };
}

/** Every attribute resolved, each with its own provenance. */
export type Resolved = { [A in Attribute]: Provenance<NonNullable<LevelValues[A]>> };

/**
 * Resolve **every** attribute, so the explorer can show the tree with a
 * per-leaf source. This is the call that makes DoD #6 possible.
 */
export function resolve(c: Cascade, chain: Chain): Resolved {
  const d = defaults();
  const out = {} as Resolved;
  for (const attr of ATTRIBUTES) {
    (out as Record<string, unknown>)[attr] = resolveAttr(
      c,
      chain,
      attr,
      d[attr] as never
    );
  }
  return out;
}

/** One row of the explorer's tree: the label, the effective value, its source. */
export interface ExplorerRow {
  attr: Attribute;
  label: string;
  value: string;
  /** The level that set this value, or `undefined` for the built-in default. */
  source?: Level;
  /** `true` when `source` is more general than the level being inspected. */
  inherited: boolean;
}

/** Human labels, matching `Resolved.rows()` on the Rust side. */
const ATTR_LABEL: Record<Attribute, string> = {
  font: "Font",
  size: "Size",
  color: "Colour",
  bold: "Bold",
  italic: "Italic",
  underline: "Underline",
  strike: "Strike",
  vAlign: "Vertical align",
  bgColor: "Background",
  transition: "Transition",
  luminosity: "Luminosity",
};

function fmtNum(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(2);
}

function yesNo(v: boolean): string {
  return v ? "yes" : "no";
}

/** Format one attribute's resolved value the way the explorer shows it. */
export function formatValue(attr: Attribute, v: unknown): string {
  switch (attr) {
    case "size":
    case "luminosity":
      return fmtNum(v as number);
    case "bold":
    case "italic":
    case "underline":
    case "strike":
      return yesNo(v as boolean);
    default:
      return String(v);
  }
}

/**
 * The explorer's rows, in the original's order:
 * `Font, Size, Colour, Bold, Italic, Underline, Strike, Vertical align,
 *  Background, Transition, Luminosity`.
 *
 * `atLevel` is the level the operator is inspecting, and drives `inherited` —
 * the marker the original shows on leaves that came from further up.
 */
export function explorerRows(c: Cascade, chain: Chain, atLevel?: Level): ExplorerRow[] {
  const r = resolve(c, chain);
  return ATTRIBUTES.map((attr) => {
    const p = r[attr] as Provenance<unknown>;
    return {
      attr,
      label: ATTR_LABEL[attr],
      value: formatValue(attr, p.value),
      source: p.source,
      inherited: atLevel !== undefined && isInheritedFrom(p, atLevel),
    };
  });
}