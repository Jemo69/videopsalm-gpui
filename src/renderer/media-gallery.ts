/**
 * B-32 image + video galleries + B-12-adjacent transport (Electron port).
 *
 * Clean-room reconstruction from `videopsalm-ux-research/assets/RESEARCH-media.md`
 * §§2–9, mirroring `gpui-app/vp-core/src/media.rs` + `media_gallery.rs`:
 * two filterable galleries with filename captions (§2.1, §8), the seven-row
 * control stack (§3), click-to-assign (§6.2), the leading
 * black sentinel that removes the background (§7), Luminosité as a cascading
 * style attribute (§4.2, stored in `agenda.settings.cascade`), and the
 * in/out cut range with verse/song scope (controls 11, 13/14 — §10, §11).
 *
 * Honest gaps (same as the GPUI port, stated not hidden):
 * - Assignment is global (one background for the whole session), not at the
 *   selected style level (§6.2 assigns per song/verse level — unmet).
 * - Per-file black-flash editing (§12.3: right-click menu) is unmet: the
 *   500 ms default is stored on each entry but nothing edits or consumes it.
 * - Metadata search (§8.1: image Keywords / video Title) needs a metadata
 *   pipeline — filtering matches file names only.
 * - `Rebuild thumbnails` is a labelled no-op: thumbnails ARE the files
 *   (`<img>` / `<video preload="metadata">`), so there is no cache to rebuild.
 * - `Explore media folder` names the folders the listed files live in (the
 *   path-discovering half of the vendor button); opening them in the OS file
 *   manager needs a shell bridge v1 does not have.
 * - Luminosity bounds are undocumented (§4.3), so the slider range −10…+10 is
 *   a UI-only choice; the model stores a raw number.
 * - Which physical button is verse vs song (13 vs 14) is undocumented
 *   inference (§10) — the UI labels scopes by name, never by button number.
 * - The remaining B-16 player bar (volume, mute, seek, time cycling, loop
 *   toggle, fit modes, preview hide) is out of scope here.
 *
 * Pure functions are fully tested in `tests/media-gallery.test.ts`. `mount*`
 * needs a DOM and is thin glue over the hooks.
 */
import {
  Cascade,
  Chain,
  Level,
  emptyCascade,
  getLevel,
  resolve,
  setLevel,
} from "../core/cascade.js";
import { fileUrl } from "../core/fileUrl.js";

export type GalleryKind = "image" | "video";

export interface GalleryEntry {
  id: string;
  path: string;
  kind: GalleryKind;
  /** Per-video black-flash duration, RESEARCH §12.3 (default 500 ms, S3). */
  blackFlashMs: number;
}

export const BLACK_FLASH_DEFAULT_MS = 500;
/** Leading video-gallery sentinel that *removes* the background (§7). */
export const BLACK_SENTINEL_ID = "black";

export type Transport = "stopped" | "playing" | "paused";
export type CutScope = "verse" | "song";

export interface CutRange {
  startMs: number | null;
  endMs: number | null;
  scope: CutScope;
}

export function fileCaption(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? "";
  return base || "(unnamed)";
}

/** Containing folder of a path (for the Explore-folder row), "" when none. */
export function dirOf(path: string): string {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join("/");
}

/**
 * Instant-filter by filename keywords, order-free (§8). Metadata matching
 * (§8.1) is unmet — see module docs.
 */
export function filterGallery(entries: GalleryEntry[], query: string): GalleryEntry[] {
  const words = query.toLowerCase().split(/\s+/).filter((w) => w.length > 0);
  if (!words.length) return [...entries];
  return entries.filter((e) => {
    const name = fileCaption(e.path).toLowerCase();
    return words.every((w) => name.includes(w));
  });
}

/** Background kind a gallery pick assigns (§6.2). */
export function assignedBgKind(kind: GalleryKind): "image" | "videoLoop" {
  return kind === "image" ? "image" : "videoLoop";
}

