// Library storage regression tests.
//
// Guards the audit finding that the sql.js/wasm fallback stub threw
// ("async-only") on first query: `openLibrary` must ALWAYS return a working
// sync `Library` — better-sqlite3 when loadable, JSON-file store otherwise —
// and a song written through it must survive a close/reopen round-trip.

import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { openLibrary, createJsonLibrary, type Library } from "../src/core/library";
import type { Song } from "../src/core/schemas";

function sampleSong(id: string): Song {
  return {
    id,
    title: "Amazing Grace",
    verses: [{ id: "V1", lines: ["Amazing grace! how sweet the sound"], order: 1 }],
    authors: ["John Newton"],
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-02T03:04:05Z",
  };
}

describe("library storage", () => {
  let dir = "";
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), "perez-lib-"));
  });
  afterEach(() => {
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it("openLibrary returns a working library on this machine (any backend)", () => {
    const lib: Library = openLibrary(dir);
    assert.ok(["better-sqlite3", "json-file"].includes(lib.backend), `unknown backend ${lib.backend}`);
    lib.addSong(sampleSong("s1"));
    assert.deepEqual(lib.findSongs("amazing").map((s) => s.id), ["s1"]);
    assert.equal(lib.getSong("s1")?.title, "Amazing Grace");
    // Queries must not throw — the old sql.js stub threw here.
    assert.doesNotThrow(() => lib.findSongs(""));
    assert.doesNotThrow(() => lib.getSong("missing"));
    lib.recordRecent("/tmp/service.vpagd");
    assert.deepEqual(lib.recentAgendas(), ["/tmp/service.vpagd"]);
    lib.close();
  });

  it("the JSON-file store survives close/reopen", () => {
    const first = createJsonLibrary(dir);
    assert.equal(first.backend, "json-file");
    first.addSong(sampleSong("s1"));
    first.recordRecent("/tmp/service.vpagd");
    first.close();

    const second = createJsonLibrary(dir);
    assert.equal(second.getSong("s1")?.title, "Amazing Grace");
    assert.deepEqual(second.recentAgendas(), ["/tmp/service.vpagd"]);
    // Diacritic-insensitive find still works after reopen.
    second.addSong({ ...sampleSong("s2"), title: "À toi la gloire" });
    assert.deepEqual(second.findSongs("a toi").map((s) => s.id), ["s2"]);
    second.close();
  });

  it("findSongs matches across title, authors and verse lines", () => {
    const lib = createJsonLibrary(dir);
    lib.addSong(sampleSong("s1"));
    assert.equal(lib.findSongs("newton").length, 1);
    assert.equal(lib.findSongs("sweet the sound").length, 1);
    assert.equal(lib.findSongs("no such song").length, 0);
    lib.close();
  });
});
