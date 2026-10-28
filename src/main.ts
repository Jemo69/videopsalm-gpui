// Electron main process: control window + fullscreen projector + stage view.
// Offline-first. No network calls. IPC: vp:project, vp:stage, vp:alert, vp:clock.

import { BrowserWindow, app, ipcMain, screen, dialog, Menu, type MenuItemConstructorOptions } from "electron";
import * as fs from "fs";
import * as path from "path";
import { openLibrary, type Library } from "./core/library";
import { parseAgenda, serializeAgenda, createAgenda } from "./core/agendaSerde";
import { importBibleXml } from "./core/bibleImport";
import { filterSongs, type SearchScope } from "./core/search";
import {
  readVideopsalmSongbook,
  resolveVideopsalmDataFolder,
  saveSongToVideopsalm,
  scanVideopsalmFolder,
  videopsalmFolderCandidates,
  type VideopsalmFolderScan,
} from "./core/videopsalm";
import { readVideopsalmAgenda, writeVideopsalmAgenda } from "./core/videopsalmAgenda";
import { copyMediaIntoFolder } from "./core/mediaImport";
import type { Agenda, Song } from "./core/schemas";

// Single-instance lock: worship presentation software must never run two competing projector instances
const gotSingleInstanceLock = app.requestSingleInstanceLock();
if (!gotSingleInstanceLock) {
  app.quit();
} else {
  app.on("second-instance", () => {
    if (controlWin && !controlWin.isDestroyed()) {
      if (controlWin.isMinimized()) controlWin.restore();
      controlWin.focus();
    }
  });
}

// Global process error guards
process.on("uncaughtException", (error) => {
  console.error("VideoPsalm uncaught exception:", error);
});
process.on("unhandledRejection", (reason) => {
  console.error("VideoPsalm unhandled rejection:", reason);
});

let controlWin: BrowserWindow | null = null;
let projectorWin: BrowserWindow | null = null;
let stageWin: BrowserWindow | null = null;
let library: Library | null = null;

let currentAgenda: Agenda = createAgenda("Untitled");
let currentSlide: { title: string; html: string } = { title: "", html: "" };
let stageState: { current: string; next: string; clock: string; alert: { visible: boolean; text: string }; wholeSong?: string[]; activeIndex?: number } = { current: "", next: "", clock: "", alert: { visible: false, text: "" } };
let alertState = { visible: false, text: "" };

function distFile(...parts: string[]): string {
  return path.join(__dirname, ...parts);
}

/** The bundled `data/` directory, whichever of the two layouts we run from. */
function dataDir(): string {
  const candidates = [
    path.join(app.getAppPath(), "..", "electron-app", "data"),
    path.join(__dirname, "..", "..", "data"),
    path.join(process.resourcesPath, "data"),
    path.join(app.getAppPath(), "data"),
    path.join(__dirname, "..", "data"),
  ];
  for (const p of candidates) {
    if (fs.existsSync(path.join(p, "songs.sample.json"))) return p;
  }
  return candidates[1];
}

function secureWebContents(contents: Electron.WebContents): void {
  contents.setWindowOpenHandler(() => ({ action: "deny" }));
  contents.on("will-navigate", (event, url) => {
    if (!url.startsWith("file://")) {
      event.preventDefault();
    }
  });
}