/** Media resolve through their own chains — never the song chain (B-33). */
export function galleryChain(kind: GalleryKind): Chain {
  return kind === "image" ? "images" : "videos";
}

/** Luminosity writes at the media-chain collection level (§4.2). */
export function luminosityLevel(kind: GalleryKind): Level {
  return kind === "image" ? "images" : "videos";
}

export function resolveGalleryLuminosity(
  cascade: Cascade,
  kind: GalleryKind,
): { value: number; source?: Level } {
  const p = resolve(cascade ?? emptyCascade(), galleryChain(kind)).luminosity;
  return { value: p.value, source: p.source };
}

export function setGalleryLuminosity(cascade: Cascade, kind: GalleryKind, value: number): Cascade {
  const level = luminosityLevel(kind);
  const cur = getLevel(cascade ?? emptyCascade(), level) ?? {};
  return setLevel(cascade ?? emptyCascade(), level, { ...cur, luminosity: value });
}

/**
 * CSS brightness for a resolved luminosity. UI-only mapping (the vendor
 * documents polarity, not a formula): ±1 ≈ ±8% brightness, clamped to
 * 20%…200% so the slider cannot white/black the room out by accident.
 */
export function luminosityToFilter(value: number): string {
  if (value === 0) return "";
  const b = Math.min(2.0, Math.max(0.2, 1 + value * 0.08));
  return `brightness(${b.toFixed(2)})`;
}

/** CSS filter for a background kind, or "" when none applies. */
export function brightnessFilter(cascade: Cascade, bgKind: string): string {
  if (bgKind !== "image" && bgKind !== "videoLoop") return "";
  return luminosityToFilter(resolveGalleryLuminosity(cascade, bgKind === "image" ? "image" : "video").value);
}

export function stepTransport(t: Transport, action: "play" | "pause" | "stop" | "toggle"): Transport {
  switch (action) {
    case "play": return "playing";
    case "pause": return t === "playing" ? "paused" : t;
    case "stop": return "stopped";
    case "toggle": return t === "playing" ? "paused" : "playing";
  }
}

/** Parse an "in-out" ms range (I/O keys equivalent). */
export function parseRange(text: string): { startMs: number | null; endMs: number | null } {
  const parts = text.split("-");
  const num = (s: string | undefined): number | null => {
    if (s === undefined) return null;
    const t = s.trim();
    if (!t) return null;
    const n = Number(t);
    return Number.isFinite(n) && n >= 0 ? n : null;
  };
  return { startMs: num(parts[0]), endMs: num(parts[1]) };
}

// ---------------------------------------------------------------------------
// DOM mount (thin glue; tested only via the pure functions above)
// ---------------------------------------------------------------------------

export interface GalleryHooks {
  listMedia(): GalleryEntry[];
  importFile(path: string, kind: GalleryKind): void;
  deleteEntry(id: string): void;
  /** `path === null` = black sentinel: unassign back to color/Base (§7). */
  assignBackground(kind: GalleryKind, path: string | null): void;
  assignedPath(): string | null;
  getCascade(): Cascade;
  setCascade(c: Cascade): void;
  getKeepOpen(): boolean;
  setKeepOpen(b: boolean): void;
  getCollapsed(): boolean;
  setCollapsed(b: boolean): void;
  getTransport(): Transport;
  transport(action: "play" | "pause" | "stop" | "toggle"): void;
  getRange(): CutRange;
  setRange(r: CutRange): void;
  hint(t: string): void;
}

function h(tag: string, text?: string): HTMLElement {
  const n = document.createElement(tag);
  if (text !== undefined) n.textContent = text;
  return n;
}

