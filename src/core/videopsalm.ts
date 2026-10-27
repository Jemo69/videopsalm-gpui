// Native VideoPsalm songbook + document-folder support.
//
// VideoPsalm stores each songbook as an individual `Songbooks/*.json` file;
// newer installs may use `.vpc`, a one-entry Ionic.Zip archive (WinZip AES-256
// when protected) containing the same JSON document. The JSON is *almost*
// JSON: object keys are unquoted, strings may contain literal newlines/tabs,
// and values often use 0/1 instead of booleans. This module parses that
// dialect into an AST, maps it to the shared `Song` shape, and writes edits
// back **in place** while preserving every unknown field.
//
// Clean-room reconstruction: the dialect was verified against the sample
// songbooks shipped with OpenLP's VideoPsalm importer and against the field
// names extracted from VideoPsalm 1.29's managed metadata (E-004).

import * as fs from "fs";
import * as path from "path";
import { createHash } from "crypto";
import { buildVpcEntry, readVpcEntry } from "./vpc";
import type { Song, SongSource } from "./schemas";

export type VpScalar = string | number | boolean | null;
export interface VpObject {
  [key: string]: VpJson;
}
export type VpJson = VpScalar | VpJson[] | VpObject;

export interface VideopsalmSongbookInfo {
  file: string;
  name: string;
  abbrev: string;
  guid: string;
  songCount: number;
}

export interface VideopsalmImport {
  songbook: VideopsalmSongbookInfo;
  songs: Song[];
}

export interface VideopsalmMedia {
  path: string;
  kind: "image" | "video";
}

export interface VideopsalmBackground {
  kind: "image" | "videoLoop";
  file: string;
  /** File the assignment came from (`RootStyle.json`, etc.). */
  source: string;
}

export interface VideopsalmFolderScan {
  folder: string;
  songs: Song[];
  songbooks: VideopsalmSongbookInfo[];
  images: string[];
  videos: string[];
  backgrounds: VideopsalmBackground[];
  errors: string[];
}

export class VideopsalmParseError extends Error {
  constructor(message: string, readonly position: number) {
    super(`${message} at character ${position}`);
    this.name = "VideopsalmParseError";
  }
}

// ---------------------------------------------------------------------------
// Tolerant parser for VideoPsalm's JSON dialect
// ---------------------------------------------------------------------------

class Parser {
  private i = 0;
  constructor(private readonly text: string) {}

  parse(): VpJson {
    this.skip();
    if (this.i >= this.text.length) throw new VideopsalmParseError("empty document", this.i);
    const value = this.value();
    this.skip();
    // A stray BOM / trailing control character is harmless in the native
    // writer. Anything else is a real syntax error and must be surfaced.
    if (this.i < this.text.length) {
      const rest = this.text.slice(this.i).trim();
      if (rest) throw new VideopsalmParseError(`unexpected ${JSON.stringify(rest.slice(0, 24))}`, this.i);
    }
    return value;
  }

  private skip(): void {
    while (this.i < this.text.length && /\s/.test(this.text[this.i])) this.i++;
  }

  private value(): VpJson {
    this.skip();
    const c = this.text[this.i];
    if (c === "{") return this.object();
    if (c === "[") return this.array();
    if (c === '"') return this.string();
    return this.bare();
  }

  private object(): VpObject {
    this.i++; // {
    const out: VpObject = {};
    this.skip();
    if (this.text[this.i] === "}") {
      this.i++;
      return out;
    }
    for (;;) {
      this.skip();
      if (this.text[this.i] === "}") {
        this.i++;
        break;
      }
      const key = this.key();
      this.skip();
      if (this.text[this.i] !== ":") throw new VideopsalmParseError(`expected ":" after key ${JSON.stringify(key)}`, this.i);
      this.i++;
      out[key] = this.value();
      this.skip();
      const c = this.text[this.i];
      if (c === ",") {
        this.i++;
        continue;
      }
      if (c === "}") {
        this.i++;
        break;
      }
      throw new VideopsalmParseError(`expected "," or "}" after ${JSON.stringify(key)}`, this.i);
    }
    return out;
  }

