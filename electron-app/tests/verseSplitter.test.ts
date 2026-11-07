import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { splitVerse, splitSong } from "../src/core/verseSplitter";

describe("verse-splitter", () => {
  it("packs lines within maxLines/maxChars", () => {
    const slides = splitVerse(["line one", "line two", "line three", "line four", "line five"], { maxLines: 2, maxChars: 18 });
    assert.ok(slides.length >= 2);
    assert.ok(slides.every((s) => s.length <= 2 && s.every((l) => l.length <= 18)));
    assert.equal(slides.flat().join(" "), "line one line two line three line four line five");
  });

  it("wraps long lines at word boundaries", () => {
    const slides = splitVerse(["Amazing grace how sweet the sound that saved"], { maxLines: 4, maxChars: 20 });
    assert.ok(slides.flat().every((l) => l.length <= 20));
    assert.equal(slides.flat().join(" "), "Amazing grace how sweet the sound that saved");
  });

  it("never leaves a single orphan word stranded", () => {
    const slides = splitVerse(["alpha beta gamma delta epsilon zeta"], { maxLines: 2, maxChars: 11 });
    const last = slides[slides.length - 1];
    const isLoneOrphan = last.length === 1 && last[0].split(/\s+/).length === 1;
    assert.equal(isLoneOrphan, false);
  });

  it("returns [] for empty input and throws on bad options", () => {
    assert.deepEqual(splitVerse([], { maxLines: 4, maxChars: 42 }), []);
    assert.throws(() => splitVerse(["x"], { maxLines: 0, maxChars: 42 }));
  });

  it("splits whole songs verse by verse", () => {
    const out = splitSong(
      [
        { id: "v1", lines: ["a b", "c d"] },
        { id: "v2", lines: ["e f"] },
      ],
      { maxLines: 4, maxChars: 42 }
    );
    assert.equal(out.length, 2);
    assert.equal(out[0].verseId, "v1");
    assert.equal(out[1].verseId, "v2");
  });
});