function thumb(entry: GalleryEntry, assigned: boolean, hooks: GalleryHooks, kind: GalleryKind): HTMLElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "gallery-thumb" + (assigned ? " assigned" : "");
  b.title = entry.path;
  if (kind === "image") {
    const img = document.createElement("img");
    img.src = fileUrl(entry.path);
    img.alt = fileCaption(entry.path);
    img.loading = "lazy";
    b.appendChild(img);
  } else {
    const v = document.createElement("video");
    v.src = fileUrl(entry.path);
    v.muted = true;
    v.preload = "metadata";
    b.appendChild(v);
  }
  const cap = h("span", (kind === "image" ? "▦ " : "▶ ") + fileCaption(entry.path));
  cap.className = "gallery-caption";
  b.appendChild(cap);
  b.addEventListener("click", () => {
    hooks.assignBackground(kind, entry.path);
    if (!hooks.getKeepOpen()) hooks.setCollapsed(true);
  });
  return b;
}

function controlRow(labelText: string, ...kids: HTMLElement[]): HTMLElement {
  const row = h("div");
  row.className = "gallery-row";
  const lab = h("span", labelText);
  lab.className = "hint";
  row.appendChild(lab);
  for (const k of kids) row.appendChild(k);
  return row;
}

function gallerySection(kind: GalleryKind, hooks: GalleryHooks): HTMLElement {
  const sec = h("section");
  sec.className = "gallery";
  const title = kind === "image" ? "Images (background = cover)" : "Videos (background loops)";
  sec.appendChild(h("h4", title));

  const search = document.createElement("input");
  search.placeholder = kind === "image" ? "Filter images…" : "Filter videos…";
  search.setAttribute("aria-label", search.placeholder);
  sec.appendChild(search);

  const grid = h("div");
  grid.className = "gallery-grid";
  sec.appendChild(grid);

  const paint = (): void => {
    grid.innerHTML = "";
    if (kind === "video") {
      // Leading black sentinel (§7): always first, never deletable.
      const s = document.createElement("button");
      s.type = "button";
      s.className = "gallery-thumb" + (hooks.assignedPath() === null ? " assigned" : "");
      s.title = "Remove the background (falls through to Base)";
      s.appendChild(h("span", "■ black (remove bg)"));
      s.addEventListener("click", () => {
        hooks.assignBackground("video", null);
        if (!hooks.getKeepOpen()) hooks.setCollapsed(true);
      });
      grid.appendChild(s);
    }
    const all = hooks.listMedia().filter((e) => e.kind === kind);
    const shown = filterGallery(all, search.value);
    if (!shown.length) {
      const empty = h("p", "(empty — galleries list user-added files only; import one below)");
      empty.className = "hint";
      grid.appendChild(empty);
    }
    for (const e of shown) {
      grid.appendChild(thumb(e, hooks.assignedPath() === e.path, hooks, kind));
    }
    const count = h("span", `${shown.length} shown`);
    count.className = "hint";
    grid.appendChild(count);
  };
  search.addEventListener("input", paint);

  // Control stack, RESEARCH §3 order: Delete · Explore · Luminosité ·
  // Keep-opened. (Import lives in the shared row below; Rebuild is a
  // labelled no-op note there — thumbnails ARE the files.)
  const del = document.createElement("button");
  del.textContent = "Delete selected";
  del.addEventListener("click", () => {
    const sel = grid.querySelector(".gallery-thumb.selected") as HTMLElement | null;
    const id = sel?.dataset["id"];
    if (!id || id === BLACK_SENTINEL_ID) {
      hooks.hint(id ? "The black sentinel is built in — it cannot be deleted." : "Delete: select a thumbnail first (click it).");
      return;
    }
    hooks.deleteEntry(id);
    paint();
  });
  sec.appendChild(controlRow("Delete:", del));

  // Explore-folder row (§3 row 3): the vendor opens the OS file manager at
  // the gallery folder (S2). v1 has no single gallery folder — entries are
  // agenda files — and no shell bridge, so this is the path-discovering half:
  // it names the distinct folders the listed files live in.
  const explore = document.createElement("button");
  explore.textContent = "Explore media folder";
  explore.addEventListener("click", () => {
    const dirs = [...new Set(hooks.listMedia().map((e) => dirOf(e.path)))].filter((d) => d.length > 0);
    hooks.hint(dirs.length
      ? `Media folders: ${dirs.join(" · ")} (v1 lists paths — no OS file-manager bridge yet)`
      : "Explore: no files yet — import one below.");
  });
  sec.appendChild(controlRow("Folder:", explore));

  const lumVal = h("strong", "+0");
  const lumSrc = h("span", "");
  lumSrc.className = "hint";
  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "-10";
  slider.max = "10";
  slider.step = "1";
  // Signed, zero-centred, integer-valued track (§4.3). The ±10 bounds are a
  // UI-only choice — the vendor documents polarity, not bounds — and the
  // model stores the raw number, so no vendor fact is invented.
  slider.title = "Luminosity (−10…+10, UI range; 0 = neutral)";
  slider.setAttribute("aria-label", "Luminosity");
  const paintLum = (): void => {
    const { value, source } = resolveGalleryLuminosity(hooks.getCascade(), kind);
    lumVal.textContent = `${value >= 0 ? "+" : ""}${Math.round(value)}`;
    lumSrc.textContent = `at ${source ?? "default"}`;
    slider.value = String(Math.max(-10, Math.min(10, Math.round(value))));
  };
  const setLum = (v: number): void => {
    hooks.setCascade(setGalleryLuminosity(hooks.getCascade(), kind, v));
    paintLum();
  };
  slider.addEventListener("input", () => setLum(Number(slider.value)));
  const step = (d: number): void => {
    const { value } = resolveGalleryLuminosity(hooks.getCascade(), kind);
    setLum(value + d);
  };
  const minus = document.createElement("button");
  minus.textContent = "⊟";
  minus.addEventListener("click", () => step(-1));
  const plus = document.createElement("button");
  plus.textContent = "⊞";
  plus.addEventListener("click", () => step(1));
  sec.appendChild(controlRow("Luminosité:", minus, slider, plus, lumVal, lumSrc));
  paintLum();

  const pin = document.createElement("button");
  const paintPin = (): void => {
    pin.textContent = hooks.getKeepOpen() ? "☑ Keep the gallery opened" : "☐ Keep the gallery opened";
  };
  pin.addEventListener("click", () => { hooks.setKeepOpen(!hooks.getKeepOpen()); paintPin(); });
  paintPin();
  sec.appendChild(controlRow("Pin:", pin));

  // Click-to-select arms Delete (assignment is single-click on the thumb).
  grid.addEventListener("click", (e) => {
    const t = (e.target as HTMLElement).closest(".gallery-thumb") as HTMLElement | null;
    if (!t) return;
    grid.querySelectorAll(".gallery-thumb.selected").forEach((x) => x.classList.remove("selected"));
    t.classList.add("selected");
  });

  (sec as unknown as { repaint: () => void }).repaint = () => { paint(); paintLum(); paintPin(); };
  paint();
  return sec;
}

