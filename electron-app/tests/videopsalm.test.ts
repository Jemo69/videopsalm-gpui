// Native VideoPsalm songbook support: dialect parser, shared-Song mapping,
// write-through edits (unknown fields preserved), and document-folder scan.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import {
  applySongToVideopsalm,
  parseVideopsalm,
  readVideopsalmSongbook,
  resolveVideopsalmDataFolder,
  saveSongToVideopsalm,
  scanVideopsalmFolder,
  stringifyVideopsalm,
  videopsalmFolderCandidates,
  videopsalmToSongs,
  type VpObject,
} from "../src/core/videopsalm";
import { buildVpcEntry, readVpcEntry } from "../src/core/vpc";
import type { Song } from "../src/core/schemas";

const SAMPLE = `\uFEFF{Abbreviation:"SB1",Copyright:"Public domain",Songs:[{ID:3,Composer:"Unknown",Author:"Martin Luther",Capo:-1,Copyright:"Public
Domain",Theme:"tema1
tema2",CCLI:"12345",Alias:"A safe stronghold",Memo1:"This is
the first comment
",Memo2:"This is
the second comment
",Memo3:"This is
the third comment
",Reference:"reference",CustomField:"kept",Guid:"jtCkrJdPIUOmECjaQylg/g",Verses:[{
Text:"As safe a stronghold our God is still,
A trusty shield and weapon;
He’ll help us clear from all the ill
That hath us now o’ertaken."},{ID:2,
Text:"With \\"force\\" of arms we nothing can,
Full soon were we down-ridden;
But for us fights \\\\ the proper Man."}],AudioFile:"282.mp3",IsAudioFileEnabled:1,
Text:"A Safe Stronghold Our God is Still",Sequence:"V1 V2",VerseOrderIndex:0}],Guid:"khiHU2blX0Kb41dGdbDLhA",VersionDate:"20121012000000",
Text:"SongBook1"}`;

