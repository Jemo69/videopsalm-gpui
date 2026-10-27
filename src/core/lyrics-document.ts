// The lyrics editor document, parsed the way VideoPsalm stores songs.
//
// VideoPsalm has two independent breaks:
//   * a verse boundary is a `Tag`/`ID` pair (`V1`, `C`, `B`, …), which is
//     metadata, never lyric text;
//   * a slide boundary inside one verse is a truly empty line (the native
//     writer normalises it to a line containing only U+00A0).
//
// The old renderer used blank lines for both, so editing a multi-slide verse
// split it into extra native verses and the `V1`/`C` headings leaked into the
// slides. This module is the single parser used by the editor, preview and
// slide panel so all three agree.

import type { SongVerse } from "./schemas";

/** Recognised native invariant tags, in SongVerseTag enum order. */
export const VERSE_TAGS = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"] as const;

const TAG_LINE = /^([VCPBTEOSIR])\s*(\d*)$/i;
const NBSP = "\u00a0";

export interface LyricsBlock {
  tag?: string;
  tagId?: number;
  /** Lines as authored. A blank/NBSP-only line is an in-verse slide break. */
  lines: string[];
}

export interface LyricsSlide {
  tag?: string;
  tagId?: number;
  /** Display label (`V1`, `C`, …), or "" when the source has no tags. */
  label: string;
  /** Lines shown on this slide, without any tag heading. */
  lines: string[];
  body: string;
}

/** `V1` / `C` / `B` from the native tag pair. ID 0 is intentionally bare. */
export function verseLabel(tag?: string, tagId?: number): string {
  if (!tag) return "";
  return `${tag}${tagId !== undefined && tagId > 0 ? tagId : ""}`;
}

/** True for a heading line such as `V1`, `C`, or `B2` (not lyric text). */
export function isVerseTagLine(line: string): boolean {
  return TAG_LINE.test(line.trim());
}

/** True when the document contains at least one native heading line. */
export function hasVerseTagLine(text: string): boolean {
  return text.split(/\r?\n/).some(isVerseTagLine);
}

export interface VerseTagInsertion {
  text: string;
  cursor: number;
  changed: boolean;
}

/**
 * Insert `V1`/`C`/`B` as a standalone heading for the block containing the
 * caret. In a tagged/native document the block is already delimited by the
 * previous heading, so an existing label is left alone (no duplicate `C2`);
 * in an untagged local song the heading starts the current blank-line stanza.
 * The heading must be on its own line, never inline with lyric text.
 */
export function insertVerseTag(
  text: string,
  cursor: number,
  tag: string,
  tagged: boolean
): VerseTagInsertion {
  const pos = Math.max(0, Math.min(cursor, text.length));
  const before = text.slice(0, pos);
  let start: number;
  if (tagged) {
    // A blank line is a slide break inside a native verse. The verse starts
    // after its own heading line, not after the blank line.
    start = 0;
    let offset = 0;
    for (const line of before.split("\n")) {
      if (isVerseTagLine(line)) start = offset + line.length + 1;
      offset += line.length + 1;
    }
    if (start > 0 || isVerseTagLine((before.split("\n")[0] ?? "").trim())) {
      return { text, cursor: pos, changed: false };
    }
  } else {
    const boundary = before.lastIndexOf("\n\n");
    start = boundary < 0 ? 0 : boundary + 2;
  }
  const insert = `${tag}\n`;
  return {
    text: text.slice(0, start) + insert + text.slice(start),
    cursor: start + insert.length,
    changed: true,
  };
}

/**
 * Split a lyrics document into native verse blocks.
 *
 * `tagged` mode is for native VideoPsalm songs: headings delimit verses and a
 * blank line stays inside the verse as a slide break. Untagged mode preserves
 * the legacy local-song rule where a blank line starts the next stanza.
 */
export function parseLyricsDocument(text: string, tagged: boolean): LyricsBlock[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  if (!tagged) {
    const blocks: LyricsBlock[] = [];
    let current: string[] = [];
    const flush = (): void => {
      const trimmed = trimBlankEdges(current);
      if (trimmed.length) blocks.push({ lines: trimmed });
      current = [];
    };
    for (const line of lines) {
      // Only a truly empty line is a boundary. A line containing one space is
      // VideoPsalm's documented "empty slide" content.
      if (line.length === 0) flush();
      else current.push(line);
    }
    flush();
    return blocks;
  }

  const blocks: LyricsBlock[] = [];
  let current: LyricsBlock | null = null;
  const flush = (): void => {
    if (!current) return;
    const trimmed = trimBlankEdges(current.lines);
    blocks.push({ ...current, lines: trimmed });
    current = null;
  };
  for (const line of lines) {
    const match = TAG_LINE.exec(line.trim());
    if (match) {
      flush();
      current = {
        tag: match[1].toUpperCase(),
        tagId: match[2] ? Number(match[2]) : 0,
        lines: [],
      };
      continue;
    }
    if (!current) current = { lines: [] };
    current.lines.push(line);
  }
  flush();
  return blocks;
}

/** Remove leading/trailing slide breaks from one block. */
function trimBlankEdges(lines: string[]): string[] {
  let start = 0;
  let end = lines.length;
  while (start < end && isSlideBreak(lines[start])) start++;
  while (end > start && isSlideBreak(lines[end - 1])) end--;
  return lines.slice(start, end);
}

/** A truly empty line or VideoPsalm's U+00A0-only slide separator. */
export function isSlideBreak(line: string): boolean {
  return line.length === 0 || /^[\u00a0\u2007\u202f]+$/.test(line);
}

/**
 * Split one verse into the slides VideoPsalm will project. A verse can span
 * several slides; every slide keeps the same `V1`/`C` label.
 */