/** Mount both galleries + transport into `#media-galleries`. */
export function mountGalleries(root: HTMLElement, hooks: GalleryHooks): void {
  root.innerHTML = "";
  const head = h("h3", "Background galleries (user files only)");
  root.appendChild(head);

  const imgSec = gallerySection("image", hooks);
  const vidSec = gallerySection("video", hooks);
  const repaintAll = (): void => {
    for (const s of [imgSec, vidSec]) {
      (s as unknown as { repaint: () => void }).repaint();
    }
  };

  // Shared import row (by path; no embedded picker in v1).
  const imp = h("div");
  imp.className = "gallery-row";
  imp.appendChild(h("span", "Import file path:"));
  const pathInput = document.createElement("input");
  pathInput.size = 30;
  pathInput.placeholder = "/path/to/file…";
  imp.appendChild(pathInput);
  for (const kind of ["image", "video"] as const) {
    const b = document.createElement("button");
    b.textContent = kind === "image" ? "+ Import image" : "+ Import video";
    b.addEventListener("click", () => {
      const p = pathInput.value.trim();
      if (!p) { hooks.hint("Import: type a file path first."); return; }
      hooks.importFile(p, kind);
      pathInput.value = "";
      repaintAll();
    });
    imp.appendChild(b);
  }
  const note = h("span", "Rebuild thumbnails: files render directly — nothing cached, nothing to rebuild.");
  note.className = "hint";
  imp.appendChild(note);

  // Transport: play/pause/stop + in/out range + verse/song scope.
  const tr = h("div");
  tr.className = "gallery-row";
  tr.appendChild(h("span", "Transport:"));
  const state = h("strong", hooks.getTransport());
  const setState = (): void => { state.textContent = hooks.getTransport(); };
  for (const [act, glyph] of [["play", "▶ play"], ["toggle", "⏸ pause"], ["stop", "⏹ stop"]] as const) {
    const b = document.createElement("button");
    b.textContent = glyph;
    b.addEventListener("click", () => { hooks.transport(act); setState(); });
    tr.appendChild(b);
  }
  tr.appendChild(state);
  const range = document.createElement("input");
  range.size = 14;
  const cur = hooks.getRange();
  range.value = cur.startMs !== null || cur.endMs !== null
    ? `${cur.startMs ?? ""}-${cur.endMs ?? ""}` : "";
  range.placeholder = "in-out ms, e.g. 1000-5000";
  range.title = "Cut range (I/O keys equivalent)";
  range.setAttribute("aria-label", "Cut range in milliseconds");
  range.addEventListener("change", () => {
    const { startMs, endMs } = parseRange(range.value);
    hooks.setRange({ ...hooks.getRange(), startMs, endMs });
  });
  tr.appendChild(range);
  for (const scope of ["verse", "song"] as const) {
    const b = document.createElement("button");
    b.textContent = scope === "verse" ? "verse cut" : "song cut";
    b.classList.toggle("on", hooks.getRange().scope === scope);
    b.addEventListener("click", () => {
      hooks.setRange({ ...hooks.getRange(), scope });
      tr.querySelectorAll("button").forEach((x) => {
        if (x.textContent === "verse cut" || x.textContent === "song cut") {
          x.classList.toggle("on", x.textContent === (scope === "verse" ? "verse cut" : "song cut"));
        }
      });
    });
    tr.appendChild(b);
  }

  const close = document.createElement("button");
  const paintAll = (): void => {
    const collapsed = hooks.getCollapsed();
    imgSec.hidden = collapsed;
    vidSec.hidden = collapsed;
    imp.hidden = collapsed;
    tr.hidden = collapsed;
    close.textContent = collapsed
      ? `▸ Galleries (${hooks.listMedia().filter((e) => e.kind === "image").length} images, ${hooks.listMedia().filter((e) => e.kind === "video").length} videos) — expand`
      : "✖ Close galleries";
  };
  close.addEventListener("click", () => {
    hooks.setCollapsed(!hooks.getCollapsed());
    paintAll();
    repaintAll();
  });
  root.appendChild(close);
  root.appendChild(imgSec);
  root.appendChild(vidSec);
  root.appendChild(imp);
  root.appendChild(tr);
  // Agenda reloads replace the media list underneath us — repaint the grids
  // (search text survives; only the rows rebuild).
  root.addEventListener("vp:media-changed", repaintAll);
  paintAll();
}

