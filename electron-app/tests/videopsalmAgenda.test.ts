// Native VideoPsalm service plan (`.vpagd`) import.
//
// The plan is a ZIP of per-item projections (`Song_0.json`, `SongBook_0.json`,
// `BibleVerses_0.json`, `BibleBook_0.json`, …) in agenda order, plus the
// media blobs. The importer maps that onto the shared agenda schema and the
// shared song schema.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { buildVideopsalmAgendaEntries, readVideopsalmAgenda, writeVideopsalmAgenda } from "../src/core/videopsalmAgenda";
import { parseVideopsalm } from "../src/core/videopsalm";
import type { Agenda, Song } from "../src/core/schemas";
import { buildZipEntries } from "../src/core/vpc";
import { parseAgenda, serializeAgenda } from "../src/core/agendaSerde";

const SONG = '{Text:"Amazing Grace",Guid:"song-guid-1",Author:"John Newton",CCLI:"2762838",Verses:[{Text:"Amazing grace how sweet the sound"},{Text:"That saved a wretch like me"}]}';
const SONG_BOOK = '{Text:"Hymns",Abbreviation:"HYM",Guid:"book-guid-1"}';
const VERSES = '{Text:"John 3:16-17",Verses:[{ID:16,Text:"For God so loved the world."},{ID:17,Text:"For God did not send his Son."}]}';
const BIBLE_BOOK = '{Text:"John",ID:43}';
const BIBLE_CHAPTER = '{ID:3}';
const BIBLE = '{Text:"KJV"}';
const IMAGE = '{Text:"Welcome",FileName:"welcome.jpg"}';

function plan(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-agenda-"));
  const file = path.join(dir, "Sunday.vpagd");
  fs.writeFileSync(
    file,
    buildZipEntries([
      { name: "RootStyle.json", data: Buffer.from("{BackgroundStyle:{}}", "utf8") },
      { name: "Song_0.json", data: Buffer.from(SONG, "utf8") },
      { name: "SongBook_0.json", data: Buffer.from(SONG_BOOK, "utf8") },
      { name: "BibleVerses_0.json", data: Buffer.from(VERSES, "utf8") },
      { name: "BibleBook_0.json", data: Buffer.from(BIBLE_BOOK, "utf8") },
      { name: "BibleChapter_0.json", data: Buffer.from(BIBLE_CHAPTER, "utf8") },
      { name: "Bible_0.json", data: Buffer.from(BIBLE, "utf8") },
      { name: "Image_0.json", data: Buffer.from(IMAGE, "utf8") },
      { name: "welcome.jpg", data: Buffer.from("not-really-a-jpeg", "utf8") },
    ])
  );
  return file;
}

