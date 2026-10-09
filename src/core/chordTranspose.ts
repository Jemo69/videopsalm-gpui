// Chord transpose: A-B-C (latin) + Do-Re-Mi notations, ChordPro parse.
// Clean-room implementation of SPEC 2.2 chord behavior.

export type ChordNotation = "latin" | "doremi";

const LATIN_SHARP = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const LATIN_FLAT_MAP: Record<string, string> = {
  Db: "C#",
  Eb: "D#",
  Gb: "F#",
  Ab: "G#",
  Bb: "A#",
};
const DOREMI_SHARP = ["Do", "Do#", "Re", "Re#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si"];
const DOREMI_FLAT_MAP: Record<string, string> = {
  "Reb": "Do#",
  "Mib": "Re#",
  "Solb": "Fa#",
  "Lab": "Sol#",
  "Sib": "La#",
  "Dob": "Si",
  "Fab": "Mi",
};

const LATIN_ROOT_RE = /^([A-G])([#b]?)/;
const DOREMI_ROOT_RE = /^(Do|Re|Mi|Fa|Sol|La|Si)([#b]?)/;

function normalizeRoot(root: string, notation: ChordNotation): string {
  if (notation === "latin") {
    const m = root.match(LATIN_ROOT_RE);
    if (!m) throw new Error(`invalid latin chord root: ${root}`);
    const base = m[1] + m[2];
    return LATIN_FLAT_MAP[base] ?? base;
  }
  const m = root.match(DOREMI_ROOT_RE);
  if (!m) throw new Error(`invalid doremi chord root: ${root}`);
  const base = m[1] + m[2];
  return DOREMI_FLAT_MAP[base] ?? base;
}

function indexOfRoot(norm: string, notation: ChordNotation): number {
  const table = notation === "latin" ? LATIN_SHARP : DOREMI_SHARP;
  const i = table.indexOf(norm);
  if (i < 0) throw new Error(`unknown chord root: ${norm}`);
  return i;
}

function splitRoot(chord: string, notation: ChordNotation): { root: string; suffix: string } {
  const re = notation === "latin" ? LATIN_ROOT_RE : DOREMI_ROOT_RE;
  const m = chord.match(re);
  if (!m) throw new Error(`invalid chord: ${chord}`);
  return { root: m[0], suffix: chord.slice(m[0].length) };
}

/** Transpose a single chord symbol by semitones (supports slash bass). */
export function transposeChord(chord: string, semitones: number, notation: ChordNotation = "latin"): string {
  const parts = chord.split("/");
  const transposeOne = (c: string): string => {
    const { root, suffix } = splitRoot(c.trim(), notation);
    const norm = normalizeRoot(root, notation);
    const table = notation === "latin" ? LATIN_SHARP : DOREMI_SHARP;
    const idx = (indexOfRoot(norm, notation) + ((semitones % 12) + 12)) % 12;
    return table[idx] + suffix;
  };
  return parts.map(transposeOne).join("/");
}

/** Transpose every ChordPro bracket [Am] in a line of text. */
export function transposeChordProLine(line: string, semitones: number, notation: ChordNotation = "latin"): string {
  return line.replace(/\[([^\]]+)\]/g, (_m, chord: string) => {
    try {
      return "[" + transposeChord(chord.trim(), semitones, notation) + "]";
    } catch {
      return "[" + chord + "]"; // leave unparseable tokens untouched
    }
  });
}

/** Parse ChordPro text into lyric lines + chord overlay positions. */
export function parseChordPro(text: string): { lines: string[]; overlays: { line: number; chord: string; pos: number }[] } {
  const lines: string[] = [];
  const overlays: { line: number; chord: string; pos: number }[] = [];
  for (const raw of text.split("\n")) {
    let plain = "";
    let i = 0;
    while (i < raw.length) {
      if (raw[i] === "[") {
        const end = raw.indexOf("]", i);
        if (end < 0) {
          plain += raw.slice(i);
          break;
        }
        const chord = raw.slice(i + 1, end);
        overlays.push({ line: lines.length, chord, pos: plain.length });
        i = end + 1;
      } else {
        plain += raw[i];
        i++;
      }
    }
    lines.push(plain);
  }
  return { lines, overlays };
}
