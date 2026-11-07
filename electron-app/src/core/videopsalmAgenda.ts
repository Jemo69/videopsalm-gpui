// Native VideoPsalm service plan (`.vpagd`) import.
//
// VideoPsalm saves a service plan as a ZIP (`Agenda.SaveToFile`): one
// projection per agenda item, named after the projection type, plus the
// style files and the media blobs:
//
//   Song_0.json         the song projection (same shape as a songbook song)
//   SongBook_0.json     a clone of the songbook the song came from
//   BibleVerses_0.json  the verse selection
//   BibleBook_0.json    its book      BibleChapter_0.json  its chapter
//   Bible_0.json        its translation
//   Image_0.json        FileName      Video_0.json   FileName
//   Audio_0.json        FileName      WebSite_0.json Url
//   RootStyle.json, SongBookStyle.json, …   the styles
//   <media files>       the blobs (when "store documents in the agenda file")
//
// The ZIP entry order *is* the agenda order: the writer walks the agenda
// items in order and appends their entries, and the reader inserts each
// loaded item at the next position.
//
// This module maps that file onto the shared `Agenda` schema. The songs that
// come with the plan are mapped with the same rules as a songbook, so their
// ids match the operator's own songbook and the two do not duplicate.

import * as fs from "fs";
import * as path from "path";
import { createHash } from "crypto";
import { parseVideopsalm, songFromNative, stringifyVideopsalm, type VpJson, type VpObject } from "./videopsalm";
import { buildZipEntries, readVpcEntries } from "./vpc";
import type { Agenda, AgendaItem, BibleRef, Song } from "./schemas";

export interface VideopsalmAgendaImport {
  /** Agenda title (the file name, as VideoPsalm shows it). */
  title: string;
  items: AgendaItem[];
  /** Songs carried by the plan that the library may not have yet. */
  songs: Song[];
  /** Media files extracted next to the plan. */
  media: string[];
  /** Things the operator should know about (missing files, odd items). */
  warnings: string[];
}

/** The projection kinds the plan can carry, and our agenda kind for each. */
const ITEM_KINDS: Record<string, AgendaItem["kind"]> = {
  Song: "Song",
  BibleVerses: "BibleVerses",
  Image: "Image",
  Video: "Video",
  Audio: "Audio",
  Pdf: "Pdf",
  PowerPoint: "PowerPoint",
  Word: "Word",
  Excel: "Excel",
  WebSite: "WebSite",
};

/** File-backed kinds resolve their media file from the projection. */
const FILE_KINDS = new Set(["Image", "Video", "Audio", "Pdf", "PowerPoint", "Word", "Excel"]);

function text(value: VpJson | undefined): string {
  if (!value) return "";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return "";
}

function member(root: VpJson, key: string): VpJson | undefined {
  return root && typeof root === "object" && !Array.isArray(root)
    ? (root as Record<string, VpJson>)[key]
    : undefined;
}

function memberText(root: VpJson, key: string): string {
  return text(member(root, key));
}