  private array(): VpJson[] {
    this.i++; // [
    const out: VpJson[] = [];
    this.skip();
    if (this.text[this.i] === "]") {
      this.i++;
      return out;
    }
    for (;;) {
      out.push(this.value());
      this.skip();
      const c = this.text[this.i];
      if (c === ",") {
        this.i++;
        this.skip();
        if (this.text[this.i] === "]") {
          this.i++;
          break;
        }
        continue;
      }
      if (c === "]") {
        this.i++;
        break;
      }
      throw new VideopsalmParseError("expected \",\" or \"]\" after array item", this.i);
    }
    return out;
  }

  private key(): string {
    this.skip();
    if (this.text[this.i] === '"') return this.string();
    const start = this.i;
    while (this.i < this.text.length) {
      const c = this.text[this.i];
      if (c === ":" || c === "}" || c === ",") break;
      this.i++;
    }
    const key = this.text.slice(start, this.i).trim();
    if (!key) throw new VideopsalmParseError("empty object key", start);
    return key;
  }

  private bare(): VpJson {
    const start = this.i;
    while (this.i < this.text.length) {
      const c = this.text[this.i];
      if (c === "," || c === "}" || c === "]") break;
      this.i++;
    }
    const raw = this.text.slice(start, this.i).trim();
    if (!raw) return null;
    if (raw === "true") return true;
    if (raw === "false") return false;
    if (raw === "null") return null;
    if (/^-?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?$/.test(raw)) return Number(raw);
    return raw;
  }

  private string(): string {
    this.i++; // opening quote
    let out = "";
    while (this.i < this.text.length) {
      const c = this.text[this.i++];
      if (c === '"') return out;
      if (c === "\\") {
        if (this.i >= this.text.length) break;
        const e = this.text[this.i++];
        switch (e) {
          case '"': out += '"'; break;
          case "\\": out += "\\"; break;
          case "/": out += "/"; break;
          case "b": out += "\b"; break;
          case "f": out += "\f"; break;
          case "n": out += "\n"; break;
          case "r": out += "\n"; break; // normalise CR to LF
          case "t": out += "\t"; break;
          case "u": {
            const hex = this.text.slice(this.i, this.i + 4);
            if (!/^[0-9a-fA-F]{4}$/.test(hex)) throw new VideopsalmParseError("invalid \\u escape", this.i);
            out += String.fromCharCode(parseInt(hex, 16));
            this.i += 4;
            break;
          }
          case "\n": break; // escaped physical newline: line continuation
          case "\r":
            if (this.text[this.i] === "\n") this.i++;
            break;
          default:
            // VideoPsalm emits unknown escapes verbatim. Preserve the
            // character rather than turning a song into a parse failure.
            out += e;
        }
      } else if (c === "\r") {
        if (this.text[this.i] === "\n") this.i++;
        out += "\n";
      } else {
        // Literal newlines and tabs are legal in the native dialect.
        out += c;
      }
    }
    throw new VideopsalmParseError("unterminated string", this.i);
  }
}

/** Parse a native VideoPsalm JSON document (BOM-safe). */
export function parseVideopsalm(text: string): VpJson {
  const body = text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
  return new Parser(body).parse();
}

function escapedString(value: string): string {
  let out = '"';
  for (const ch of value) {
    if (ch === "\\") out += "\\\\";
    else if (ch === '"') out += '\\"';
    else if (ch === "\n") out += "\n"; // literal, exactly as VideoPsalm writes it
    else if (ch === "\t") out += "\t";
    else if (ch === "\r") out += "\\r";
    else if (ch.charCodeAt(0) < 32) out += `\\u${ch.charCodeAt(0).toString(16).padStart(4, "0")}`;
    else out += ch;
  }
  return out + '"';
}

function escapedKey(key: string): string {
  return /^[A-Za-z_][A-Za-z0-9_.-]*$/.test(key) ? key : escapedString(key);
}