function createControlWindow(): void {
  controlWin = new BrowserWindow({
    width: 1280,
    height: 860,
    title: "VideoPsalm (Electron v1) — Control",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
  secureWebContents(controlWin.webContents);
  controlWin.loadFile(distFile("renderer", "index.html"));
  if (process.argv.includes("--dev")) controlWin.webContents.openDevTools({ mode: "detach" });
  controlWin.on("closed", () => {
    controlWin = null;
  });
}

/** Display the projector should use. The operator picks it in the ribbon
 *  ("Monitor", 1 = primary); the last choice wins, and the window follows. */
let projectorMonitor: number | undefined;

function projectorDisplay(): Electron.Display {
  const displays = screen.getAllDisplays();
  const index = projectorMonitor;
  if (typeof index === "number" && index >= 0 && index < displays.length) return displays[index];
  // No explicit choice: the last display is the projection screen, as the
  // original does.
  return displays.length > 1 ? displays[displays.length - 1] : displays[0];
}

function createProjectorWindow(): BrowserWindow {
  const displays = screen.getAllDisplays();
  const target = projectorDisplay();
  // Dual display: fullscreen on the projection screen, as the original does.
  // Single display: a windowed companion (960×540, bottom-right) — a
  // fullscreen takeover on the only screen hides the control window and the
  // operator loses the app entirely (it reads as "nothing works").
  const single = displays.length === 1;
  if (projectorWin && !projectorWin.isDestroyed()) {
    // Already open: move it to the chosen screen instead of ignoring the
    // operator's monitor choice.
    const bounds = single
      ? { x: target.bounds.x + Math.max(0, target.bounds.width - 980), y: target.bounds.y + Math.max(0, target.bounds.height - 560), width: 960, height: 540 }
      : target.bounds;
    projectorWin.setBounds(bounds);
    projectorWin.setFullScreen(!single);
    projectorWin.focus();
    return projectorWin;
  }
  projectorWin = new BrowserWindow({
    x: single ? target.bounds.x + Math.max(0, target.bounds.width - 980) : target.bounds.x,
    y: single ? target.bounds.y + Math.max(0, target.bounds.height - 560) : target.bounds.y,
    width: single ? 960 : undefined,
    height: single ? 540 : undefined,
    fullscreen: !single,
    backgroundColor: "#000000",
    title: "VideoPsalm — Projector",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
  secureWebContents(projectorWin.webContents);
  projectorWin.loadFile(distFile("renderer", "projector.html"));
  projectorWin.on("closed", () => {
    projectorWin = null;
  });
  return projectorWin;
}

function createStageWindow(): BrowserWindow {  if (stageWin && !stageWin.isDestroyed()) {
    stageWin.focus();
    return stageWin;
  }
  stageWin = new BrowserWindow({
    width: 800,
    height: 500,
    title: "VideoPsalm — Stage view",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
  secureWebContents(stageWin.webContents);
  stageWin.loadFile(distFile("renderer", "stage.html"));
  stageWin.on("closed", () => {
    stageWin = null;
  });
  return stageWin;
}

function pushProjector(): void {
  if (projectorWin && !projectorWin.isDestroyed()) {
    projectorWin.webContents.send("vp:render", { slide: currentSlide, alert: alertState });
  }
}

function pushStage(): void {
  if (stageWin && !stageWin.isDestroyed()) {
    stageWin.webContents.send("vp:stage-render", stageState);
  }
}

/** Import one native songbook into the local library. */
function importVideopsalmFile(file: string): { ok: true; file: string; songbook: unknown; songCount: number; ids: string[] } {
  const imported = readVideopsalmSongbook(file);
  for (const song of imported.songs) library?.addSong(song);
  return {
    ok: true,
    file: imported.songbook.file,
    songbook: imported.songbook,
    songCount: imported.songs.length,
    ids: imported.songs.map((s) => s.id),
  };
}

/** Import every songbook found under a VideoPsalm document folder. */
function importVideopsalmFolder(folder: string): Omit<VideopsalmFolderScan, "songs"> & { ok: boolean; songCount: number } {
  let isDir = false;
  try {
    isDir = fs.statSync(folder).isDirectory();
  } catch {
    isDir = false;
  }
  if (!isDir) {
    return {
      ok: false,
      folder: "",
      songbooks: [],
      images: [],
      videos: [],
      backgrounds: [],
      errors: [`Document folder not found: ${folder}`],
      songCount: 0,
    };
  }
  const scan = scanVideopsalmFolder(folder);
  for (const song of scan.songs) library?.addSong(song);
  const { songs: importedSongs, ...summary } = scan;
  // A folder with one corrupt/unsupported songbook is still useful: import
  // what we could read and report the rest through `errors`.
  return { ...summary, ok: true, songCount: importedSongs.length };
}

function defaultVideopsalmFolder(): string | null {
  let home = "";
  let documents = "";
  try {
    home = app.getPath("home");
    documents = app.getPath("documents");
  } catch {
    // app path APIs are unavailable in some test harnesses; env candidates
    // still let an operator point at the folder explicitly.
  }
  const hit = videopsalmFolderCandidates(home, documents).find((dir) => {
    try {
      return fs.existsSync(dir) && fs.statSync(dir).isDirectory();
    } catch {
      return false;
    }
  });
  // Follow `CommonSettings.json` when the operator moved the VideoPsalm data
  // folder; the default folder only contains the pointer.
  return hit ? resolveVideopsalmDataFolder(hit) : null;
}

/**
 * Persist one song: the local library always, and the operator's real
 * VideoPsalm songbook when the song came from one.
 */
function persistSong(song: Song, edits?: string[]): { ok: boolean; native: boolean; file?: string; error?: string } {
  song.updatedAt = new Date().toISOString();
  library?.addSong(song);
  if (song.source?.format === "videopsalm") {
    if (!song.source.file) return { ok: false, native: true, error: "VideoPsalm source file is missing" };
    try {
      saveSongToVideopsalm(song.source.file, song, edits);
      return { ok: true, native: true, file: song.source.file };
    } catch (e) {
      return { ok: false, native: true, file: song.source.file, error: e instanceof Error ? e.message : String(e) };
    }
  }
  return { ok: true, native: false };
}

function registerIpc(): void {
  // Test-hook gate: resolved here because main's argv is authoritative.
  ipcMain.on("vp:test-hook-enabled", (e) => {
    e.returnValue = process.argv.includes("--test-hook");
  });
  ipcMain.handle("vp:get-agenda", () => currentAgenda);
  ipcMain.handle("vp:set-agenda", (_e, agenda: Agenda) => {
    currentAgenda = agenda;
    return true;
  });
  ipcMain.handle("vp:new-agenda", (_e, title: string) => {
    currentAgenda = createAgenda(title || "Untitled");
    return currentAgenda;
  });
  function openAgendaFile(file: string): { path: string; agenda: Agenda } {
    const text = fs.readFileSync(file, "utf8");
    currentAgenda = parseAgenda(text);
    library?.recordRecent(file);
    return { path: file, agenda: currentAgenda };
  }
  ipcMain.handle("vp:open-agenda", async () => {
    const res = await dialog.showOpenDialog({ filters: [{ name: "Agenda JSON", extensions: ["json", "vpsalm"] }], properties: ["openFile"] });
    if (res.canceled || res.filePaths.length === 0) return null;
    return openAgendaFile(res.filePaths[0]);
  });
  // The same open, by path — the test hook (`window.vpTest`) and scripts.
  ipcMain.handle("vp:open-agenda-path", (_e, file: string) => openAgendaFile(file));
  // A native VideoPsalm service plan (`.vpagd`): a ZIP of per-item
  // projections. The songs it carries are added to the library only when the
  // operator's own songbook does not already have them, so a plan never
  // overwrites the real songbook copy (and its write-back target).
  function importVideopsalmAgendaFile(file: string): {
    ok: boolean;
    error?: string;
    file?: string;
    agenda?: Agenda;
    songCount?: number;
    itemCount?: number;
    media?: string[];
    warnings?: string[];
  } {
    try {
      const imported = readVideopsalmAgenda(file);
      const known = new Set((library?.findSongs("") ?? []).map((s) => s.id));
      let added = 0;
      for (const song of imported.songs) {
        if (known.has(song.id)) continue;
        library?.addSong(song);
        added += 1;
      }
      currentAgenda = {
        version: 1,
        title: imported.title,
        items: imported.items,
        settings: {},
      };
      library?.recordRecent(file);
      return {
        ok: true,
        file,
        agenda: currentAgenda,
        songCount: added,
        itemCount: imported.items.length,
        media: imported.media,
        warnings: imported.warnings,
      };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
  }
  ipcMain.handle("vp:open-videopsalm-agenda", async () => {
    const res = await dialog.showOpenDialog({
      title: "Open a VideoPsalm service plan",
      filters: [{ name: "VideoPsalm agenda", extensions: ["vpagd"] }],
      properties: ["openFile"],
    });
    if (res.canceled || res.filePaths.length === 0) return { ok: false, canceled: true };
    return importVideopsalmAgendaFile(res.filePaths[0]);
  });
  // The same import, by path (a recent file, a drag & drop, a script).
  ipcMain.handle("vp:import-videopsalm-agenda", (_e, file: string) => importVideopsalmAgendaFile(file));
  ipcMain.handle("vp:save-agenda", async (_e, filePath?: string) => {
    let target = filePath;
    if (!target) {
      const res = await dialog.showSaveDialog({ filters: [{ name: "Agenda JSON", extensions: ["json"] }], defaultPath: "agenda.json" });
      if (res.canceled || !res.filePath) return null;
      target = res.filePath;
    }
    fs.writeFileSync(target, serializeAgenda(currentAgenda));
    library?.recordRecent(target);
    return target;
  });
  // Export the agenda back to the native VideoPsalm plan shape (`.vpagd`):
  // a plain multi-entry ZIP the original opens with `Agenda.LoadFromFile`.
  // Songs resolve against the library so the plan carries full lyrics;
  // kinds with no native projection are skipped with a warning.
  function saveVideopsalmAgendaFile(file: string): { ok: boolean; file?: string; itemCount?: number; media?: string[]; warnings?: string[]; error?: string } {
    try {
      const songs = new Map((library?.findSongs("") ?? []).map((s) => [s.id, s] as const));
      const written = writeVideopsalmAgenda(file, currentAgenda, songs);
      library?.recordRecent(file);
      return { ok: true, file, itemCount: currentAgenda.items.length, media: written.media, warnings: written.warnings };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
  }
  ipcMain.handle("vp:save-videopsalm-agenda", async () => {
    const res = await dialog.showSaveDialog({
      title: "Save as a VideoPsalm service plan",
      filters: [{ name: "VideoPsalm agenda", extensions: ["vpagd"] }],
      defaultPath: `${currentAgenda.title || "Agenda"}.vpagd`,
    });
    if (res.canceled || !res.filePath) return { ok: false, canceled: true };
    return saveVideopsalmAgendaFile(res.filePath);
  });
  // The same export, by path — the test hook (`window.vpTest`) and scripts.
  ipcMain.handle("vp:save-videopsalm-agenda-path", (_e, file: string) => saveVideopsalmAgendaFile(file));
  // Native media import: pick files in the OS explorer and file them
  // straight into the VideoPsalm document folder (Images/ or Videos/).
  // Falls back to the app's own Media dir when no document folder is open.
  ipcMain.handle("vp:import-media", async (_e, folder?: string) => {
    const res = await dialog.showOpenDialog({
      title: "Import media into the VideoPsalm folder",
      filters: [
        { name: "Images", extensions: ["jpg", "jpeg", "jpe", "jfif", "png", "bmp", "tif", "tiff", "tga", "gif"] },
        { name: "Videos", extensions: ["asf", "avi", "mp4", "m4v", "mkv", "mov", "mpg", "mpeg", "wmv", "webm", "ogv"] },
        { name: "All files", extensions: ["*"] },
      ],
      properties: ["openFile", "multiSelections"],
    });
    if (res.canceled || res.filePaths.length === 0) return { ok: false, canceled: true };
    let docs = (folder ?? "").trim();
    try {
      if (!docs || !fs.statSync(docs).isDirectory()) docs = "";
    } catch {
      docs = "";
    }
    if (!docs) {
      docs = path.join(app.getPath("userData"), "Media");
      fs.mkdirSync(docs, { recursive: true });
    }
    const { copied, errors } = copyMediaIntoFolder(res.filePaths, docs);
    return { ok: true, copied, errors, folder: docs };
  });
  ipcMain.handle("vp:import-sample-data", () => {
    const dir = dataDir();
    const songs = JSON.parse(fs.readFileSync(path.join(dir, "songs.sample.json"), "utf8")) as import("./core/schemas").Song[];
    for (const s of songs) library?.addSong(s);
    return songs.length;
  });
  // Installed Bible translations (SPEC 2.2/2.3): the bundled sample files
  // plus a local import folder — drop OSIS / Zefania / VideoPsalm XML files
  // into `bibles/` next to the data dir (or userData/bibles) and they are
  // installed at boot. Mirror of the GPUI port's boot scan.
  ipcMain.handle("vp:get-bible", () => {
    const dir = dataDir();
    const out: unknown[] = [];
    for (const file of ["bible.sample.json", "bible.sample-fr.json"]) {
      const full = path.join(dir, file);
      if (!fs.existsSync(full)) continue;
      try {
        out.push(JSON.parse(fs.readFileSync(full, "utf8")));
      } catch {
        // A malformed sample must not take the Bible pane down.
      }
    }
    const scanDirs = [path.join(dir, "bibles")];
    try {
      scanDirs.push(path.join(app.getPath("userData"), "bibles"));
    } catch {
      // userData may be unavailable in tests; the data-dir scan still runs.
    }
    for (const scanDir of scanDirs) {
      if (!fs.existsSync(scanDir)) continue;
      let names: string[] = [];
      try {
        names = fs
          .readdirSync(scanDir)
          .filter((f) => f.toLowerCase().endsWith(".xml"))
          .sort();
      } catch {
        continue;
      }
      for (const name of names) {
        try {
          const xml = fs.readFileSync(path.join(scanDir, name), "utf8");
          out.push(importBibleXml(xml, name.replace(/\.xml$/i, "")));
        } catch (e) {
          // One bad file must not take the Bible pane down.
          console.warn(`bibles/${name}: ${e instanceof Error ? e.message : e}`);
        }
      }
    }
    return out;
  });
  // Import-XML row in the Bibles pane: read + parse here (core runs in both
  // processes), hand the renderer a ready translation.
  ipcMain.handle("vp:import-bible", (_e, filePath: string) => {
    try {
      const xml = fs.readFileSync(filePath, "utf8");
      const fallback =
        path.basename(filePath).replace(/\.[^.]+$/, "") || "Imported";
      return { ok: true, translation: importBibleXml(xml, fallback) };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
  });
  ipcMain.handle("vp:pick-bible-xml", async () => {
    const res = await dialog.showOpenDialog({
      title: "Choose a Bible translation (OSIS / Zefania XML)",
      filters: [
        { name: "Bible XML", extensions: ["xml"] },
        { name: "All files", extensions: ["*"] },
      ],
      properties: ["openFile"],
    });
    return res.canceled || res.filePaths.length === 0 ? "" : res.filePaths[0];
  });
  ipcMain.handle("vp:find-songs", (_e, query: string, field?: string) => {
    const all = library?.findSongs("") ?? [];
    const scope = (field && field !== "all" ? field : "title-lyrics") as SearchScope;
    return filterSongs(all, query, scope);
  });
  // Native VideoPsalm songbooks: open one file (`.json` or an AES `.vpc`), or
  // scan a document folder (`SongBooks/`, `Images/`, `Videos/`,
  // `*Style*.json`). Imported songs keep a `source` locator so lyric/title
  // edits write straight back to the real file instead of only the local
  // library; `.vpc` is re-encrypted with VideoPsalm's fixed container key.
  ipcMain.handle("vp:open-videopsalm-songbook", async () => {
    const res = await dialog.showOpenDialog({
      title: "Open a VideoPsalm songbook",
      filters: [{ name: "VideoPsalm songbook", extensions: ["json", "vpc"] }],
      properties: ["openFile"],
    });
    if (res.canceled || res.filePaths.length === 0) return { ok: false, canceled: true };
    try {
      return importVideopsalmFile(res.filePaths[0]);
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : String(e) };
    }
  });
  ipcMain.handle("vp:open-videopsalm-folder", async () => {
    const res = await dialog.showOpenDialog({
      title: "Select the VideoPsalm document folder",
      properties: ["openDirectory"],
    });
    if (res.canceled || res.filePaths.length === 0) return { ok: false, canceled: true };
    return importVideopsalmFolder(res.filePaths[0]);
  });
  ipcMain.handle("vp:scan-videopsalm-folder", (_e, folder: string) => importVideopsalmFolder(folder));
  ipcMain.handle("vp:default-videopsalm-folder", () => defaultVideopsalmFolder());

  // Lyrics edits: the centre editor is the live source of truth while a song
  // is open; each edit upserts the song so the filmstrip, the projector and a
  // restart all see the same text (addSong upserts on id in both backends).
  // For native VideoPsalm songs the same edit is also written back into the
  // operator's songbook file, preserving every field the editor does not own.
  ipcMain.handle("vp:save-song", (_e, song: Song, edits?: string[]) => persistSong(song, edits));
  // Blocking variant for the window-close path: a pending edit must reach the
  // operator's songbook even when the app is closed right after typing.
  ipcMain.on("vp:save-song-sync", (_e, song: Song) => {
    try {
      persistSong(song);
    } catch {
      // The window is closing; there is nobody left to show an error to.
    }
  });
  ipcMain.handle("vp:open-projector", (_e, monitor?: number) => {
    if (typeof monitor === "number" && monitor >= 0) projectorMonitor = monitor;
    createProjectorWindow();
    return true;
  });
  // The projector tells the control window when it had to shrink the lyrics to
  // fit the frame, so the operator can split a long verse before the service.
  ipcMain.on("vp:fit", (_e, info: { shrunk?: boolean; factor?: number }) => {
    if (controlWin && !controlWin.isDestroyed()) controlWin.webContents.send("vp:fit", info);
  });
  ipcMain.handle("vp:open-stage", () => {
    createStageWindow();
    return true;
  });
  ipcMain.handle("vp:project", (_e, slide: { title: string; html: string; wholeSong?: string[]; activeIndex?: number }, next?: string, monitor?: number) => {
    currentSlide = slide;
    if (typeof monitor === "number" && monitor >= 0) projectorMonitor = monitor;
    createProjectorWindow();
    setTimeout(pushProjector, 150);
    stageState = {
      ...stageState,
      current: slide.title,
      next: next ?? stageState.next,
      wholeSong: slide.wholeSong,
      activeIndex: slide.activeIndex,
    };
    pushStage();
    controlWin?.webContents.send("vp:projected", slide);
    return true;
  });
  ipcMain.handle("vp:stop", () => {
    currentSlide = { title: "", html: "" };
    pushProjector();
    return true;
  });
  ipcMain.handle("vp:alert", (_e, text: string, visible: boolean) => {
    alertState = { text, visible };
    stageState = { ...stageState, alert: { text, visible } };
    pushProjector();
    pushStage();
    return true;
  });
  ipcMain.handle("vp:clock", (_e, clockText: string) => {
    stageState = { ...stageState, clock: clockText };
    pushStage();
    if (projectorWin && !projectorWin.isDestroyed()) {
      projectorWin.webContents.send("vp:clock", clockText);
    }
    return true;
  });
  ipcMain.handle("vp:get-projector-state", () => ({ slide: currentSlide, alert: alertState }));
  ipcMain.handle("vp:get-stage-state", () => stageState);
  // Single-screen guard (UX-RESEARCH §7 `*` remark): the `*` shortcuts are
  // disabled with two or more screens attached.
  ipcMain.handle("vp:get-display-count", () => screen.getAllDisplays().length);
  ipcMain.handle("vp:library-backend", () => library?.backend ?? "none");
}

function setupApplicationMenu(): void {
  const isMac = process.platform === "darwin";
  const template: MenuItemConstructorOptions[] = [
    ...(isMac ? [{ role: "appMenu" as const }] : []),
    {
      label: "File",
      submenu: [
        {
          label: "New Agenda",
          accelerator: "CmdOrCtrl+N",
          click: () => {
            currentAgenda = createAgenda("Untitled");
            controlWin?.webContents.send("vp:agenda-changed", currentAgenda);
          },
        },
        { type: "separator" },
        isMac ? { role: "close" as const } : { role: "quit" as const },
      ],
    },
    {
      label: "Edit",
      submenu: [
        { role: "undo" as const },
        { role: "redo" as const },
        { type: "separator" },
        { role: "cut" as const },
        { role: "copy" as const },
        { role: "paste" as const },
        { role: "selectAll" as const },
      ],
    },
    {
      label: "View",
      submenu: [
        { role: "reload" as const },
        { role: "forceReload" as const },
        { role: "toggleDevTools" as const },
        { type: "separator" },
        { role: "resetZoom" as const },
        { role: "zoomIn" as const },
        { role: "zoomOut" as const },
        { type: "separator" },
        { role: "togglefullscreen" as const },
      ],
    },
    {
      label: "Window",
      submenu: [
        { role: "minimize" as const },
        { role: "zoom" as const },
        ...(isMac ? [{ type: "separator" as const }, { role: "front" as const }] : []),
      ],
    },
    {
      label: "Help",
      submenu: [
        {
          label: "VideoPsalm Help",
          click: () => {
            controlWin?.webContents.send("vp:toggle-shortcuts");
          },
        },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

app.whenReady().then(() => {
  const userDir = path.join(app.getPath("userData"), "library");
  library = openLibrary(userDir);
  // First run (or an empty library): seed the bundled samples so the app is
  // usable the moment it opens — GPUI parity: it boots with the sample songs
  // and agenda already loaded instead of three empty panes.
  try {
    if (library.findSongs("").length === 0) {
      const dir = dataDir();
      const songs = JSON.parse(
        fs.readFileSync(path.join(dir, "songs.sample.json"), "utf8")
      ) as import("./core/schemas").Song[];
      for (const s of songs) library.addSong(s);
    }
  } catch (e) {
    console.warn("sample songs:", e instanceof Error ? e.message : e);
  }
  try {
    const dir = dataDir();
    const agendaPath = path.join(dir, "agenda.sample.json");
    if (currentAgenda.items.length === 0 && fs.existsSync(agendaPath)) {
      currentAgenda = parseAgenda(fs.readFileSync(agendaPath, "utf8"));
    }
  } catch (e) {
    console.warn("sample agenda:", e instanceof Error ? e.message : e);
  }
  setupApplicationMenu();
  registerIpc();
  createControlWindow();
  if (process.argv.includes("--control-only")) {
    // skip projector auto-open; control window only
  }
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createControlWindow();
  });
});

app.on("window-all-closed", () => {
  library?.close();
  if (process.platform !== "darwin") app.quit();
});