function safeName(name: string): string {
  return name.replace(/[\\/:*?"<>|]/g, "_").trim() || "file";
}

/**
 * Read a native VideoPsalm service plan.
 *
 * `extractMediaTo` is the folder the plan's media blobs are written to; it
 * defaults to a folder next to the plan file, so the imported items point at
 * real files.
 */
export function readVideopsalmAgenda(
  file: string,
  options: { extractMediaTo?: string } = {}
): VideopsalmAgendaImport {
  const entries = readVpcEntries(file);
  const byName = new Map(entries.map((e) => [e.name, e.data]));
  const order = entries.map((e) => e.name);
  const mediaDir = options.extractMediaTo ?? path.join(path.dirname(file), `${safeName(path.basename(file, path.extname(file)))}-media`);
  const warnings: string[] = [];
  const media: string[] = [];
  const songs: Song[] = [];
  const items: AgendaItem[] = [];

  /** The blob for a referenced file, if the plan carries it. */
  const blobFor = (fileName: string): Buffer | undefined => {
    if (!fileName) return undefined;
    const base = path.basename(fileName.replace(/\\/g, "/"));
    return byName.get(fileName) ?? byName.get(base);
  };

  const extractMedia = (fileName: string): string | undefined => {
    const blob = blobFor(fileName);
    if (!blob) return undefined;
    const target = path.join(mediaDir, safeName(path.basename(fileName.replace(/\\/g, "/"))));
    try {
      fs.mkdirSync(mediaDir, { recursive: true });
      fs.writeFileSync(target, blob);
      media.push(target);
      return target;
    } catch (e) {
      warnings.push(`Could not extract ${fileName}: ${e instanceof Error ? e.message : String(e)}`);
      return undefined;
    }
  };

  let counter = 0;
  for (const name of order) {
    const match = /^([A-Za-z]+)_(\d+)\.json$/.exec(name);
    if (!match) continue;
    const kind = match[1];
    if (!ITEM_KINDS[kind]) continue; // SongBook_/BibleBook_/… are context, not items
    const index = match[2];
    let projection: VpJson;
    try {
      projection = parseVideopsalm(byName.get(name)!.toString("utf8"));
    } catch (e) {
      warnings.push(`Could not read ${name}: ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }
    const item = mapItem({
      kind,
      index,
      projection,
      byName,
      extractMedia,
      warnings,
      songs,
    });
    if (item) {
      counter += 1;
      items.push({ id: `vpagd-${counter}-${index}`, ...item });
    }
  }

  return {
    title: path.basename(file),
    items,
    songs,
    media,
    warnings,
  };
}

interface MapContext {
  kind: string;
  index: string;
  projection: VpJson;
  byName: Map<string, Buffer>;
  extractMedia: (fileName: string) => string | undefined;
  warnings: string[];
  songs: Song[];
}

function mapItem(ctx: MapContext): Omit<AgendaItem, "id"> | null {
  const { kind, index, projection, byName, warnings, songs } = ctx;
  const title = memberText(projection, "Text");

  if (kind === "Song") {
    const bookEntry = byName.get(`SongBook_${index}.json`);
    const book = bookEntry ? parseVideopsalm(bookEntry.toString("utf8")) : null;
    const bookGuid = book ? memberText(book, "Guid") : "";
    const bookName = book ? memberText(book, "Text") || memberText(book, "Abbreviation") : "";
    const song = songFromNative(projection, "", bookGuid, bookName, Number(index) || 0);
    if (!song) return null;
    // The plan carries a *snapshot* of the song, not the operator's songbook
    // file: it must not become a write-back target. When the real songbook is
    // loaded later, the library upserts the same id and the native source
    // locator comes with it.
    const snapshot: Song = { ...song, source: undefined };
    songs.push(snapshot);
    return {
      kind: "Song",
      title: title || snapshot.title,
      ref: { songId: snapshot.id },
      flowType: "Manual",
    };
  }

  if (kind === "BibleVerses") {
    const verses = member(projection, "Verses");
    const list = Array.isArray(verses) ? verses : [];
    const bookEntry = byName.get(`BibleBook_${index}.json`);
    const chapterEntry = byName.get(`BibleChapter_${index}.json`);
    const bibleEntry = byName.get(`Bible_${index}.json`);
    const book = bookEntry ? parseVideopsalm(bookEntry.toString("utf8")) : null;
    const chapter = chapterEntry ? parseVideopsalm(chapterEntry.toString("utf8")) : null;
    const bible = bibleEntry ? parseVideopsalm(bibleEntry.toString("utf8")) : null;
    const bookName = book ? memberText(book, "Text") : "";
    const chapterNumber = chapter ? Number(memberText(chapter, "ID")) : 0;
    const translationId = bible ? memberText(bible, "Text") : "";
    const verseNumbers: number[] = [];
    const lines: string[] = [];
    for (const verse of list) {
      const number = Number(memberText(verse, "ID"));
      if (Number.isFinite(number) && number > 0) verseNumbers.push(number);
      const body = memberText(verse, "Text");
      if (body) lines.push(number > 0 ? `${number} ${body}` : body);
    }
    const ref: BibleRef = {
      translationId: translationId || "sample",
      book: bookName,
      chapter: chapterNumber,
      verses: verseNumbers,
      versification: "EN",
    };
    const first = verseNumbers[0];
    const last = verseNumbers[verseNumbers.length - 1];
    const reference = bookName && chapterNumber
      ? `${bookName} ${chapterNumber}${first ? `:${first}` : ""}${last && last !== first ? `-${last}` : ""}`
      : title || "Bible";
    if (!bookName) {
      warnings.push(`Bible item ${index}: the plan has no book information.`);
    }
    return {
      kind: "BibleVerses",
      title: reference,
      ref: { bibleRef: ref, text: lines.join("\n") },
      flowType: "Manual",
    };
  }

  if (kind === "WebSite") {
    const url = memberText(projection, "Url");
    return {
      kind: "WebSite",
      title: title || url,
      ref: { url },
      flowType: "Manual",
    };
  }

  if (FILE_KINDS.has(kind)) {
    const fileName = memberText(projection, "FileName");
    const extracted = ctx.extractMedia(fileName);
    const target = extracted ?? (fileName && path.isAbsolute(fileName) ? fileName : undefined);
    if (!target) {
      warnings.push(`${kind} item "${title || fileName}" has no media file (not in the plan, and not an absolute path).`);
    }
    return {
      kind: ITEM_KINDS[kind],
      title: title || path.basename(fileName || ""),
      ref: target ? { file: target } : undefined,
      flowType: "Manual",
    };
  }

  return null;
}

// ---------------------------------------------------------------------------
// Native service plan export (`.vpagd` writer)
//
// Inverse of the importer above. Mirrors `Agenda.SaveToFile`
// (`VideoPsalm/Agenda.cs:183`): the writer walks the agenda items in order
// and appends one projection per item, so the ZIP entry order *is* the
// agenda order; per-kind counters name the entries (`Song_0.json`, …).
// `Version.json` carries `"2"` (`SaveVersion`), and
// `AgendaItemProperties.json` carries the per-item flow/advance/order row
// (`SaveAgendaAbleProperties`). Styles (`RootStyle.json`, …) are omitted:
// the native loader tolerates missing styles (`LoadStyle` leaves the item
// style null), and this app has no native style tree to serialize — the
// operator's own styles stay untouched.
//
// Media blobs are embedded by basename (Ionic `AddFile` semantics), which is
// what `blobFor` on the import side resolves. A file item whose file is not
// on disk is still written as a projection with its `FileName`, plus a
// warning — the plan stays complete and the native side can relink it.
// ---------------------------------------------------------------------------

export interface VideopsalmAgendaEntries {
  entries: { name: string; data: Buffer }[];
  /** Media blobs embedded by basename. */
  media: string[];
  warnings: string[];
}

export interface VideopsalmAgendaWriteResult extends VideopsalmAgendaEntries {
  file: string;
}

/** `SongVerseTag` enum order, verbatim from the 1.29 metadata (`Song.cs:53`). */
const VERSE_TAGS = ["V", "C", "P", "B", "T", "E", "O", "S", "I", "R"] as const;

function tagNumber(letter: string | undefined): number {
  const n = VERSE_TAGS.indexOf((letter ?? "V").toUpperCase() as (typeof VERSE_TAGS)[number]);
  return n >= 0 ? n : 9;
}

/**
 * Shared `FlowType` → native `AgendaFlowType` int (`AgendaFlowType.cs`:
 * `Root=0, TextAndRoot=1, Next=2, TextAndNext=3, Linked=4`).
 *
 * **Inferred**: no mapping table was found in the decompiled writer (it
 * copies the live int straight through), so manual stepping maps to `Next`,
 * timed advance to `TextAndNext`, and looping to `Linked`.
 */
function nativeFlowType(flow: AgendaItem["flowType"]): number {
  const f = (flow ?? "manual").toLowerCase();
  if (f === "loop") return 4;
  if (f === "auto" || f === "autoadvance") return 3;
  return 2;
}

/** Canonical English names of the 39 Old Testament books (`vp-core` BOOKS 1–39). */
const OLD_TESTAMENT = new Set([
  "Genesis", "Exodus", "Leviticus", "Numbers", "Deuteronomy", "Joshua",
  "Judges", "Ruth", "1 Samuel", "2 Samuel", "1 Kings", "2 Kings",
  "1 Chronicles", "2 Chronicles", "Ezra", "Nehemiah", "Esther", "Job",
  "Psalm", "Proverbs", "Ecclesiastes", "Song of Solomon", "Isaiah",
  "Jeremiah", "Lamentations", "Ezekiel", "Daniel", "Hosea", "Joel",
  "Amos", "Obadiah", "Jonah", "Micah", "Nahum", "Habakkuk",
  "Zephaniah", "Haggai", "Zechariah", "Malachi",
]);

function testamentFor(book: string): string {
  return OLD_TESTAMENT.has(book) ? "Old Testament" : "New Testament";
}

function stableHash(value: string): string {
  return createHash("sha1").update(value).digest("hex").slice(0, 12);
}

interface SongIdentity {
  bookGuid: string;
  bookName: string;
  songGuid: string;
}

function songIdentity(song: Song): SongIdentity {
  const parts = song.id.split(":");
  const native = parts[0] === "videopsalm" && parts.length >= 3
    ? { bookGuid: parts[1], songGuid: parts.slice(2).join(":") }
    : null;
  const bookGuid = song.source?.songbookGuid || native?.bookGuid || `local-book-${stableHash(song.songbookId ?? song.title)}`;
  const bookName = song.source?.songbookName ?? song.songbookId ?? "SongBook";
  const songGuid = song.source?.guid || native?.songGuid || `local-${stableHash(song.id)}`;
  return { bookGuid, bookName, songGuid };
}

/** Map one shared song onto its native projection object. */
export function nativeSongFromSong(song: Song): VpObject {
  const { songGuid } = songIdentity(song);
  const native: VpObject = { Text: song.title, Guid: songGuid };
  const songNumber = song.songId !== undefined && /^\d+$/.test(song.songId) ? Number(song.songId) : NaN;
  if (Number.isInteger(songNumber)) native.ID = songNumber;
  const authors = (song.authors ?? []).join("; ");
  if (authors) native.Author = authors;
  if (song.composer) native.Composer = song.composer;
  if (song.ccli) native.CCLI = song.ccli;
  if (song.copyright) native.Copyright = song.copyright;
  if (song.themes?.length) native.Theme = song.themes.join("\n");
  if (song.references?.length) native.Reference = song.references.join("\n");
  if (song.memo1) native.Memo1 = song.memo1;
  if (song.memo2) native.Memo2 = song.memo2;
  if (song.memo3) native.Memo3 = song.memo3;
  if (song.sequence) native.Sequence = song.sequence;
  if (typeof song.verseOrderIndex === "number" && song.verseOrderIndex >= 0) {
    native.VerseOrderIndex = song.verseOrderIndex;
  }
  native.Verses = song.verses.map((block, i) => {
    const verse: VpObject = { Text: block.lines.join("\n") };
    const tag = tagNumber(block.tag);
    if (tag !== 0) verse.Tag = tag;
    // The native serializer omits the default verse id (V1 has no `ID`, as
    // in the shared fixture); every other verse keeps its tag id so the
    // importer maps it back to the same verse.
    const id = block.tagId ?? (tag === 0 ? i + 1 : 1);
    if (!(tag === 0 && id === 1)) verse.ID = id;
    return verse;
  });
  return native;
}

interface VerseBody {
  id: number;
  text: string;
}

/** Split the shared `bibleRef + text` back into native `{ID, Text}` verses. */
function verseBodies(item: AgendaItem): VerseBody[] {
  const ref = item.ref;
  const lines = (ref?.text ?? "").split("\n").map((l) => l.trimEnd()).filter((l) => l.length > 0);
  const numbers = ref?.bibleRef?.verses ?? [];
  if (numbers.length > 0) {
    return numbers.map((n, i) => {
      const line = lines.find((l) => new RegExp(`^${n}\\s+`).test(l))
        ?? lines.find((l) => l.startsWith(`${n} `))
        ?? lines[i];
      const body = line !== undefined ? line.replace(new RegExp(`^${n}\\s+`), "") : "";
      return { id: n, text: body };
    });
  }
  return lines.map((line, i) => {
    const match = /^(\d+)\s+([\s\S]*)$/.exec(line);
    return match ? { id: Number(match[1]), text: match[2] } : { id: i + 1, text: line };
  });
}

/** Find a library song for an agenda item, by `ref.songId`. */
function lookupSong(songsById: Map<string, Song> | Record<string, Song> | undefined, id: string | undefined): Song | undefined {
  if (!id) return undefined;
  return songsById instanceof Map ? songsById.get(id) : songsById?.[id];
}

function json(value: VpJson): Buffer {
  return Buffer.from(stringifyVideopsalm(value), "utf8");
}

/**
 * Build the native entry set for an agenda. Pure (no disk writes except
 * reading media blobs); `writeVideopsalmAgenda` persists the result.
 */
export function buildVideopsalmAgendaEntries(
  agenda: Agenda,
  songsById?: Map<string, Song> | Record<string, Song>
): VideopsalmAgendaEntries {
  const entries: { name: string; data: Buffer }[] = [];
  const media: string[] = [];
  const warnings: string[] = [];
  const properties: VpObject[] = [];
  const counters = { song: 0, website: 0, verses: 0, image: 0, video: 0, powerpoint: 0, pdf: 0, audio: 0, word: 0, excel: 0 };
  const embedded = new Set<string>();
  const blobEntries: { name: string; data: Buffer }[] = [];

  const embedMedia = (file: string): string => {
    const base = path.basename(file.replace(/\\/g, "/"));
    if (!embedded.has(base)) {
      try {
        const data = fs.readFileSync(file);
        // Blobs trail the projections: the native writer records file names
        // during the item walk and appends the blobs afterwards (`SaveBlobs`).
        blobEntries.push({ name: base, data });
        embedded.add(base);
        media.push(file);
      } catch {
        warnings.push(`Media file not found, linked by path only: ${file}`);
      }
    }
    // Portable entry: the importer resolves by basename, and the native
    // loader extracts blobs beside its own library — never an absolute path
    // from this machine.
    return embedded.has(base) ? base : file;
  };

  for (const item of agenda.items) {
    const song = item.kind === "Song" ? lookupSong(songsById, item.ref?.songId) : undefined;
    const verseOrderIndex = song?.verseOrderIndex ?? -1;
    properties.push({
      FlowType: nativeFlowType(item.flowType),
      AutoAdvance: item.autoAdvanceMs && item.autoAdvanceMs > 0 ? 1 : 0,
      Interval: item.autoAdvanceMs ?? 0,
      VerseOrderIndex: verseOrderIndex,
    });

    if (item.kind === "Song") {
      const n = counters.song++;
      const resolved: Song = song ?? {
        id: item.ref?.songId ?? `local:${item.id}`,
        title: item.title || "Untitled",
        verses: [{ id: "1", lines: [item.title || "Untitled"], order: 1, tag: "V", tagId: 1 }],
        authors: [],
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      if (!song) warnings.push(`Song "${item.title}" is not in the library; written as a placeholder.`);
      const { bookGuid, bookName } = songIdentity(resolved);
      entries.push({ name: `Song_${n}.json`, data: json(nativeSongFromSong(resolved)) });
      entries.push({
        name: `SongBook_${n}.json`,
        data: json({ Text: bookName, Abbreviation: bookName, Guid: bookGuid }),
      });
      continue;
    }

    if (item.kind === "BibleVerses") {
      const n = counters.verses++;
      const ref = item.ref?.bibleRef;
      const bodies = verseBodies(item);
      entries.push({
        name: `BibleVerses_${n}.json`,
        data: json({ Text: item.title, Verses: bodies.map((v) => ({ ID: v.id, Text: v.text })) }),
      });
      entries.push({ name: `BibleChapter_${n}.json`, data: json({ ID: ref?.chapter ?? 0 }) });
      entries.push({ name: `BibleBook_${n}.json`, data: json({ Text: ref?.book ?? item.title }) });
      entries.push({ name: `Testament_${n}.json`, data: json({ Text: testamentFor(ref?.book ?? "") }) });
      entries.push({ name: `Bible_${n}.json`, data: json({ Text: ref?.translationId ?? "sample" }) });
      continue;
    }

    if (item.kind === "WebSite" || item.kind === "YouTube") {
      const n = counters.website++;
      if (item.kind === "YouTube" && !item.ref?.url) {
        warnings.push(`YouTube item "${item.title}" has no URL; skipped (no native projection).`);
        continue;
      }
      entries.push({ name: `WebSite_${n}.json`, data: json({ Text: item.title, Url: item.ref?.url ?? "" }) });
      continue;
    }

    if (FILE_KINDS.has(item.kind)) {
      const file = item.ref?.file ?? "";
      const fileName = file ? embedMedia(file) : "";
      if (!file) warnings.push(`${item.kind} item "${item.title}" has no file; written with an empty FileName.`);
      const prefix = item.kind.toLowerCase() as keyof typeof counters;
      const counter = counters[prefix] ?? 0;
      counters[prefix] = counter + 1;
      entries.push({ name: `${item.kind}_${counter}.json`, data: json({ Text: item.title, FileName: fileName }) });
      continue;
    }

    warnings.push(`${item.kind} item "${item.title}" has no native projection and was skipped.`);
  }

  entries.push(...blobEntries);
  entries.push({ name: "AgendaItemProperties.json", data: json({ Items: properties }) });
  const versioned = [{ name: "Version.json", data: Buffer.from("2", "utf8") }, ...entries];
  return { entries: versioned, media, warnings };
}

/**
 * Write an agenda back to the native `.vpagd` shape (plain multi-entry ZIP).
 * Round-trips through `readVideopsalmAgenda`: entry order is agenda order.
 */
export function writeVideopsalmAgenda(
  file: string,
  agenda: Agenda,
  songsById?: Map<string, Song> | Record<string, Song>
): VideopsalmAgendaWriteResult {
  const { entries, media, warnings } = buildVideopsalmAgendaEntries(agenda, songsById);
  fs.writeFileSync(file, buildZipEntries(entries));
  return { file, entries, media, warnings };
}
