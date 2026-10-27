// Cross-port schema fixture (AGENTS.md rule 3: "one schema, two ports").
//
// `tests/fixtures/cross-port-schema.json` holds one document per shared
// shape, containing **every** enum literal and **every** optional field. The
// same file is parsed by `gpui-app/vp-core/src/schema.rs`'s `#[cfg(test)]`
// block, so any literal added to one port and not the other fails a test here
// or there rather than a user's agenda.
//
// This file asserts the TypeScript half: that `schemas.ts` declares exactly
// the canonical literal tables, that every fixture document parses into the
// declared interfaces with nothing dropped, and that the legacy spellings are
// still accepted and normalise to the canonical ones.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";

import {
  AGENDA_KINDS,
  ALIGNS,
  BACKGROUND_KINDS,
  CHORD_NOTATION_ALIASES,
  CHORD_NOTATIONS,
  FLOW_TYPE_ALIASES,
  FLOW_TYPES,
  NORMALIZED_SPACE,
  RECT_DEFAULT_BODY,
  RECT_FULL,
  TEXT_SIZE_SPACE,
  TRANSITIONS,
  VERSIFICATIONS,
  canonicalChordNotation,
  canonicalFlowType,
  isAgendaItemKind,
  isChordNotation,
  isFlowType,
  type Agenda,
  type AgendaItem,
  type BibleRef,
  type Song,
  type Style,
} from "../src/core/schemas";
import {
  ASPECT_RATIOS,
  aspectViewport,
  resolveRect,
  resolveRectPercent,
  textSize,
  viewport,
  type FitMode,
} from "../src/core/geometry";
import { parseAgenda, serializeAgenda } from "../src/core/agendaSerde";

interface Fixture {
  comment: string;
  enums: {
    chordNotations: string[];
    flowTypes: string[];
    itemKinds: string[];
    versifications: string[];
    aligns: string[];
    backgroundKinds: string[];
    transitions: string[];
  };
  songs: Song[];
  bibleRefs: BibleRef[];
  styles: Style[];
  agenda: Agenda;
  /** B-06: the normalized space and everything resolved inside it. */
  geometry: {
    normalizedSpace: number;
    textSizeSpace: number;
    rectFull: { x: number; y: number; w: number; h: number };
    rectDefaultBody: { x: number; y: number; w: number; h: number };
    rectDefaultHeader: { x: number; y: number; w: number; h: number };
    rectDefaultFooter: { x: number; y: number; w: number; h: number };
    aspectRatios: {
      label: string;
      num: number;
      den: number;
      viewport1920: { width: number; height: number };
    }[];
    resolveBody9300Fill: {
      viewport: { width: number; height: number };
      mode: FitMode;
      percent: { x: number; y: number; w: number; h: number };
      pixels: { x: number; y: number; w: number; h: number };
    }[];
    resolveBody9300Letterbox4x3: {
      percent: { x: number; y: number; w: number; h: number };
      pixels: { x: number; y: number; w: number; h: number };
    };
    textSize70At1080p: number;
    textSize100At1080p: number;
  };
  legacyAliases: {
    chordNotations: Record<string, string>;
    flowTypes: Record<string, string>;
    song: Song;
    agenda: Agenda;
  };
}

const FIXTURE_PATH = path.join(__dirname, "..", "..", "tests", "fixtures", "cross-port-schema.json");
const FIXTURE_TEXT = fs.readFileSync(FIXTURE_PATH, "utf8");
const fixture: Fixture = JSON.parse(FIXTURE_TEXT);