/** Serialise the AST back to VideoPsalm's compact, unquoted-key dialect. */
export function stringifyVideopsalm(value: VpJson): string {
  const write = (v: VpJson): string => {
    if (v === null) return "null";
    if (typeof v === "string") return escapedString(v);
    if (typeof v === "number") return Number.isFinite(v) ? String(v) : "0";
    if (typeof v === "boolean") return v ? "true" : "false";
    if (Array.isArray(v)) return `[${v.map(write).join(",")}]`;
    return `{${Object.entries(v).map(([k, x]) => `${escapedKey(k)}:${write(x)}`).join(",")}}`;
  };
  return write(value);
}

// ---------------------------------------------------------------------------
// Native -> shared Song
// ---------------------------------------------------------------------------

function obj(v: VpJson | undefined): VpObject | null {
  return v !== null && typeof v === "object" && !Array.isArray(v) ? v : null;
}

function text(v: VpJson | undefined): string {
  if (typeof v === "string") return v;
  if (typeof v === "number" || typeof v === "boolean") return String(v);
  return "";
}

/** Native `[DefaultValue(-1)]` order index; empty/negative is \"document order\". */
function optionalInt(v: VpJson | undefined): number | undefined {
  if (typeof v !== "number" || !Number.isInteger(v) || v < 0) return undefined;
  return v;
}

function splitAuthors(value: string): string[] {
  return value
    .split(/[;,]/)
    .map((x) => x.trim())
    .filter(Boolean);
}

function splitLines(value: string): string[] {
  const normal = value.replace(/\r\n?/g, "\n");
  const trimmed = normal.replace(/^\n/, "").replace(/\n$/, "");
  return trimmed.split("\n");
}

/** VideoPsalm `SongVerseTag` enum order, verbatim from the 1.29 metadata. */
const VERSE_TAGS = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"] as const;

function tagLetter(value: VpJson | undefined): string {
  const n = Number(text(value));
  return Number.isInteger(n) && n >= 0 && n < VERSE_TAGS.length ? VERSE_TAGS[n] : "V";
}

function tagNumber(letter: string | undefined): number {
  const n = VERSE_TAGS.indexOf((letter ?? "V").toUpperCase() as (typeof VERSE_TAGS)[number]);
  return n >= 0 ? n : 9;
}

function nativeVerseTagged(vo: VpObject | null): { tag: string; tagId: number } {
  if (!vo) return { tag: "V", tagId: 1 };
  const hasTag = vo.Tag !== undefined;
  const hasId = vo.ID !== undefined;
  const id = hasId ? Number(text(vo.ID)) : 1;
  return {
    tag: hasTag ? tagLetter(vo.Tag) : "V",
    tagId: Number.isFinite(id) ? id : 1,
  };
}

function stableHash(value: string): string {
  return createHash("sha1").update(value).digest("hex").slice(0, 12);
}

function bookGuidFor(root: VpObject, file: string): string {
  return text(root.Guid) || `file-${stableHash(path.resolve(file))}`;
}

function songSource(file: string, bookGuid: string, bookName: string, raw: VpObject): SongSource {
  return {
    format: "videopsalm",
    file: path.resolve(file),
    songbookGuid: bookGuid,
    songbookName: bookName,
    songId: text(raw.ID) || undefined,
    guid: text(raw.Guid) || undefined,
  };
}