/**
 * Enforce a cut range on freshly projected preview media (control 11): seek
 * to the in-point on load and loop inside [in, out] while it plays. The
 * range lives per assignment (§11.2); the loop, not a trim of the file.
 */
export function wireRangeLoop(container: HTMLElement, range: CutRange): void {
  const m = container.querySelector("video:not(.vp-bgvideo), audio") as HTMLMediaElement | null;
  if (!m) return;
  const { startMs, endMs } = range;
  if (startMs === null && endMs === null) return;
  if (startMs !== null) {
    try { m.currentTime = startMs / 1000; } catch { /* media not ready */ }
  }
  m.addEventListener("timeupdate", () => {
    if (endMs !== null && m.currentTime * 1000 >= endMs) {
      if (startMs !== null) {
        try { m.currentTime = startMs / 1000; } catch { /* media not ready */ }
      } else {
        m.pause();
      }
    }
  });
}

// ---------------------------------------------------------------------------
// One-call renderer wiring: keeps renderer.ts to an import + a context object
// ---------------------------------------------------------------------------

/** Structural agenda slice the galleries need (no renderer internals leak). */
export interface GalleryCtx {
  getItems(): { id: string; title: string; kind: string; file?: string }[];
  /** Files discovered in a VideoPsalm document folder (Images/ + Videos/). */
  getExtraMedia(): { path: string; kind: GalleryKind }[];
  addItem(kind: "Image" | "Video", title: string, file: string): void;
  removeItem(id: string): void;
  getBg(): { kind: string; file: string };
  setBg(kind: string, file: string): void;
  getCascade(): Cascade;
  setCascade(c: Cascade): void;
  getKeepOpen(): boolean;
  setKeepOpen(b: boolean): void;
  getCollapsed(): boolean;
  setCollapsed(b: boolean): void;
  getTransport(): Transport;
  doTransport(action: "play" | "pause" | "stop" | "toggle"): void;
  getRange(): CutRange;
  setRange(r: CutRange): void;
  afterChange(): void;
  hint(t: string): void;
}