describe("cross-port schema fixture", () => {
  it("parses as JSON and names the invariant it protects", () => {
    assert.match(fixture.comment, /rule 3/);
  });

  describe("canonical literal tables match schemas.ts exactly", () => {
    it("chord notations", () => {
      assert.deepEqual([...CHORD_NOTATIONS], fixture.enums.chordNotations);
    });
    it("flow types", () => {
      assert.deepEqual([...FLOW_TYPES], fixture.enums.flowTypes);
    });
    it("item kinds", () => {
      assert.deepEqual([...AGENDA_KINDS], fixture.enums.itemKinds);
    });
    it("versifications", () => {
      assert.deepEqual([...VERSIFICATIONS], fixture.enums.versifications);
    });
    it("aligns", () => {
      assert.deepEqual([...ALIGNS], fixture.enums.aligns);
    });
    it("background kinds", () => {
      assert.deepEqual([...BACKGROUND_KINDS], fixture.enums.backgroundKinds);
    });
    it("transitions", () => {
      assert.deepEqual([...TRANSITIONS], fixture.enums.transitions);
    });
  });

  describe("songs", () => {
    it("carries chordpro, updatedAt and the searchable metadata", () => {
      assert.equal(fixture.songs.length, 2);
      const [abc, doremi] = fixture.songs;

      // chordpro: declared here, absent from schema.rs until now.
      assert.equal(abc.chords?.chordpro, "[G]How precious did that [C]grace appear");
      assert.equal(doremi.chords?.chordpro, undefined);

      // Both canonical notations appear.
      assert.equal(abc.chords?.notation, "ABC");
      assert.equal(doremi.chords?.notation, "DoReMi");
      assert.ok(isChordNotation(abc.chords!.notation));

      // updatedAt: emitted by Rust, previously undeclared here.
      assert.equal(abc.updatedAt, "2026-01-02T03:04:05Z");
      assert.equal(doremi.updatedAt, "2026-10-06");

      assert.equal(abc.copyright, "Public domain");
      assert.deepEqual(abc.themes, ["grace", "salvation"]);
      assert.deepEqual(abc.references, ["Isaiah 53:5"]);
      assert.equal(abc.memo1, "slow");
      assert.equal(abc.memo2, "pickup on line 3");
      assert.equal(abc.songId, "00000001");

      // Native VideoPsalm additions: verse identity, selected Sequence, and
      // the source locator used for write-through editing.
      assert.equal(abc.verses[0].tag, "V");
      assert.equal(abc.verses[0].tagId, 1);
      assert.equal(abc.sequence, "V1 C V2 C");
      assert.equal(abc.verseOrderIndex, 0);
      assert.equal(abc.source?.format, "videopsalm");
      assert.equal(abc.source?.songbookGuid, "book-guid");
      assert.equal(abc.source?.guid, "song-guid");
    });

    it("survives a JSON round-trip with every field intact", () => {
      for (const song of fixture.songs) {
        assert.deepEqual(JSON.parse(JSON.stringify(song)), song);
      }
    });
  });

  describe("bible refs", () => {
    it("uses the singular canonical book key and covers all three versifications", () => {
      assert.equal(fixture.bibleRefs.length, 3);
      assert.deepEqual(
        fixture.bibleRefs.map((r) => r.versification),
        fixture.enums.versifications
      );
      assert.equal(fixture.bibleRefs[0].book, "John");
      assert.equal(fixture.bibleRefs[1].book, "Psalm");
      assert.deepEqual(fixture.bibleRefs[0].verses, [16, 17, 18]);
    });
  });

  describe("styles", () => {
    it("declares transition on every style and covers every align / background kind", () => {
      assert.equal(fixture.styles.length, 4);
      for (const style of fixture.styles) {
        assert.equal(typeof style.transition, "string", `${style.level} needs transition`);
        assert.ok(
          (TRANSITIONS as readonly string[]).includes(style.transition!),
          `${style.level} has unknown transition ${style.transition}`
        );
      }
      const aligns = fixture.styles.map((s) => s.body.align);
      const kinds = fixture.styles.map((s) => s.background.kind);
      for (const a of fixture.enums.aligns) assert.ok(aligns.includes(a as never), `missing align ${a}`);
      for (const k of fixture.enums.backgroundKinds) assert.ok(kinds.includes(k as never), `missing kind ${k}`);
    });

    it("the videoLoop key is spelled the Rust way", () => {
      const loop = fixture.styles.find((s) => s.background.kind === "videoLoop");
      assert.equal(loop?.background.videoLoop, "assets/media/loop.mp4");
    });

    it("carries a rect on every frame, and the constants match geometry.ts", () => {
      for (const style of fixture.styles) {
        assert.ok(style.body.rect, `${style.level} needs a body rect`);
        assert.ok(style.background.rect, `${style.level} needs a background rect`);
      }
      // The first fixture style's rects are the worked examples the geometry
      // tests resolve; pin the linkage, not just the presence.
      assert.deepEqual(fixture.styles[0].body.rect, { x: 0, y: 1000, w: 9000, h: 8000 });
      assert.deepEqual(fixture.styles[0].background.rect, RECT_FULL);
      assert.deepEqual(RECT_DEFAULT_BODY, fixture.geometry.rectDefaultBody);
    });
  });

  describe("geometry", () => {
    const g = fixture.geometry;

    it("the space is 10000 and text size has its own 1000 space", () => {
      assert.equal(g.normalizedSpace, NORMALIZED_SPACE);
      assert.equal(g.normalizedSpace, 10_000);
      assert.equal(g.textSizeSpace, TEXT_SIZE_SPACE);
      assert.equal(g.textSizeSpace, 1_000);
    });

    it("the constant rects match geometry.ts", () => {
      assert.deepEqual(g.rectFull, RECT_FULL);
      assert.deepEqual(g.rectDefaultBody, RECT_DEFAULT_BODY);
    });

    it("the five vendor presets match ASPECT_RATIOS, in ribbon order", () => {
      assert.deepEqual(
        g.aspectRatios.map((a) => a.label),
        ASPECT_RATIOS.map((a) => a.label)
      );
      for (let i = 0; i < g.aspectRatios.length; i++) {
        const want = g.aspectRatios[i];
        const got = ASPECT_RATIOS[i];
        assert.equal(got.num, want.num, want.label);
        assert.equal(got.den, want.den, want.label);
        const vp = aspectViewport(got, 1920);
        assert.deepEqual({ width: vp.width, height: vp.height }, want.viewport1920, want.label);
      }
    });

    it("a rect authored once resolves to the same fraction on 4:3 and 16:9", () => {
      const body = { x: 0, y: 0, w: 10_000, h: 9_300 };
      for (const c of g.resolveBody9300Fill) {
        const vp = viewport(c.viewport.width, c.viewport.height);
        const p = resolveRectPercent(body, vp, c.mode);
        // Float arithmetic, so compare within a hair — the Rust half uses the
        // same 1e-6. `deepEqual` would fail on 93.0000000001 vs 93.
        for (const k of ["x", "y", "w", "h"] as const) {
          assert.ok(Math.abs(p[k] - c.percent[k]) < 1e-6, `${k}: ${p[k]} vs ${c.percent[k]}`);
        }
        assert.deepEqual(resolveRect(body, vp, c.mode), c.pixels);
      }
    });

    it("letterboxed at 4:3 the square hangs 12.5% in on each side", () => {
      const body = { x: 0, y: 0, w: 10_000, h: 9_300 };
      const vp = viewport(1024, 768);
      const p = resolveRectPercent(body, vp, "letterbox");
      for (const k of ["x", "y", "w", "h"] as const) {
        const want = g.resolveBody9300Letterbox4x3.percent[k];
        assert.ok(Math.abs(p[k] - want) < 1e-6, `${k}: ${p[k]} vs ${want}`);
      }
      assert.deepEqual(resolveRect(body, vp, "letterbox"), g.resolveBody9300Letterbox4x3.pixels);
    });

    it("70 units are 75.6 px at 1080p, and 100 is 'too big' at 108", () => {
      const hd = viewport(1920, 1080);
      assert.ok(Math.abs(textSize(70, hd, "letterbox") - g.textSize70At1080p) < 0.01);
      assert.ok(Math.abs(textSize(100, hd, "letterbox") - g.textSize100At1080p) < 0.01);
    });
  });

  describe("agenda", () => {
    const agenda = fixture.agenda;

    it("parses through agendaSerde without losing anything", () => {
      const parsed = parseAgenda(JSON.stringify(agenda));
      assert.equal(parsed.version, 1);
      assert.equal(parsed.items.length, fixture.agenda.items.length);
      // `parseAgenda` maps an absent/`null` optional field to `undefined`,
      // which `JSON.stringify` drops, so compare the serialised forms of both
      // sides rather than the in-memory objects.
      assert.deepEqual(JSON.parse(serializeAgenda(parsed)), JSON.parse(JSON.stringify(parsed)));
    });

    it("covers every item kind, in Rust's order", () => {
      const kinds = agenda.items.map((i) => i.kind);
      assert.deepEqual(kinds, fixture.enums.itemKinds as AgendaItem["kind"][]);
      for (const k of kinds) assert.ok(isAgendaItemKind(k), `unknown kind ${k}`);
    });

    it("covers every flow type", () => {
      const flows = agenda.items.map((i) => i.flowType);
      for (const f of fixture.enums.flowTypes) {
        assert.ok(flows.includes(f as never), `missing flowType ${f}`);
      }
    });

    it("exercises every ref variant", () => {
      const has = (k: string) => agenda.items.find((i) => i.kind === k)!.ref ?? {};
      assert.ok(has("Song").songId);
      assert.ok(has("BibleVerses").bibleRef);
      assert.ok(has("Bible").bibleRef);
      assert.ok(has("Image").file);
      assert.ok(has("YouTube").url);
      assert.ok(has("WebSite").url);
      assert.ok(has("Alert").text);
      assert.ok(has("Clock").text);
      assert.ok(has("CCLI").text);
    });

    it("carries overrides and the settings style", () => {
      const song = agenda.items.find((i) => i.kind === "Song")!;
      assert.deepEqual(song.overrides?.["verseOrder"], ["V2", "V1"]);
      assert.equal(song.overrides?.["pptxImport"], true);

      const ppt = agenda.items.find((i) => i.kind === "PowerPoint")!;
      assert.equal(ppt.overrides?.["startSlide"], 1);
      assert.equal(ppt.overrides?.["endSlide"], 12);

      assert.equal((agenda.settings as Record<string, unknown>)["bilingualSuppressEmpty"], true);
      assert.equal((agenda.settings as Record<string, unknown>)["wholeReferenceHeader"], true);
      assert.equal((agenda.settings as Record<string, unknown>)["lang"], "en");
    });

    it("pairs autoAdvanceMs with the timed flow types", () => {
      for (const item of agenda.items) {
        if (item.flowType === "auto" || item.flowType === "AutoAdvance") {
          assert.equal(item.autoAdvanceMs, 30000, `${item.id} is timed but has no timer`);
        }
      }
    });
  });

  describe("legacy literal aliases", () => {
    it("every chord-notation alias is accepted and normalises canonically", () => {
      for (const [legacy, canonical] of Object.entries(fixture.legacyAliases.chordNotations)) {
        assert.ok(isChordNotation(legacy), `${legacy} must still parse`);
        assert.equal(canonicalChordNotation(legacy as never), canonical);
        assert.ok(CHORD_NOTATIONS.includes(canonical as never));
      }
    });

    it("every accepted literal in an alias table is declared in schemas.ts", () => {
      // Guards against an alias documented in a comment but absent from the
      // union: anything the fixture exercises must be in the accepted set.
      const accepted = [
        ...CHORD_NOTATIONS,
        ...Object.keys(CHORD_NOTATION_ALIASES),
      ];
      for (const lit of Object.keys(fixture.legacyAliases.chordNotations)) {
        assert.ok(accepted.includes(lit as never), `${lit} missing from accepted set`);
      }
      const acceptedFlows = [...FLOW_TYPES, ...Object.keys(FLOW_TYPE_ALIASES)];
      for (const lit of Object.keys(fixture.legacyAliases.flowTypes)) {
        assert.ok(acceptedFlows.includes(lit as never), `${lit} missing from accepted set`);
      }
    });

    it("every flow-type alias is accepted and normalises canonically", () => {
      for (const [legacy, canonical] of Object.entries(fixture.legacyAliases.flowTypes)) {
        assert.ok(isFlowType(legacy), `${legacy} must still parse`);
        assert.equal(canonicalFlowType(legacy as never), canonical);
        assert.ok(FLOW_TYPES.includes(canonical as never));
      }
    });

    it("the alias tables cover exactly the fixture's alias documents", () => {
      assert.deepEqual(
        Object.keys(CHORD_NOTATION_ALIASES).sort(),
        Object.keys(fixture.legacyAliases.chordNotations).sort()
      );
      assert.deepEqual(
        Object.keys(FLOW_TYPE_ALIASES).sort(),
        Object.keys(fixture.legacyAliases.flowTypes).sort()
      );
    });

    it("a legacy song with `latin` notation and chordpro still opens", () => {
      const song = fixture.legacyAliases.song;
      assert.equal(song.chords?.notation, "latin");
      assert.equal(canonicalChordNotation(song.chords!.notation), "ABC");
      assert.equal(song.chords?.chordpro, "[G]Line one");
    });

    it("a legacy agenda with `Manual` / `loop` / `Default` still opens", () => {
      const parsed = parseAgenda(JSON.stringify(fixture.legacyAliases.agenda));
      const flows = parsed.items.map((i) => i.flowType);
      assert.deepEqual(flows.map((f) => canonicalFlowType(f!)), ["manual", "Loop", "default"]);
    });
  });

  describe("shipped sample files still parse", () => {
    it("data/agenda.sample.json opens, including its `Loop` item", () => {
      const p = path.join(__dirname, "..", "..", "data", "agenda.sample.json");
      const parsed = parseAgenda(fs.readFileSync(p, "utf8"));
      const flows = parsed.items.map((i) => i.flowType);
      assert.ok(flows.every((f) => isFlowType(f!)), "every shipped flowType is a known literal");
      assert.ok(flows.includes("Loop"), "the shipped sample has a Loop item");
      for (const item of parsed.items) assert.ok(isAgendaItemKind(item.kind));
    });

    it("data/songs.sample.json carries `latin` notation and chordpro", () => {
      const p = path.join(__dirname, "..", "..", "data", "songs.sample.json");
      const songs: Song[] = JSON.parse(fs.readFileSync(p, "utf8"));
      const withChords = songs.find((s) => s.chords);
      assert.equal(withChords?.chords?.notation, "latin");
      assert.equal(canonicalChordNotation(withChords!.chords!.notation), "ABC");
      assert.ok(withChords?.chords?.chordpro);
      assert.ok(songs.every((s) => typeof s.updatedAt === "string"));
    });
  });
});