/** Map one native song object onto the shared song shape. */
export function songFromNative(rawValue: VpJson, file: string, bookGuid: string, bookName: string, index: number): Song | null {
  const raw = obj(rawValue);
  if (!raw) return null;
  const nativeId = text(raw.Guid) || text(raw.ID) || String(index + 1);
  const now = new Date().toISOString();
  const versesRaw = Array.isArray(raw.Verses) ? raw.Verses : [];
  const verses = versesRaw.map((v, i) => {
    if (typeof v === "string") {
      return { id: String(i + 1), lines: splitLines(v), order: i + 1, tag: "V", tagId: i + 1 };
    }
    const vo = obj(v);
    const tagged = nativeVerseTagged(vo);
    return {
      id: text(vo?.ID) || String(i + 1),
      lines: vo ? splitLines(text(vo.Text)) : [],
      order: i + 1,
      tag: tagged.tag,
      tagId: tagged.tagId,
    };
  });
  const themes = splitLines(text(raw.Theme)).map((x) => x.trim()).filter(Boolean);
  const references = splitLines(text(raw.Reference)).map((x) => x.trim()).filter(Boolean);
  const authors = splitAuthors(text(raw.Author));
  return {
    id: `videopsalm:${bookGuid}:${nativeId}`,
    title: text(raw.Text) || "Untitled",
    verses,
    authors,
    composer: text(raw.Composer) || undefined,
    ccli: text(raw.CCLI) || undefined,
    songId: text(raw.ID) || undefined,
    songbookId: bookName,
    color: text(raw.DefaultColorString) || undefined,
    icon: text(raw.Icon) || undefined,
    sequence: text(raw.Sequence) || undefined,
    verseOrderIndex: optionalInt(raw.VerseOrderIndex),
    copyright: text(raw.Copyright).replace(/\n/g, " ").trim() || undefined,
    themes: themes.length ? themes : undefined,
    references: references.length ? references : undefined,
    memo1: text(raw.Memo1) || undefined,
    memo2: text(raw.Memo2) || undefined,
    memo3: text(raw.Memo3) || undefined,
    createdAt: now,
    updatedAt: now,
    source: songSource(file, bookGuid, bookName, raw),
  };
}

/** Map a parsed songbook AST to shared songs. */
export function videopsalmToSongs(rootValue: VpJson, file: string): VideopsalmImport {
  const root = obj(rootValue);
  if (!root) throw new Error("VideoPsalm songbook root is not an object");
  const rawSongs = root.Songs;
  if (!Array.isArray(rawSongs)) throw new Error("VideoPsalm songbook has no Songs array");
  const guid = bookGuidFor(root, file);
  const name = text(root.Text) || path.basename(file).replace(/\.(?:json|vpc|vpcsetting)$/i, "");
  const abbrev = text(root.Abbreviation) || name;
  const songs = rawSongs
    .map((raw, i) => songFromNative(raw, file, guid, abbrev, i))
    .filter((s): s is Song => s !== null);
  return {
    songbook: { file: path.resolve(file), name, abbrev, guid, songCount: songs.length },
    songs,
  };
}

function isVpcFile(file: string): boolean {
  return /\.(?:vpc|vpcsetting)$/i.test(file);
}

/** Native JSON lives either directly in the file or inside a `.vpc` archive. */
function readNativeSongbookText(file: string): string {
  return isVpcFile(file)
    ? readVpcEntry(file).data.toString("utf8")
    : fs.readFileSync(file, "utf8");
}

/** Read + map one native songbook file (`.json` or encrypted `.vpc`). */
export function readVideopsalmSongbook(file: string): VideopsalmImport {
  return videopsalmToSongs(parseVideopsalm(readNativeSongbookText(file)), file);
}

// ---------------------------------------------------------------------------
// Shared Song -> native AST, write-through
// ---------------------------------------------------------------------------

function findNativeSong(root: VpObject, song: Song): VpObject | null {
  const rawSongs = Array.isArray(root.Songs) ? root.Songs : null;
  if (!rawSongs) return null;
  const source = song.source;
  const candidates = rawSongs.map(obj).filter((x): x is VpObject => x !== null);
  if (source?.guid) {
    const hit = candidates.find((s) => text(s.Guid) === source.guid);
    if (hit) return hit;
  }
  if (source?.songId) {
    const hit = candidates.find((s) => text(s.ID) === source.songId);
    if (hit) return hit;
  }
  return candidates.find((s) => text(s.Text) === song.title) ?? null;
}

