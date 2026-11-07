// Verse splitter: packs lyric/verse lines into slides.
// Rules (SPEC 2.2): max lines per slide, max chars per line, no orphan words
// (a trailing slide must not contain a single lone word pulled from context).

export interface SplitOptions {
  maxLines: number;
  maxChars: number;
}

export const DEFAULT_SPLIT_OPTIONS: SplitOptions = { maxLines: 4, maxChars: 42 };

function wrapWords(words: string[], maxChars: number): string[] {
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    if (w.length > maxChars) {
      // hard-break overlong word
      if (cur) {
        lines.push(cur);
        cur = "";
      }
      for (let i = 0; i < w.length; i += maxChars) lines.push(w.slice(i, i + maxChars));
      continue;
    }
    const cand = cur ? cur + " " + w : w;
    if (cand.length <= maxChars) {
      cur = cand;
    } else {
      lines.push(cur);
      cur = w;
    }
  }
  if (cur) lines.push(cur);
  return lines;
}

/** Split a verse's lines into slides (arrays of lines). */
export function splitVerse(verseLines: string[], opts: SplitOptions = DEFAULT_SPLIT_OPTIONS): string[][] {
  const { maxLines, maxChars } = opts;
  if (maxLines < 1 || maxChars < 1) throw new Error("maxLines and maxChars must be >= 1");
  const words = verseLines
    .join(" ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return [];
  const lines = wrapWords(words, maxChars);
  const slides: string[][] = [];
  for (let i = 0; i < lines.length; i += maxLines) slides.push(lines.slice(i, i + maxLines));

  // No-orphan fix: if the last slide is a single line holding a single word
  // and a previous slide exists with room to spare, move one word back.
  const last = slides[slides.length - 1];
  if (slides.length > 1 && last.length === 1 && last[0].split(/\s+/).length === 1) {
    const prev = slides[slides.length - 2];
    if (prev.length > 1) {
      const prevWords = prev.join(" ").split(/\s+/);
      const orphan = prevWords.pop()!;
      slides[slides.length - 2] = wrapWords(prevWords, maxChars).slice(0, maxLines);
      // re-wrap tail: orphan + last word
      const tail = wrapWords([orphan, ...last[0].split(/\s+/)], maxChars);
      slides[slides.length - 1] = tail;
    } else {
      // merge tiny tail into previous single-line slide if it fits
      const merged = wrapWords((prev[0] + " " + last[0]).split(/\s+/), maxChars);
      if (merged.length <= maxLines) {
        slides.splice(slides.length - 2, 2, merged);
      }
    }
  }
  return slides;
}

/** Split a whole song (list of verses) into an ordered slide list. */
export function splitSong(
  verses: { id: string; lines: string[] }[],
  opts: SplitOptions = DEFAULT_SPLIT_OPTIONS
): { verseId: string; slide: number; lines: string[] }[] {
  const out: { verseId: string; slide: number; lines: string[] }[] = [];
  for (const v of verses) {
    const slides = splitVerse(v.lines, opts);
    slides.forEach((lines, i) => out.push({ verseId: v.id, slide: i, lines }));
  }
  return out;
}
