import { describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import { createAgenda, addItem, moveItem, removeItem, renameItem, serializeAgenda, parseAgenda } from "../src/core/agendaSerde";

describe("agenda-serde", () => {
  it("round-trips create/add/serialize/parse", () => {
    const a = createAgenda("Sunday");
    addItem(a, "Song", "Amazing Grace", { songId: "song-amazing-grace" });
    addItem(a, "BibleVerses", "John 3:16", {
      bibleRef: { translationId: "sample-en", book: "John", chapter: 3, verses: [16], versification: "EN" },
    });
    const back = parseAgenda(serializeAgenda(a));
    assert.equal(back.title, "Sunday");
    assert.equal(back.items.length, 2);
    assert.equal(back.items[0].kind, "Song");
  });

  it("moves, renames and removes items", () => {
    const a = createAgenda("T");
    const x = addItem(a, "Alert", "X", { text: "x" });
    addItem(a, "Clock", "Y", { text: "clock" });
    assert.equal(moveItem(a, x.id, 1), true);
    assert.equal(a.items[1].id, x.id);
    assert.equal(renameItem(a, x.id, "Z"), true);
    assert.equal(a.items[1].title, "Z");
    assert.equal(removeItem(a, x.id), true);
    assert.equal(a.items.length, 1);
    assert.equal(moveItem(a, "nope", 1), false);
  });

  it("rejects bad versions, kinds and malformed JSON", () => {
    assert.throws(() => parseAgenda("{nope"));
    assert.throws(() => parseAgenda(JSON.stringify({ version: 2, title: "x", items: [] })));
    assert.throws(() =>
      parseAgenda(JSON.stringify({ version: 1, title: "x", items: [{ id: "a", kind: "Nope", title: "t" }] }))
    );
  });

  it("the shipped sample agenda parses", () => {
    const p = path.join(__dirname, "..", "..", "data", "agenda.sample.json");
    const back = parseAgenda(fs.readFileSync(p, "utf8"));
    assert.ok(back.items.length >= 12);
    const kinds = new Set(back.items.map((i) => i.kind));
    for (const k of ["Song", "BibleVerses", "Image", "Video", "Audio", "Pdf", "PowerPoint", "YouTube", "Clock", "Alert"]) {
      assert.ok(kinds.has(k as never), `sample agenda missing kind ${k}`);
    }
  });

  it("the shipped sample songs match the Song schema", () => {
    const p = path.join(__dirname, "..", "..", "data", "songs.sample.json");
    const songs = JSON.parse(fs.readFileSync(p, "utf8")) as { id: string; title: string; verses: { id: string; lines: string[] }[] }[];
    assert.equal(songs.length, 2);
    assert.ok(songs.every((s) => s.id && s.title && Array.isArray(s.verses) && s.verses.length > 0));
  });
});
