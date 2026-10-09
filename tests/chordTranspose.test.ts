import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { transposeChord, transposeChordProLine, parseChordPro } from "../src/core/chordTranspose";

describe("chord-transpose", () => {
  it("transposes latin chords up", () => {
    assert.equal(transposeChord("C", 2), "D");
    assert.equal(transposeChord("Am", 2), "Bm");
    assert.equal(transposeChord("G/B", 2), "A/C#");
  });

  it("transposes down and wraps around", () => {
    assert.equal(transposeChord("C", -1), "B");
    assert.equal(transposeChord("B", 1), "C");
  });

  it("normalizes flats to sharps", () => {
    assert.equal(transposeChord("Bb", 0), "A#");
    assert.equal(transposeChord("Eb", 2), "F");
  });

  it("keeps suffixes intact", () => {
    assert.equal(transposeChord("F#m7", 1), "Gm7");
    assert.equal(transposeChord("Csus4", 0), "Csus4");
  });

  it("transposes Do-Re-Mi notation", () => {
    assert.equal(transposeChord("Do", 2, "doremi"), "Re");
    assert.equal(transposeChord("Si", 1, "doremi"), "Do");
    assert.equal(transposeChord("Sib", 0, "doremi"), "La#");
  });

  it("transposes ChordPro lines, leaving bad tokens alone", () => {
    assert.equal(transposeChordProLine("[G]Amazing [C]grace", 2), "[A]Amazing [D]grace");
    assert.equal(transposeChordProLine("[N.C.] drum break", 2), "[N.C.] drum break");
  });

  it("parses ChordPro into lines + overlays", () => {
    const { lines, overlays } = parseChordPro("[G]Hello [C]world\nSecond [D]line");
    assert.deepEqual(lines, ["Hello world", "Second line"]);
    assert.equal(overlays.length, 3);
    assert.deepEqual(overlays[0], { line: 0, chord: "G", pos: 0 });
  });
});
