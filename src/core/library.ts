// SQLite-backed local library (songs, bibles metadata, agenda recents, settings).
// Primary: better-sqlite3 when installed and loadable; fallback: JSON-file
// store in the user directory. Offline-first, no network.
//
// NOTE: an earlier revision carried a `createSqlJsBackend` (sql.js/wasm)
// stub whose sync `all()`/`run()` threw ("async-only") — a crash on first
// query whenever it was reached. It had no callers (`openLibrary` below
// already fell through to the JSON store), so it was deleted outright rather
// than fixed: one working fallback beats two half-wired ones. If a wasm
// backend is ever wanted again it must implement the full sync `Library`
// interface with a regression test, not a throwing stub.

import * as fs from "fs";
import * as path from "path";
import type { Song } from "./schemas";

export interface LibraryCounts {
  songs: number;
  recents: number;
}

export interface Library {
  backend: string;
  addSong(song: Song): void;
  findSongs(query: string): Song[];
  getSong(id: string): Song | undefined;
  recordRecent(agendaPath: string): void;
  recentAgendas(): string[];
  close(): void;
}

/** JSON-file fallback library (also used when native builds are unavailable). */
export function createJsonLibrary(dir: string): Library {
  const songsPath = path.join(dir, "songs.json");
  const recentsPath = path.join(dir, "recents.json");
  const read = <T>(p: string, fallback: T): T => {
    try {
      return JSON.parse(fs.readFileSync(p, "utf8")) as T;
    } catch {
      return fallback;
    }
  };
  const write = (p: string, v: unknown) => {
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, JSON.stringify(v, null, 2));
  };
  const norm = (s: string) =>
    s
      .normalize("NFD")
      .replace(/[̀-ͯ]/g, "")
      .toLowerCase();
  return {
    backend: "json-file",
    addSong(song: Song): void {
      const songs = read<Song[]>(songsPath, []);
      const i = songs.findIndex((s) => s.id === song.id);
      if (i >= 0) songs[i] = song;
      else songs.push(song);
      write(songsPath, songs);
    },
    findSongs(query: string): Song[] {
      const songs = read<Song[]>(songsPath, []);
      const q = norm(query.trim());
      if (!q) return songs;
      return songs.filter((s) =>
        norm([s.title, s.songId ?? "", s.ccli ?? "", ...s.authors, ...s.verses.flatMap((v) => v.lines)].join(" ")).includes(q)
      );
    },
    getSong(id: string): Song | undefined {
      return read<Song[]>(songsPath, []).find((s) => s.id === id);
    },
    recordRecent(agendaPath: string): void {
      const recents = read<string[]>(recentsPath, []).filter((r) => r !== agendaPath);
      recents.unshift(agendaPath);
      write(recentsPath, recents.slice(0, 10));
    },
    recentAgendas(): string[] {
      return read<string[]>(recentsPath, []);
    },
    close(): void {},
  };
}

/** Open the library: better-sqlite3 if loadable, else JSON-file store. */
export function openLibrary(dir: string): Library {
  try {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const Database = require("better-sqlite3") as new (p: string) => BetterDb;
    interface BetterDb {
      exec(sql: string): void;
      prepare(sql: string): { all(...p: unknown[]): Song[]; get(...p: unknown[]): Song | undefined; run(...p: unknown[]): void };
      close(): void;
    }
    fs.mkdirSync(dir, { recursive: true });
    const db = new Database(path.join(dir, "library.db"));
    db.exec(`CREATE TABLE IF NOT EXISTS songs (id TEXT PRIMARY KEY, data TEXT);
             CREATE TABLE IF NOT EXISTS recents (path TEXT PRIMARY KEY, opened_at TEXT);`);
    const norm = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
    return {
      backend: "better-sqlite3",
      addSong(song: Song): void {
        db.prepare("INSERT OR REPLACE INTO songs (id, data) VALUES (?, ?)").run(song.id, JSON.stringify(song));
      },
      findSongs(query: string): Song[] {
        const allSongs = (db.prepare("SELECT data FROM songs").all() as unknown as { data: string }[]).map(
          (r) => JSON.parse(r.data) as Song
        );
        const q = norm(query.trim());
        if (!q) return allSongs;
        return allSongs.filter((s) =>
          norm([s.title, s.songId ?? "", s.ccli ?? "", ...s.authors, ...s.verses.flatMap((v) => v.lines)].join(" ")).includes(q)
        );
      },
      getSong(id: string): Song | undefined {
        const row = (db.prepare("SELECT data FROM songs WHERE id = ?").get(id) as unknown as { data: string } | undefined);
        return row ? (JSON.parse(row.data) as Song) : undefined;
      },
      recordRecent(agendaPath: string): void {
        db.prepare("INSERT OR REPLACE INTO recents (path, opened_at) VALUES (?, ?)").run(agendaPath, new Date().toISOString());
      },
      recentAgendas(): string[] {
        return (db.prepare("SELECT path FROM recents ORDER BY opened_at DESC").all() as unknown as { path: string }[]).map(
          (r) => r.path
        );
      },
      close(): void {
        db.close();
      },
    };
  } catch {
    return createJsonLibrary(dir);
  }
}
