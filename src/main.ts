// Electron main process: control window + fullscreen projector + stage view.
// Offline-first. No network calls. IPC: vp:project, vp:stage, vp:alert, vp:clock.

import { BrowserWindow, app, ipcMain, screen, dialog } from "electron";
import * as fs from "fs";
import * as path from "path";
import { openLibrary, type Library } from "./core/library";
import { parseAgenda, serializeAgenda, createAgenda } from "./core/agendaSerde";
import type { Agenda } from "./core/schemas";

let controlWin: BrowserWindow | null = null;
let projectorWin: BrowserWindow | null = null;
let stageWin: BrowserWindow | null = null;
let library: Library | null = null;

let currentAgenda: Agenda = createAgenda("Untitled");
let currentSlide: { title: string; html: string } = { title: "", html: "" };
let stageState = { current: "", next: "", clock: "", alert: { visible: false, text: "" } };
let alertState = { visible: false, text: "" };

function distFile(...parts: string[]): string {
  return path.join(__dirname, ...parts);
}

/** The bundled `data/` directory, whichever of the two layouts we run from. */
function dataDir(): string {
  const dataDirPath = path.join(app.getAppPath(), "..", "electron-app", "data");
  const altDir = path.join(__dirname, "..", "..", "data");
  return fs.existsSync(path.join(dataDirPath, "songs.sample.json")) ? dataDirPath : altDir;
}

function createControlWindow(): void {
  controlWin = new BrowserWindow({
    width: 1280,
    height: 860,
    title: "VideoPsalm (Electron v1) — Control",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
  controlWin.loadFile(distFile("renderer", "index.html"));
  if (process.argv.includes("--dev")) controlWin.webContents.openDevTools({ mode: "detach" });
  controlWin.on("closed", () => {
    controlWin = null;
  });
}

function createProjectorWindow(): BrowserWindow {
  if (projectorWin && !projectorWin.isDestroyed()) {
    projectorWin.focus();
    return projectorWin;
  }
  const displays = screen.getAllDisplays();
  const target = displays.length > 1 ? displays[displays.length - 1] : displays[0];
  projectorWin = new BrowserWindow({
    x: target.bounds.x,
    y: target.bounds.y,
    fullscreen: true,
    backgroundColor: "#000000",
    title: "VideoPsalm — Projector",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
  projectorWin.loadFile(distFile("renderer", "projector.html"));
  projectorWin.on("closed", () => {
    projectorWin = null;
  });
  return projectorWin;
}

function createStageWindow(): BrowserWindow {
  if (stageWin && !stageWin.isDestroyed()) {
    stageWin.focus();
    return stageWin;
  }
  stageWin = new BrowserWindow({
    width: 800,
    height: 500,
    title: "VideoPsalm — Stage view",
    webPreferences: { preload: distFile("preload.js"), contextIsolation: true, nodeIntegration: false },
  });
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

function registerIpc(): void {
  ipcMain.handle("vp:get-agenda", () => currentAgenda);
  ipcMain.handle("vp:set-agenda", (_e, agenda: Agenda) => {
    currentAgenda = agenda;
    return true;
  });
  ipcMain.handle("vp:new-agenda", (_e, title: string) => {
    currentAgenda = createAgenda(title || "Untitled");
    return currentAgenda;
  });
  ipcMain.handle("vp:open-agenda", async () => {
    const res = await dialog.showOpenDialog({ filters: [{ name: "Agenda JSON", extensions: ["json", "vpsalm"] }], properties: ["openFile"] });
    if (res.canceled || res.filePaths.length === 0) return null;
    const text = fs.readFileSync(res.filePaths[0], "utf8");
    currentAgenda = parseAgenda(text);
    library?.recordRecent(res.filePaths[0]);
    return { path: res.filePaths[0], agenda: currentAgenda };
  });
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
  ipcMain.handle("vp:import-sample-data", () => {
    const dir = dataDir();
    const songs = JSON.parse(fs.readFileSync(path.join(dir, "songs.sample.json"), "utf8")) as import("./core/schemas").Song[];
    for (const s of songs) library?.addSong(s);
    return songs.length;
  });
  // Installed Bible translations (SPEC 2.2/2.3): one document per sample
  // file, so the renderer can offer the translation tabs and run the
  // keyword/phrase search over real verse text.
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
    return out;
  });
  ipcMain.handle("vp:find-songs", (_e, query: string, field?: string) => {
    const songs = library?.findSongs(query) ?? [];
    if (!field || field === "all") return songs;
    const q = query.toLowerCase();
    return songs.filter((s) => {
      if (field === "title") return s.title.toLowerCase().startsWith(q);
      if (field === "author") return s.authors.some((a) => a.toLowerCase().includes(q));
      if (field === "ccli") return (s.ccli ?? "").includes(query);
      if (field === "id") return (s.songId ?? "").includes(query);
      return true;
    });
  });
  ipcMain.handle("vp:open-projector", () => {
    createProjectorWindow();
    return true;
  });
  ipcMain.handle("vp:open-stage", () => {
    createStageWindow();
    return true;
  });
  ipcMain.handle("vp:project", (_e, slide: { title: string; html: string }, next?: string) => {
    currentSlide = slide;
    createProjectorWindow();
    setTimeout(pushProjector, 150);
    stageState = { ...stageState, current: slide.title, next: next ?? stageState.next };
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

app.whenReady().then(() => {
  const userDir = path.join(app.getPath("userData"), "library");
  library = openLibrary(userDir);
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