describe("VideoPsalm service plan import", () => {
  it("maps the plan onto the shared agenda, in the plan's order", () => {
    const file = plan();
    try {
      const imported = readVideopsalmAgenda(file);
      assert.equal(imported.title, "Sunday.vpagd");
      assert.equal(imported.items.length, 3);
      assert.deepEqual(imported.items.map((i) => i.kind), ["Song", "BibleVerses", "Image"]);

      const song = imported.items[0];
      assert.equal(song.title, "Amazing Grace");
      assert.equal(song.ref?.songId, "videopsalm:book-guid-1:song-guid-1");
      assert.equal(song.flowType, "Manual");

      const bible = imported.items[1];
      assert.equal(bible.title, "John 3:16-17");
      assert.deepEqual(bible.ref?.bibleRef, {
        translationId: "KJV",
        book: "John",
        chapter: 3,
        verses: [16, 17],
        versification: "EN",
      });
      assert.match(bible.ref?.text ?? "", /16 For God so loved the world\./);
      assert.match(bible.ref?.text ?? "", /17 For God did not send his Son\./);

      const image = imported.items[2];
      assert.equal(image.title, "Welcome");
      assert.ok(image.ref?.file?.endsWith("welcome.jpg"));
      assert.equal(imported.warnings.length, 0);
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  });

  it("carries the plan's songs, without a write-back target", () => {
    const file = plan();
    try {
      const imported = readVideopsalmAgenda(file);
      assert.equal(imported.songs.length, 1);
      const song = imported.songs[0];
      assert.equal(song.title, "Amazing Grace");
      assert.equal(song.id, "videopsalm:book-guid-1:song-guid-1");
      assert.equal(song.verses.length, 2);
      assert.equal(song.verses[0].lines[0], "Amazing grace how sweet the sound");
      // The plan is a snapshot: it must never become a write-back target, or
      // an edit would try to save into the .vpagd archive.
      assert.equal(song.source, undefined);
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  });

  it("extracts the media the plan carries", () => {
    const file = plan();
    try {
      const imported = readVideopsalmAgenda(file);
      assert.equal(imported.media.length, 1);
      assert.ok(fs.existsSync(imported.media[0]));
      assert.equal(fs.readFileSync(imported.media[0], "utf8"), "not-really-a-jpeg");
      // The agenda item points at the extracted file, not the archive.
      const image = imported.items.find((i) => i.kind === "Image");
      assert.equal(image?.ref?.file, imported.media[0]);
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  });

  it("reports media the plan does not carry instead of failing", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-agenda-missing-"));
    try {
      const file = path.join(dir, "NoMedia.vpagd");
      fs.writeFileSync(
        file,
        buildZipEntries([{ name: "Image_0.json", data: Buffer.from(IMAGE, "utf8") }])
      );
      const imported = readVideopsalmAgenda(file);
      assert.equal(imported.items.length, 1);
      assert.equal(imported.items[0].ref?.file, undefined);
      assert.equal(imported.warnings.length, 1);
      assert.match(imported.warnings[0], /no media file/);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("survives being saved as this app's JSON agenda", () => {
    // The operator edits the imported plan here and saves it as JSON; the
    // items (including the Bible reference) must survive the round trip.
    const file = plan();
    try {
      const imported = readVideopsalmAgenda(file);
      const agenda = { version: 1 as const, title: imported.title, items: imported.items, settings: {} };
      const round = parseAgenda(serializeAgenda(agenda));
      assert.equal(round.title, "Sunday.vpagd");
      assert.equal(round.items.length, 3);
      assert.deepEqual(
        round.items.map((i) => i.kind),
        ["Song", "BibleVerses", "Image"]
      );
      assert.deepEqual(round.items[1].ref?.bibleRef?.verses, [16, 17]);
      assert.equal(round.items[1].ref?.bibleRef?.book, "John");
      assert.equal(round.items[0].ref?.songId, "videopsalm:book-guid-1:song-guid-1");
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  });
});

describe("VideoPsalm service plan export", () => {
  function librarySong(): Song {
    return {
      id: "videopsalm:book-guid-1:song-guid-1",
      title: "Amazing Grace",
      verses: [
        { id: "1", lines: ["Amazing grace how sweet the sound"], order: 1, tag: "V", tagId: 1 },
        { id: "2", lines: ["That saved a wretch like me"], order: 2, tag: "V", tagId: 2 },
      ],
      authors: ["John Newton"],
      ccli: "2762838",
      songbookId: "HYM",
      sequence: "V1 V2",
      verseOrderIndex: 0,
      createdAt: "2026-10-26T00:00:00.000Z",
      updatedAt: "2026-10-26T00:00:00.000Z",
      source: { format: "videopsalm", file: "/tmp/SongBooks/HYM.json", songbookGuid: "book-guid-1", songbookName: "HYM", guid: "song-guid-1" },
    };
  }

  function makeAgenda(imageFile: string): { agenda: Agenda; songs: Map<string, Song> } {
    const song = librarySong();
    return {
      songs: new Map([[song.id, song]]),
      agenda: {
        version: 1,
        title: "Sunday",
        items: [
          { id: "a1", kind: "Song", title: "Amazing Grace", ref: { songId: song.id }, flowType: "manual" },
          {
            id: "a2", kind: "BibleVerses", title: "John 3:16-17",
            ref: {
              bibleRef: { translationId: "KJV", book: "John", chapter: 3, verses: [16, 17], versification: "EN" },
              text: "16 For God so loved the world.\n17 For God did not send his Son.",
            },
            flowType: "AutoAdvance",
            autoAdvanceMs: 5000,
          },
          { id: "a3", kind: "Image", title: "Welcome", ref: { file: imageFile } },
          { id: "a4", kind: "WebSite", title: "Church", ref: { url: "https://example.test" } },
          { id: "a5", kind: "Alert", title: "Doors open" },
        ],
        settings: {},
      },
    };
  }

  it("writes the native entry set in agenda order", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-export-"));
    try {
      const mediaFile = path.join(dir, "welcome.jpg");
      fs.writeFileSync(mediaFile, "not-really-a-jpeg");
      const { agenda, songs } = makeAgenda(mediaFile);
      const { entries, media, warnings } = buildVideopsalmAgendaEntries(agenda, songs);
      const names = entries.map((e) => e.name);
      // The writer walks the agenda in order: the ZIP entry order IS the agenda order.
      assert.deepEqual(names, [
        "Version.json",
        "Song_0.json", "SongBook_0.json",
        "BibleVerses_0.json", "BibleChapter_0.json", "BibleBook_0.json", "Testament_0.json", "Bible_0.json",
        "Image_0.json",
        "WebSite_0.json",
        "welcome.jpg",
        "AgendaItemProperties.json",
      ]);
      assert.equal(entries[0].data.toString("utf8"), "2");
      assert.deepEqual(media, [mediaFile]);
      // Alert has no native projection: skipped with a warning, never silent.
      assert.equal(warnings.length, 1);
      assert.match(warnings[0], /Alert.*no native projection/);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("carries the per-item flow/advance/order row", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-export-props-"));
    try {
      const { agenda, songs } = makeAgenda(path.join(dir, "missing.jpg"));
      const { entries } = buildVideopsalmAgendaEntries(agenda, songs);
      const propsEntry = entries.find((e) => e.name === "AgendaItemProperties.json");
      assert.ok(propsEntry);
      const props = parseVideopsalm(propsEntry.data.toString("utf8")) as { Items: unknown[] };
      assert.ok(Array.isArray(props.Items));
      assert.equal(props.Items.length, 5);
      const rows = props.Items as Record<string, unknown>[];
      // Manual stepping -> Next (2); timed advance -> TextAndNext (3). Inferred (see writer header).
      assert.equal(rows[0].FlowType, 2);
      assert.equal(rows[1].FlowType, 3);
      assert.equal(rows[1].AutoAdvance, 1);
      assert.equal(rows[1].Interval, 5000);
      // The song's native verse order travels with the plan.
      assert.equal(rows[0].VerseOrderIndex, 0);
      assert.equal(rows[1].VerseOrderIndex, -1);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("round-trips through the importer", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-export-round-"));
    try {
      const mediaFile = path.join(dir, "welcome.jpg");
      fs.writeFileSync(mediaFile, "not-really-a-jpeg");
      const { agenda, songs } = makeAgenda(mediaFile);
      const outFile = path.join(dir, "Sunday.vpagd");
      const written = writeVideopsalmAgenda(outFile, agenda, songs);
      assert.equal(written.warnings.length, 1); // the Alert skip
      const imported = readVideopsalmAgenda(outFile);
      assert.deepEqual(imported.items.map((i) => i.kind), ["Song", "BibleVerses", "Image", "WebSite"]);
      assert.equal(imported.items[0].ref?.songId, "videopsalm:book-guid-1:song-guid-1");
      assert.deepEqual(imported.items[1].ref?.bibleRef?.verses, [16, 17]);
      assert.equal(imported.items[1].ref?.bibleRef?.book, "John");
      assert.match(imported.items[1].ref?.text ?? "", /16 For God so loved the world\./);
      assert.ok(imported.items[2].ref?.file?.endsWith("welcome.jpg"));
      assert.equal(imported.items[3].ref?.url, "https://example.test");
      // The song snapshot keeps its verses across the round trip.
      assert.equal(imported.songs[0].verses[1].lines[0], "That saved a wretch like me");
      assert.equal(imported.songs[0].verseOrderIndex, 0);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("links (with a warning) media that is not on disk", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-export-noblob-"));
    try {
      const { agenda, songs } = makeAgenda(path.join(dir, "gone.jpg"));
      const { entries, media, warnings } = buildVideopsalmAgendaEntries(agenda, songs);
      assert.deepEqual(media, []);
      assert.ok(!entries.some((e) => e.name === "gone.jpg"));
      assert.ok(warnings.some((w) => /linked by path only/.test(w)));
      const image = entries.find((e) => e.name === "Image_0.json");
      assert.ok(image);
      const projection = parseVideopsalm(image.data.toString("utf8")) as Record<string, unknown>;
      assert.equal(projection.FileName, path.join(dir, "gone.jpg"));
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
