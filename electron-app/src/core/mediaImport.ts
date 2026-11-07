// Media import: pick graphics in the OS file explorer and file them
// straight into the VideoPsalm document folder (Images/ or Videos/), the
// way the original works — select, import, and the file lives in your
// VideoPsalm folder from then on.
//
// The dialog itself lives in main (`vp:import-media`); this module holds the
// pure/filesystem half so it is unit-testable without Electron.
import * as fs from "fs";
import * as path from "path";
import { kindOfMedia } from "./videopsalm";

export type MediaKind = "image" | "video";

export { kindOfMedia };

/** Basename scrubbed for the library folder (no separators, no empties). */
export function safeMediaName(name: string): string {
  const base = path.basename(name.replace(/\\/g, "/"));
  const clean = base.replace(/[<>:"|?*\u0000-\u001F]/g, "_").trim();
  return clean || "media";
}

/** First free name in `dir`: `pic.jpg`, `pic (2).jpg`, … — never overwrite. */
export function freeMediaName(dir: string, name: string): string {
  const safe = safeMediaName(name);
  const ext = path.extname(safe);
  const stem = path.basename(safe, ext);
  let candidate = safe;
  for (let n = 2; fs.existsSync(path.join(dir, candidate)); n++) {
    candidate = `${stem} (${n})${ext}`;
  }
  return candidate;
}

/** Destination folder for an import: Images/ or Videos/ under the document
 *  folder, created on demand. Unknown kinds are rejected, never guessed. */
export function mediaImportDir(docFolder: string, kind: MediaKind): string {
  const dir = path.join(docFolder, kind === "video" ? "Videos" : "Images");
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * Copy picked files into the document folder's media library.
 * Files of unknown kind are reported, not imported; existing files are
 * never overwritten (see `freeMediaName`).
 */
export function copyMediaIntoFolder(
  srcPaths: string[],
  docFolder: string
): { copied: { path: string; kind: MediaKind }[]; errors: string[] } {
  const copied: { path: string; kind: MediaKind }[] = [];
  const errors: string[] = [];
  for (const src of srcPaths) {
    const kind = kindOfMedia(src);
    if (!kind) {
      errors.push(`Not a media file: ${src}`);
      continue;
    }
    let dir: string;
    try {
      dir = mediaImportDir(docFolder, kind);
    } catch (e) {
      errors.push(`Cannot use ${docFolder}: ${e instanceof Error ? e.message : String(e)}`);
      continue;
    }
    const target = path.join(dir, freeMediaName(dir, src));
    try {
      fs.copyFileSync(src, target);
      copied.push({ path: target, kind });
    } catch (e) {
      errors.push(`Could not import ${src}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  return { copied, errors };
}