export function slidesFromBlock(block: LyricsBlock, fallbackIndex = 0): LyricsSlide[] {
  const chunks: string[][] = [];
  let current: string[] = [];
  for (const line of block.lines) {
    if (isSlideBreak(line)) {
      chunks.push(current);
      current = [];
    } else {
      current.push(line);
    }
  }
  chunks.push(current);
  const nonEmpty = chunks.filter((chunk) => chunk.some((line) => line.length > 0));
  if (!nonEmpty.length) nonEmpty.push([]);
  return nonEmpty.map((lines) => ({
    ...(block.tag ? { tag: block.tag } : {}),
    ...(block.tagId !== undefined ? { tagId: block.tagId } : {}),
    label: verseLabel(block.tag, block.tagId) || `V${fallbackIndex + 1}`,
    lines,
    body: lines.join("\n"),
  }));
}

/**
 * Pick one native `Sequence` order. VideoPsalm stores alternatives separated
 * by commas and each order as space-separated invariant labels (`V1 C V2 C`).
 * A missing/negative `VerseOrderIndex` means "document order" (the native
 * default is -1).
 */
export function selectVerseOrder(
  sequence: string | undefined,
  verseOrderIndex: number | undefined
): string[] | null {
  if (!sequence || verseOrderIndex === undefined || verseOrderIndex < 0) return null;
  const orders = sequence.split(",").map((order) => order.trim()).filter(Boolean);
  if (verseOrderIndex >= orders.length) return null;
  const tokens = orders[verseOrderIndex].split(/\s+/).filter(Boolean);
  return tokens.length ? tokens : null;
}

/**
 * Reorder verse blocks the way `Song.SetSlidesCore`/`GetTokenItem` does:
 * search forward from the previous match, wrap to the beginning, and keep a
 * repeated token (so a chorus can appear between every verse). Unknown tokens
 * are skipped. If nothing matches, fall back to document order rather than
 * blanking the projector on a malformed native sequence.
 */
export function orderBlocksBySequence(
  blocks: LyricsBlock[],
  sequence: string | undefined,
  verseOrderIndex: number | undefined
): LyricsBlock[] {
  const tokens = selectVerseOrder(sequence, verseOrderIndex);
  if (!tokens) return blocks;
  const labels = blocks.map((block) => verseLabel(block.tag, block.tagId));
  const ordered: LyricsBlock[] = [];
  let previous = -1;
  for (const token of tokens) {
    let found = -1;
    for (let i = previous + 1; i < blocks.length; i++) {
      if (labels[i] === token) { found = i; break; }
    }
    if (found < 0) {
      for (let i = 0; i <= previous && i < blocks.length; i++) {
        if (labels[i] === token) { found = i; break; }
      }
    }
    if (found >= 0) {
      ordered.push(blocks[found]);
      previous = found;
    }
  }
  return ordered.length ? ordered : blocks;
}

/** Parse editor text straight into slides (heading lines removed). */
export function slidesFromLyricsDocument(
  text: string,
  tagged: boolean,
  sequence?: string,
  verseOrderIndex?: number
): LyricsSlide[] {
  const blocks = orderBlocksBySequence(parseLyricsDocument(text, tagged), sequence, verseOrderIndex);
  return blocks.flatMap((block, i) => slidesFromBlock(block, i));
}

/** Build the editor document from the shared/native verse model. */
export function documentFromVerses(verses: SongVerse[], tagged?: boolean): string {
  const useTags = tagged ?? verses.some((v) => v.tag);
  return verses
    .map((v) => {
      const label = useTags ? verseLabel(v.tag, v.tagId) : "";
      return [label, ...v.lines].filter((line, i) => i > 0 || line !== "").join("\n");
    })
    .join("\n\n");
}

/** Preview/projection text with all native heading lines removed. */
export function renderLyricsDocument(text: string, tagged: boolean): string {
  return parseLyricsDocument(text, tagged)
    .map((block) => block.lines.join("\n"))
    .join("\n\n");
}

/** Preview/projection text straight from the song model. */
export function renderVerses(verses: SongVerse[]): string {
  return verses.map((v) => v.lines.join("\n")).join("\n\n");
}

/** Convert a verse into slides without going through the editor text. */
export function slidesFromVerses(
  verses: SongVerse[],
  sequence?: string,
  verseOrderIndex?: number
): LyricsSlide[] {
  const blocks: LyricsBlock[] = verses.map((v) => ({ tag: v.tag, tagId: v.tagId, lines: v.lines }));
  return orderBlocksBySequence(blocks, sequence, verseOrderIndex)
    .flatMap((block, i) => slidesFromBlock(block, i));
}

/**
 * Apply an edited document back to the song model. Headings are consumed as
 * tags; a blank line inside a tagged verse becomes U+00A0 so the native file
 * keeps the slide break instead of inventing a new verse.
 */
export function versesFromLyricsDocument(
  text: string,
  existing: SongVerse[],
  tagged: boolean
): SongVerse[] {
  return parseLyricsDocument(text, tagged).map((block, i) => {
    const previous = existing[i];
    const tag = block.tag ?? previous?.tag ?? (tagged ? "V" : undefined);
    const tagId = block.tag !== undefined
      ? block.tagId
      : previous?.tagId ?? (tagged ? 1 : undefined);
    const lines = block.lines.map((line) => (isSlideBreak(line) ? NBSP : line));
    return {
      id: previous?.id ?? String(i + 1),
      lines,
      order: i + 1,
      ...(tag ? { tag } : {}),
      ...(tagId !== undefined ? { tagId } : {}),
    };
  });
}