function nextVerseId(verses: VpJson[]): number {
  let max = 0;
  for (const v of verses) {
    const id = Number(text(obj(v)?.ID));
    if (Number.isFinite(id) && id > max) max = id;
  }
  return max + 1;
}

function versionStamp(): string {
  const d = new Date();
  const p = (n: number): string => String(n).padStart(2, "0");
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`;
}

/**
 * Apply a shared song's editable fields to a parsed songbook AST. Unknown
 * fields, formatting, and stale metadata stay untouched.
 */
export function applySongToVideopsalm(rootValue: VpJson, song: Song, edits?: string[]): { song: VpObject } {
  const root = obj(rootValue);
  if (!root) throw new Error("VideoPsalm songbook root is not an object");
  const raw = findNativeSong(root, song);
  if (!raw) throw new Error(`Song ${JSON.stringify(song.title)} was not found in the VideoPsalm songbook`);

  if (song.title.trim()) raw.Text = song.title;

  // Metadata. A native value is never overwritten unless the operator edited
  // that field: the editor used to own only lyrics + title, and silently
  // rewriting the operator's author string would be data loss. An edited
  // field is written from the editor (an emptied field is removed).
  const edited = new Set(edits ?? []);
  const writeMeta = (key: string, value: string, field: string): void => {
    if (edited.has(field)) {
      if (value) raw[key] = value;
      else delete raw[key];
      return;
    }
    if (!text(raw[key]) && value) raw[key] = value;
  };
  writeMeta("Author", (song.authors ?? []).join("; "), "author");
  writeMeta("Composer", song.composer ?? "", "composer");
  writeMeta("CCLI", song.ccli ?? "", "ccli");
  writeMeta("Copyright", song.copyright ?? "", "copyright");
  writeMeta("Theme", (song.themes ?? []).join("\n"), "theme");
  writeMeta("Reference", (song.references ?? []).join("\n"), "reference");
  writeMeta("Memo1", song.memo1 ?? "", "memo1");
  writeMeta("Memo2", song.memo2 ?? "", "memo2");
  writeMeta("Memo3", song.memo3 ?? "", "memo3");

  // The verse order: `Sequence` holds the alternatives (comma separated) and
  // `VerseOrderIndex` picks one. VideoPsalm's default is -1 ("document
  // order"), and the serializer omits the default — so "document order" is
  // written by removing the field.
  if (edited.has("sequence")) {
    if (song.sequence) raw.Sequence = song.sequence;
    else delete raw.Sequence;
  }
  if (edited.has("verseOrderIndex")) {
    const index = song.verseOrderIndex;
    if (typeof index === "number" && index >= 0) raw.VerseOrderIndex = index;
    else delete raw.VerseOrderIndex;
  }

  const existing = Array.isArray(raw.Verses) ? raw.Verses : [];
  const usedIds = new Set<number>();
  for (const v of existing) {
    const n = Number(text(obj(v)?.ID));
    if (Number.isInteger(n) && n > 0) usedIds.add(n);
  }
  let nextId = nextVerseId(existing);
  // Native verse 1 is often written without an explicit `ID` (the serializer
  // omits the default), so count the shared tag ids as reserved too.
  for (const block of song.verses) {
    if ((block.tag ?? "V").toUpperCase() === "V" && block.tagId && block.tagId >= nextId) {
      nextId = block.tagId + 1;
    }
  }
  for (let i = 0; i < song.verses.length; i++) {
    const block = song.verses[i];
    // VideoPsalm.SongVerse.Normalize: a blank line is an in-verse slide
    // break, stored as a NBSP-only line. Preserve that invariant on write.
    const body = block.lines.join("\n").replace(/\r\n?/g, "\n").replace(/\n\n/g, "\n\u00a0\n");
    const tag = (block.tag ?? "V").toUpperCase();
    const tagNum = tagNumber(tag);
    let verse = obj(existing[i]);
    if (!verse) {
      verse = {};
      existing[i] = verse;
    }

    // The native `Tag` is an enum number, and `ID` is separate from the
    // lyrics text. V1 / C / B are editor labels, never literal lyric text.
    if (tagNum === 0) delete verse.Tag;
    else verse.Tag = tagNum;

    const nativeId = Number(text(verse.ID));
    const hasNativeId = Number.isInteger(nativeId) && nativeId > 0;
    const blockId = block.tagId !== undefined && Number.isInteger(block.tagId) && block.tagId >= 0
      ? block.tagId
      : (tag === "V" && Number.isInteger(Number(block.id)) && Number(block.id) > 0 ? Number(block.id) : 0);
    let id = blockId;
    if (id > 0 && usedIds.has(id) && id !== nativeId) {
      id = hasNativeId ? nativeId : nextId++;
    }
    if (id === 0 && tag === "V" && !hasNativeId) id = nextId++;
    if (id > 0) usedIds.add(id);
    if (id === 1) delete verse.ID; // serializer's default
    else verse.ID = id;
    verse.Text = body;
  }
  if (existing.length > song.verses.length) existing.splice(song.verses.length);
  raw.Verses = existing;
  raw.VersionDate = versionStamp();
  root.VersionDate = versionStamp();

  // Keep the persistent source locator in sync with the native identity we
  // actually matched (new songs created by the editor may only have a title).
  song.source = {
    format: "videopsalm",
    file: song.source?.file ?? "",
    songbookGuid: song.source?.songbookGuid ?? "",
    songbookName: song.source?.songbookName ?? text(root.Text),
    songId: text(raw.ID) || song.source?.songId,
    guid: text(raw.Guid) || song.source?.guid,
  };
  return { song: raw };
}

/**
 * Read a native songbook, apply one song, and write it back atomically.
 * Mirrors VideoPsalm's own writer: UTF-8 BOM, a `.bak` copy of the previous
 * file, and a same-directory temp file renamed into place so a crash cannot
 * leave a half-written songbook.
 */
export function saveSongToVideopsalm(file: string, song: Song, edits?: string[]): { file: string; text: string } {
  const rawText = readNativeSongbookText(file);
  const root = parseVideopsalm(rawText);
  applySongToVideopsalm(root, song, edits);
  const serialized = stringifyVideopsalm(root);
  let out: Buffer | string;
  if (isVpcFile(file)) {
    // Native VideoPsalm writes the compressed payload without a BOM, in one
    // entry named after the songbook (`Smoke.vpc` → `Smoke.json`). Preserve
    // an unprotected archive's mode, but otherwise use AES-256.
    const previous = readVpcEntry(file);
    const entryName = previous.name || `${path.basename(file).replace(/\.(?:vpc|vpcsetting)$/i, "")}.json`;
    out = buildVpcEntry(entryName, Buffer.from(serialized, "utf8"), { encrypt: previous.encrypted });
  } else {
    // VideoPsalm's plain writer always emits the UTF-8 preamble, whether or
    // not the source file had one.
    out = "\uFEFF" + serialized;
  }
  const dir = path.dirname(file);
  const tmp = path.join(dir, `.VPSongBook-${process.pid}-${Date.now().toString(36)}.tmp`);
  try {
    try {
      fs.copyFileSync(file, `${file}.bak`);
    } catch {
      // A read-only directory can still fail at rename; let that error surface.
    }
    fs.writeFileSync(tmp, out);
    fs.renameSync(tmp, file);
  } catch (e) {
    try {
      fs.rmSync(tmp, { force: true });
    } catch {
      // Best-effort temp cleanup; the original write error is more useful.
    }
    throw e;
  }
  return { file, text: serialized };
}

// ---------------------------------------------------------------------------
// Document folder: Image/Video galleries + style background discovery
// ---------------------------------------------------------------------------

// Exact extension sets from VideoPsalm 1.29 Images.cs / Videos.cs.
const IMAGE_EXT = /\.(?:jpe?g|jpe|jfif|png|bmp|tiff?|tga|gif)$/i;
const VIDEO_EXT = /\.(?:asf|avi|divx|dv|flv|gxf|iso|m1v|m2v|m2t|m2ts|m4v|mkv|mov|mp2|mp4|mpe?g|mpeg[1-4]|mpg|mts|mxf|nuv|ogg|ogm|ogv|ogx|ps|rec|rm|rmvb|tod|ts|vob|wmv|webm)$/i;

function kindOfMedia(file: string): "image" | "video" | null {
  if (IMAGE_EXT.test(file)) return "image";
  if (VIDEO_EXT.test(file)) return "video";
  return null;
}

function walkFiles(dir: string, depth = 0, maxDepth = 4): string[] {
  if (depth > maxDepth) return [];
  let entries: fs.Dirent[] = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  const out: string[] = [];
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walkFiles(full, depth + 1, maxDepth));
    else if (entry.isFile()) out.push(full);
  }
  return out;
}

function resolveMediaPath(value: string, folder: string): string | null {
  if (!value.trim()) return null;
  const normal = value.replace(/\\/g, path.sep).replace(/^\.?[\\/]+/, "");
  const candidates = path.isAbsolute(normal)
    ? [normal]
    : [
        path.resolve(folder, normal),
        path.resolve(folder, "Images", normal),
        path.resolve(folder, "Videos", normal),
        path.resolve(folder, "Images", path.basename(normal)),
        path.resolve(folder, "Videos", path.basename(normal)),
        path.resolve(folder, "Styles", normal),
        path.resolve(folder, "..", normal),
      ];
  for (const candidate of candidates) {
    try {
      if (fs.existsSync(candidate) && fs.statSync(candidate).isFile()) return candidate;
    } catch {
      // One unreadable candidate must not stop the scan.
    }
  }
  return null;
}

function collectMediaStrings(value: VpJson, folder: string, source: string, out: VideopsalmBackground[]): void {
  if (typeof value === "string") {
    if (kindOfMedia(value)) {
      const file = resolveMediaPath(value, folder);
      if (file) out.push({ kind: kindOfMedia(file) === "video" ? "videoLoop" : "image", file, source });
    }
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) collectMediaStrings(item, folder, source, out);
    return;
  }
  if (value && typeof value === "object") {
    for (const child of Object.values(value)) collectMediaStrings(child, folder, source, out);
  }
}

function parseStyleFile(file: string): VpJson | null {
  try {
    return parseVideopsalm(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

/** Read every `*Style*.json` at the document-folder root (and `Styles/`). */
function styleFiles(folder: string): string[] {
  const roots: string[] = [];
  try {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      if (!entry.isFile() || !/\.json$/i.test(entry.name)) continue;
      if (/style/i.test(entry.name)) roots.push(path.join(folder, entry.name));
    }
  } catch {
    // Missing document folder: caller reports that separately.
  }
  const stylesDir = path.join(folder, "Styles");
  for (const file of walkFiles(stylesDir, 0, 1)) {
    if (/\.json$/i.test(file) && /style/i.test(path.basename(file))) roots.push(file);
  }
  return [...new Set(roots)];
}

/** Native folder is `SongBooks` (capital B), per VideoPsalm.SongBooks.Folder. */
function songbookFiles(folder: string): string[] {
  const books = path.join(folder, "SongBooks");
  // A caller may point at the SongBooks folder itself rather than the
  // document root. Accept both without double-walking the whole document
  // folder (which would pick up settings and style JSON as songs).
  const root = fs.existsSync(books)
    ? books
    : /^songbooks?$/i.test(path.basename(folder))
      ? folder
      : folder;
  return walkFiles(root, 0, 3).filter((file) => {
    const base = path.basename(file);
    if (!/\.(?:json|vpc)$/i.test(base)) return false;
    // When the caller selected a loose folder, root-level settings/styles
    // are not songbooks. Native songs always live under SongBooks/.
    if (/^(?:settings|commonsettings|(?:.*style))\.json$/i.test(base)) return false;
    return true;
  });
}

/**
 * VideoPsalm stores an optional custom data folder in `CommonSettings.json`
 * (`Utils.DataBaseFolder`). That file lives in the default folder even when
 * the data itself was moved, so resolve it before walking.
 */
export function resolveVideopsalmDataFolder(folder: string): string {
  const base = path.resolve(folder);
  try {
    const settings = path.join(base, "CommonSettings.json");
    if (!fs.existsSync(settings)) return base;
    const root = obj(parseVideopsalm(fs.readFileSync(settings, "utf8")));
    const configured = text(root?.DataBaseFolder).trim();
    if (!configured) return base;
    const candidate = path.isAbsolute(configured)
      ? configured
      : path.resolve(base, configured);
    return fs.existsSync(candidate) && fs.statSync(candidate).isDirectory() ? candidate : base;
  } catch {
    // A malformed CommonSettings must not stop the folder scan; fall back to
    // the folder the operator picked.
    return base;
  }
}

/** Scan a VideoPsalm document folder for songbooks, media, and style backgrounds. */
export function scanVideopsalmFolder(folder: string): VideopsalmFolderScan {
  const resolvedFolder = resolveVideopsalmDataFolder(folder);
  const result: VideopsalmFolderScan = {
    folder: resolvedFolder,
    songs: [],
    songbooks: [],
    images: [],
    videos: [],
    backgrounds: [],
    errors: [],
  };
  if (!fs.existsSync(resolvedFolder) || !fs.statSync(resolvedFolder).isDirectory()) {
    result.errors.push(`Document folder not found: ${resolvedFolder}`);
    return result;
  }

  for (const file of songbookFiles(resolvedFolder)) {
    try {
      const imported = readVideopsalmSongbook(file);
      result.songs.push(...imported.songs);
      result.songbooks.push(imported.songbook);
    } catch (e) {
      result.errors.push(`${path.basename(file)}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  for (const dirName of ["Images", "Videos", path.join("SongBooks", "Images"), path.join("SongBooks", "Videos")]) {
    for (const file of walkFiles(path.join(resolvedFolder, dirName), 0, 3)) {
      // VideoPsalm's generated thumbnails live in `Thumbs/` and are not
      // gallery entries (Images.cs / Videos.cs both exclude them).
      if (file.split(path.sep).includes("Thumbs")) continue;
      const kind = kindOfMedia(file);
      if (kind === "image" && !result.images.includes(file)) result.images.push(file);
      if (kind === "video" && !result.videos.includes(file)) result.videos.push(file);
    }
  }

  for (const file of styleFiles(resolvedFolder)) {
    const root = parseStyleFile(file);
    if (!root) {
      result.errors.push(`${path.basename(file)}: not a readable style file`);
      continue;
    }
    const found: VideopsalmBackground[] = [];
    collectMediaStrings(root, resolvedFolder, path.basename(file), found);
    for (const bg of found) {
      const key = `${bg.kind}:${bg.file}`;
      if (!result.backgrounds.some((x) => `${x.kind}:${x.file}` === key)) result.backgrounds.push(bg);
      if (bg.kind === "videoLoop" && !result.videos.includes(bg.file)) result.videos.push(bg.file);
      if (bg.kind === "image" && !result.images.includes(bg.file)) result.images.push(bg.file);
    }
  }

  // Stable gallery order, matching the original's filename sort.
  result.images.sort((a, b) => path.basename(a).localeCompare(path.basename(b), undefined, { sensitivity: "base" }));
  result.videos.sort((a, b) => path.basename(a).localeCompare(path.basename(b), undefined, { sensitivity: "base" }));
  return result;
}

/** Candidate document folders, most-specific first. */
export function videopsalmFolderCandidates(home: string, documents: string): string[] {
  return [
    process.env["VIDEOPSALM_FOLDER"] ?? "",
    process.env["PUBLIC"] ? path.join(process.env["PUBLIC"], "Documents", "VideoPsalm") : "",
    path.join(documents, "VideoPsalm"),
    path.join(home, "Documents", "VideoPsalm"),
    path.join(home, "VideoPsalm"),
  ].filter(Boolean);
}