/**
 * Full wiring for the control window: entries are the agenda's media items
 * plus the current background file; imports add agenda items; deletes remove
 * them (agenda goes dirty, as with any agenda mutation).
 */
export function mountMediaGalleries(root: HTMLElement, ctx: GalleryCtx): void {
  mountGalleries(root, {
    listMedia: () => {
      const out: GalleryEntry[] = [];
      for (const it of ctx.getItems()) {
        if ((it.kind === "Image" || it.kind === "Video" || it.kind === "Audio") && it.file) {
          out.push({
            id: it.id,
            path: it.file,
            kind: it.kind === "Video" ? "video" : "image",
            blackFlashMs: BLACK_FLASH_DEFAULT_MS,
          });
        }
      }
      // Native VideoPsalm folder media: Images/ and Videos/ are the original's
      // galleries. They are shown alongside agenda media but are not agenda
      // items, so assigning one only changes the live background.
      for (const e of ctx.getExtraMedia()) {
        if (!out.some((x) => x.path === e.path)) {
          out.push({
            id: `vp-folder:${e.path}`,
            path: e.path,
            kind: e.kind,
            blackFlashMs: BLACK_FLASH_DEFAULT_MS,
          });
        }
      }
      const bg = ctx.getBg();
      if (bg.file && !out.some((e) => e.path === bg.file)) {
        out.push({
          id: "bg-current",
          path: bg.file,
          kind: bg.kind === "videoLoop" ? "video" : "image",
          blackFlashMs: BLACK_FLASH_DEFAULT_MS,
        });
      }
      return out;
    },
    importFile: (path, kind) => {
      ctx.addItem(kind === "video" ? "Video" : "Image", fileCaption(path), path);
    },
    deleteEntry: (id) => ctx.removeItem(id),
    assignBackground: (kind, path) => ctx.setBg(path === null ? "color" : assignedBgKind(kind), path ?? ""),
    assignedPath: () => {
      const bg = ctx.getBg();
      return bg.kind === "color" ? null : bg.file || null;
    },
    getCascade: ctx.getCascade,
    setCascade: (c) => { ctx.setCascade(c); ctx.afterChange(); },
    getKeepOpen: ctx.getKeepOpen,
    setKeepOpen: ctx.setKeepOpen,
    getCollapsed: ctx.getCollapsed,
    setCollapsed: ctx.setCollapsed,
    getTransport: ctx.getTransport,
    transport: (a) => ctx.doTransport(a),
    getRange: ctx.getRange,
    setRange: ctx.setRange,
    hint: ctx.hint,
  });
}