describe("VideoPsalm native format", () => {
  it("parses unquoted keys, literal newlines, escapes, negative numbers and BOM", () => {
    const root = parseVideopsalm(SAMPLE) as VpObject;
    assert.equal(root.Abbreviation, "SB1");
    assert.equal(root.Guid, "khiHU2blX0Kb41dGdbDLhA");
    const song = (root.Songs as VpObject[])[0] as VpObject;
    assert.equal(song.ID, 3);
    assert.equal(song.Capo, -1);
    assert.equal(song["CustomField"], "kept");
    assert.match(String(song.Text), /A Safe Stronghold/);
    const verses = song.Verses as VpObject[];
    assert.equal(verses.length, 2);
    assert.match(String(verses[1].Text), /With "force" of arms/);
    assert.match(String(verses[1].Text), /fights \\ the proper Man/);
    assert.doesNotMatch(String(verses[0].Text), /^\n/);
  });

  it("round-trips the AST through the native serialiser", () => {
    const root = parseVideopsalm(SAMPLE) as VpObject;
    const out = stringifyVideopsalm(root);
    assert.match(out, /^\{Abbreviation:"SB1"/);
    assert.doesNotMatch(out, /"Abbreviation"/);
    assert.deepEqual(parseVideopsalm(out), root);
  });

  it("maps native fields to the shared Song shape", () => {
    const imported = videopsalmToSongs(parseVideopsalm(SAMPLE), "/tmp/Songbooks/Book.json");
    assert.equal(imported.songbook.name, "SongBook1");
    assert.equal(imported.songbook.abbrev, "SB1");
    assert.equal(imported.songbook.guid, "khiHU2blX0Kb41dGdbDLhA");
    assert.equal(imported.songs.length, 1);
    const song = imported.songs[0];
    assert.equal(song.title, "A Safe Stronghold Our God is Still");
    assert.deepEqual(song.authors, ["Martin Luther"]);
    assert.equal(song.composer, "Unknown");
    assert.equal(song.ccli, "12345");
    assert.equal(song.copyright, "Public Domain");
    assert.deepEqual(song.themes, ["tema1", "tema2"]);
    assert.deepEqual(song.references, ["reference"]);
    assert.equal(song.memo1, "This is\nthe first comment\n");
    assert.equal(song.verses.length, 2);
    assert.equal(song.verses[0].id, "1");
    assert.equal(song.verses[1].id, "2");
    assert.equal(song.verses[0].tag, "V");
    assert.equal(song.verses[0].tagId, 1);
    assert.equal(song.verses[1].tagId, 2);
    assert.equal(song.sequence, "V1 V2");
    assert.equal(song.verseOrderIndex, 0);
    assert.equal(song.source?.format, "videopsalm");
    assert.equal(song.source?.guid, "jtCkrJdPIUOmECjaQylg/g");
    assert.match(song.id, /^videopsalm:khiHU2blX0Kb41dGdbDLhA:/);
  });

  it("writes lyric/title edits back without dropping unknown native fields", () => {
    const root = parseVideopsalm(SAMPLE);
    const imported = videopsalmToSongs(root, "/tmp/Songbooks/Book.json");
    const song: Song = imported.songs[0];
    song.title = "Edited title";
    song.verses = [
      { id: "1", lines: ["First line", "Second line"], order: 1, tag: "V", tagId: 1 },
      { id: "2", lines: ["New second verse", "", "Second slide"], order: 2, tag: "C", tagId: 2 },
      { id: "3", lines: ["Brand new verse"], order: 3, tag: "B", tagId: 0 },
    ];
    applySongToVideopsalm(root, song);
    const out = stringifyVideopsalm(root);
    const reparsed = parseVideopsalm(out) as VpObject;
    const native = (reparsed.Songs as VpObject[])[0] as VpObject;
    assert.equal(native.Text, "Edited title");
    assert.equal(native["CustomField"], "kept");
    assert.equal(native.Capo, -1);
    assert.equal(native.Guid, "jtCkrJdPIUOmECjaQylg/g");
    assert.equal(native.Sequence, "V1 V2");
    assert.equal(native.VerseOrderIndex, 0);
    const verses = native.Verses as VpObject[];
    assert.equal(verses.length, 3);
    // V1 is the serializer default: Tag/ID are intentionally omitted.
    assert.equal(verses[0].ID, undefined);
    assert.equal(verses[0].Tag, undefined);
    assert.equal(verses[1].Tag, 1); // Chorus
    assert.equal(verses[1].ID, 2);
    assert.equal(verses[1].Text, "New second verse\n\u00a0\nSecond slide");
    assert.equal(verses[2].Tag, 3); // Bridge
    assert.equal(verses[2].ID, 0); // B with no number
    assert.equal(verses[0].Text, "First line\nSecond line");
    assert.equal(verses[2].Text, "Brand new verse");
    assert.match(String(reparsed.VersionDate), /^\d{14}$/);
  });

  it("writes a real file in place and preserves a BOM", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-native-"));
    try {
      const file = path.join(dir, "Book.json");
      fs.writeFileSync(file, SAMPLE, "utf8");
      const imported = readVideopsalmSongbook(file);
      const song = imported.songs[0];
      song.verses[0].lines = ["Changed in place"];
      saveSongToVideopsalm(file, song);
      const raw = fs.readFileSync(file, "utf8");
      assert.equal(raw.charCodeAt(0), 0xfeff);
      const again = readVideopsalmSongbook(file);
      assert.equal(again.songs[0].verses[0].lines[0], "Changed in place");
      assert.equal(again.songs[0].memo1, "This is\nthe first comment\n");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("reads and re-encrypts a protected .vpc archive", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-vpc-"));
    try {
      const file = path.join(dir, "Book.vpc");
      fs.writeFileSync(file, buildVpcEntry("Book.json", Buffer.from(SAMPLE, "utf8")));
      const entry = readVpcEntry(file);
      assert.equal(entry.name, "Book.json");
      assert.equal(entry.encrypted, true);
      assert.match(entry.data.toString("utf8"), /A Safe Stronghold/);

      const imported = readVideopsalmSongbook(file);
      assert.equal(imported.songbook.name, "SongBook1");
      assert.equal(imported.songs[0].source?.file, file);
      imported.songs[0].verses[0].lines = ["Edited inside vpc"];
      saveSongToVideopsalm(file, imported.songs[0]);

      assert.ok(fs.existsSync(`${file}.bak`));
      const again = readVideopsalmSongbook(file);
      assert.equal(again.songs[0].verses[0].lines[0], "Edited inside vpc");
      assert.equal(readVpcEntry(file).encrypted, true);
      assert.match(readVpcEntry(file).data.toString("utf8"), /^\{/);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("writes only the metadata fields the operator edited", () => {
    // A native value that the editor does not own must never be rewritten;
    // an edited field is written from the editor (and removed when emptied).
    const song: Song = {
      id: "videopsalm:jtCkrJdPIUOmECjaQylg/g:3",
      title: "A Safe Stronghold Our God is Still",
      verses: [{ id: "1", lines: ["line"], order: 1, tag: "V", tagId: 1 }],
      authors: ["Someone Else"],
      composer: "Unknown",
      ccli: "12345",
      copyright: "Public Domain",
      themes: ["tema1"],
      references: ["reference"],
      memo1: "",
      memo2: "",
      memo3: "",
      createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z",
    };
    const root = parseVideopsalm(SAMPLE) as VpObject;
    const raw = (root.Songs as VpObject[])[0] as VpObject;

    // No edit list: the native values stay as they are.
    applySongToVideopsalm(root, song);
    assert.equal(raw.Author, "Martin Luther");
    assert.equal(raw.Composer, "Unknown");
    assert.equal(raw.CCLI, "12345");
    assert.equal(raw.Theme, "tema1\ntema2");
    assert.equal(raw.Memo1, "This is\nthe first comment\n");

    // The operator edits the author and clears the memo: only those change.
    const edited: Song = { ...song, authors: ["Someone Else"], memo1: "" };
    applySongToVideopsalm(root, edited, ["author", "memo1"]);
    assert.equal(raw.Author, "Someone Else");
    assert.equal(raw.Memo1, undefined);
    assert.equal(raw.Composer, "Unknown");
    assert.equal(raw.Theme, "tema1\ntema2");

    // The verse order: picking an order writes the index; "document order"
    // removes it again (VideoPsalm's default is -1).
    const ordered: Song = { ...song, verseOrderIndex: 1, sequence: "V1 C2 V1, C V1" };
    applySongToVideopsalm(root, ordered, ["verseOrderIndex", "sequence"]);
    assert.equal(raw.VerseOrderIndex, 1);
    assert.equal(raw.Sequence, "V1 C2 V1, C V1");
    const back: Song = { ...ordered, verseOrderIndex: undefined };
    applySongToVideopsalm(root, back, ["verseOrderIndex"]);
    assert.equal(raw.VerseOrderIndex, undefined);
  });

  it("reads a real 7-Zip WinZip-AES .vpc fixture", () => {
    // Created with:
    //   7z a -tzip -mem=AES256 -mm=Deflate -p<fixed-password> \
    //     videopsalm-aes.vpc Fixture.json
    // This is deliberately not produced by buildVpcEntry(): it catches a
    // decryptor that only round-trips its own writer.
    const file = path.join(__dirname, "..", "..", "tests", "fixtures", "videopsalm-aes.vpc");
    const entry = readVpcEntry(file);
    assert.equal(entry.name, "Fixture.json");
    assert.equal(entry.encrypted, true);
    assert.match(entry.data.toString("utf8"), /AES Fixture/);
    const imported = readVideopsalmSongbook(file);
    assert.equal(imported.songbook.name, "AES Fixture Book");
    assert.equal(imported.songs[0].title, "AES Fixture");
    assert.equal(imported.songs[0].sequence, "V1 C2 V1");
  });

  it("derives the same shared Song fields the Rust port derives", () => {
    // The same fixture is asserted by gpui-app/vp-core/src/native_songbook.rs,
    // so the two ports cannot drift on ids, labels, or metadata.
    const fixture = JSON.parse(
      fs.readFileSync(path.join(__dirname, "..", "..", "tests", "fixtures", "native-songbook.json"), "utf8")
    ) as {
      file: string;
      native: string;
      expected: {
        songbook: { name: string; abbrev: string; guid: string; songCount: number };
        song: Record<string, unknown> & {
          id: string;
          title: string;
          verses: { id: string; order: number; tag: string; tagId: number; lines: string[] }[];
          source: Record<string, string>;
        };
      };
    };
    const imported = videopsalmToSongs(parseVideopsalm(fixture.native), fixture.file);
    const want = fixture.expected;
    assert.equal(imported.songbook.name, want.songbook.name);
    assert.equal(imported.songbook.abbrev, want.songbook.abbrev);
    assert.equal(imported.songbook.guid, want.songbook.guid);
    assert.equal(imported.songs.length, want.songbook.songCount);

    const song = imported.songs[0];
    assert.equal(song.id, want.song.id);
    assert.equal(song.title, want.song.title);
    assert.deepEqual(song.authors, want.song.authors);
    assert.equal(song.composer, want.song.composer);
    assert.equal(song.ccli, want.song.ccli);
    assert.equal(song.copyright, want.song.copyright);
    assert.deepEqual(song.themes, want.song.themes);
    assert.deepEqual(song.references, want.song.references);
    assert.equal(song.memo1, want.song.memo1);
    assert.equal(song.songId, want.song.songId);
    assert.equal(song.songbookId, want.song.songbookId);
    assert.equal(song.sequence, want.song.sequence);
    assert.equal(song.verseOrderIndex, want.song.verseOrderIndex);
    assert.deepEqual(
      song.verses.map((v) => ({ id: v.id, order: v.order, tag: v.tag, tagId: v.tagId, lines: v.lines })),
      want.song.verses
    );
    assert.deepEqual(song.source, want.song.source);
  });

  it("preserves an unprotected .vpc as unprotected", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-vpc-plain-"));
    try {
      const file = path.join(dir, "Plain.vpc");
      fs.writeFileSync(file, buildVpcEntry("Plain.json", Buffer.from(SAMPLE, "utf8"), { encrypt: false }));
      assert.equal(readVpcEntry(file).encrypted, false);
      const imported = readVideopsalmSongbook(file);
      imported.songs[0].title = "Unprotected edit";
      saveSongToVideopsalm(file, imported.songs[0]);
      assert.equal(readVpcEntry(file).encrypted, false);
      assert.equal(readVideopsalmSongbook(file).songs[0].title, "Unprotected edit");
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("VideoPsalm document folder scan", () => {
  it("follows CommonSettings.DataBaseFolder when the documents were moved", () => {
    const outer = fs.mkdtempSync(path.join(os.tmpdir(), "vp-pointer-"));
    const data = path.join(outer, "moved-data");
    try {
      fs.mkdirSync(path.join(data, "SongBooks"), { recursive: true });
      fs.writeFileSync(path.join(data, "SongBooks", "Book.json"), SAMPLE, "utf8");
      fs.writeFileSync(
        path.join(outer, "CommonSettings.json"),
        `{DataBaseFolder:"${data.replace(/\\/g, "\\\\")}"}`,
        "utf8"
      );
      const scan = scanVideopsalmFolder(outer);
      assert.equal(scan.folder, data);
      assert.equal(scan.songbooks.length, 1);
      assert.equal(scan.songs.length, 1);
    } finally {
      fs.rmSync(outer, { recursive: true, force: true });
    }
  });

  it("imports songbooks, finds Images/Videos, and resolves style backgrounds", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "vp-folder-"));
    try {
      // Exact native casing (`SongBooks`), plus the song-scoped media
      // folders and the generated `Thumbs/` cache that galleries exclude.
      fs.mkdirSync(path.join(dir, "SongBooks", "Images"), { recursive: true });
      fs.mkdirSync(path.join(dir, "SongBooks", "Videos"), { recursive: true });
      fs.mkdirSync(path.join(dir, "Images", "Thumbs"), { recursive: true });
      fs.mkdirSync(path.join(dir, "Videos"));
      fs.writeFileSync(path.join(dir, "SongBooks", "Book.json"), SAMPLE, "utf8");
      fs.writeFileSync(
        path.join(dir, "SongBooks", "Encrypted.vpc"),
        buildVpcEntry(
          "Encrypted.json",
          Buffer.from(SAMPLE.replace("SongBook1", "VPC Book").replace("ID:3", "ID:4"), "utf8")
        )
      );
      fs.writeFileSync(path.join(dir, "Images", "sunset.jpg"), "not-an-image");
      fs.writeFileSync(path.join(dir, "Images", "Thumbs", "thumb.jpg"), "not-an-image");
      fs.writeFileSync(path.join(dir, "Videos", "loop.mp4"), "not-a-video");
      fs.writeFileSync(path.join(dir, "SongBooks", "Images", "verse.jpg"), "not-an-image");
      fs.writeFileSync(path.join(dir, "SongBooks", "Videos", "verse.mp4"), "not-a-video");
      fs.writeFileSync(
        path.join(dir, "RootStyle.json"),
        `{BackgroundStyle:{ImageFile:"Images\\\\sunset.jpg",VideoFile:"Videos/loop.mp4"}}`,
        "utf8"
      );
      const scan = scanVideopsalmFolder(dir);
      assert.equal(scan.songbooks.length, 2);
      assert.equal(scan.songs.length, 2);
      assert.ok(scan.songbooks.some((b) => b.file.endsWith("Encrypted.vpc")));
      assert.ok(scan.images.some((f) => f.endsWith("sunset.jpg")));
      assert.ok(scan.images.some((f) => f.endsWith("verse.jpg")));
      assert.ok(scan.videos.some((f) => f.endsWith("loop.mp4")));
      assert.ok(scan.videos.some((f) => f.endsWith("verse.mp4")));
      assert.ok(!scan.images.some((f) => f.endsWith("thumb.jpg")));
      assert.ok(scan.backgrounds.some((b) => b.kind === "image" && b.file.endsWith("sunset.jpg")));
      assert.ok(scan.backgrounds.some((b) => b.kind === "videoLoop" && b.file.endsWith("loop.mp4")));
      assert.equal(scan.errors.length, 0);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it("first-boot auto-find: default candidates resolve to a scannable folder", () => {
    // The renderer auto-finds on first boot with no saved folder: main picks
    // the first existing candidate (videopsalmFolderCandidates) and follows
    // CommonSettings (resolveVideopsalmDataFolder), then scans it. This pins
    // that pure chain end-to-end (the bridge/apply glue needs a DOM).
    const base = fs.mkdtempSync(path.join(os.tmpdir(), "vp-autofind-"));
    const home = path.join(base, "home");
    const docs = path.join(base, "Documents");
    const vpDir = path.join(docs, "VideoPsalm");
    const prevFolder = process.env["VIDEOPSALM_FOLDER"];
    const prevPublic = process.env["PUBLIC"];
    try {
      fs.mkdirSync(path.join(vpDir, "SongBooks"), { recursive: true });
      fs.writeFileSync(path.join(vpDir, "SongBooks", "Boot.json"), SAMPLE, "utf8");
      // Keep candidates hermetic: blank override/public so the home/documents
      // args decide. Empty strings are filtered by videopsalmFolderCandidates.
      process.env["VIDEOPSALM_FOLDER"] = "";
      delete process.env["PUBLIC"];

      const cands = videopsalmFolderCandidates(home, docs);
      // documents/VideoPsalm is the most-specific non-env candidate.
      assert.equal(cands[0], vpDir);
      // Mirror main.ts defaultVideopsalmFolder: first existing directory wins.
      const hit = cands.find((d) => {
        try { return fs.existsSync(d) && fs.statSync(d).isDirectory(); }
        catch { return false; }
      });
      assert.equal(hit, vpDir);
      const resolved = resolveVideopsalmDataFolder(hit!);
      const scan = scanVideopsalmFolder(resolved);
      assert.equal(scan.songbooks.length, 1);
      assert.equal(scan.songs.length, 1);
      assert.equal(scan.errors.length, 0);
    } finally {
      if (prevFolder === undefined) delete process.env["VIDEOPSALM_FOLDER"];
      else process.env["VIDEOPSALM_FOLDER"] = prevFolder;
      if (prevPublic === undefined) delete process.env["PUBLIC"];
      else process.env["PUBLIC"] = prevPublic;
      fs.rmSync(base, { recursive: true, force: true });
    }
  });
});
