// Preload: minimal, auditable IPC bridge (context isolation ON).
import { contextBridge, ipcRenderer } from "electron";

const api = {
  getAgenda: () => ipcRenderer.invoke("vp:get-agenda"),
  setAgenda: (agenda: unknown) => ipcRenderer.invoke("vp:set-agenda", agenda),
  newAgenda: (title: string) => ipcRenderer.invoke("vp:new-agenda", title),
  openAgenda: () => ipcRenderer.invoke("vp:open-agenda"),
  openVideopsalmAgenda: () => ipcRenderer.invoke("vp:open-videopsalm-agenda"),
  importVideopsalmAgenda: (file: string) => ipcRenderer.invoke("vp:import-videopsalm-agenda", file),
  saveAgenda: (filePath?: string) => ipcRenderer.invoke("vp:save-agenda", filePath),
  /** Export the agenda back to the native VideoPsalm plan shape (`.vpagd`). */
  saveVideopsalmAgenda: () => ipcRenderer.invoke("vp:save-videopsalm-agenda"),
  importSampleData: () => ipcRenderer.invoke("vp:import-sample-data"),
  openVideopsalmSongbook: () => ipcRenderer.invoke("vp:open-videopsalm-songbook"),
  openVideopsalmFolder: () => ipcRenderer.invoke("vp:open-videopsalm-folder"),
  scanVideopsalmFolder: (folder: string) => ipcRenderer.invoke("vp:scan-videopsalm-folder", folder),
  defaultVideopsalmFolder: () => ipcRenderer.invoke("vp:default-videopsalm-folder"),
  getBible: () => ipcRenderer.invoke("vp:get-bible"),
  importBible: (filePath: string) => ipcRenderer.invoke("vp:import-bible", filePath),
  /** File picker for a Bible XML translation (empty string when cancelled). */
  pickBibleXml: () => ipcRenderer.invoke("vp:pick-bible-xml"),
  findSongs: (query: string, field?: string) => ipcRenderer.invoke("vp:find-songs", query, field),
  saveSong: (song: unknown, edits?: string[]) => ipcRenderer.invoke("vp:save-song", song, edits),
  /**
   * Blocking save used on window close: the debounced write must not be
   * lost when the operator quits right after an edit.
   */
  saveSongSync: (song: unknown) => ipcRenderer.sendSync("vp:save-song-sync", song),
  openProjector: (monitor?: number) => ipcRenderer.invoke("vp:open-projector", monitor),
  openStage: () => ipcRenderer.invoke("vp:open-stage"),
  project: (slide: unknown, next?: string, monitor?: number) => ipcRenderer.invoke("vp:project", slide, next, monitor),
  stop: () => ipcRenderer.invoke("vp:stop"),
  alert: (text: string, visible: boolean) => ipcRenderer.invoke("vp:alert", text, visible),
  clock: (text: string) => ipcRenderer.invoke("vp:clock", text),
  getProjectorState: () => ipcRenderer.invoke("vp:get-projector-state"),
  getStageState: () => ipcRenderer.invoke("vp:get-stage-state"),
  libraryBackend: () => ipcRenderer.invoke("vp:library-backend"),
  displayCount: () => ipcRenderer.invoke("vp:get-display-count"),
  onRendered: (cb: (data: unknown) => void) => ipcRenderer.on("vp:render", (_e, data) => cb(data)),
  onStageRender: (cb: (data: unknown) => void) => ipcRenderer.on("vp:stage-render", (_e, data) => cb(data)),
  onClock: (cb: (text: string) => void) => ipcRenderer.on("vp:clock", (_e, text) => cb(text as string)),
  onProjected: (cb: (slide: unknown) => void) => ipcRenderer.on("vp:projected", (_e, slide) => cb(slide)),
  /** The projector reports whether it had to shrink the text to fit. */
  reportFit: (info: unknown) => ipcRenderer.send("vp:fit", info),
  onFit: (cb: (info: unknown) => void) => ipcRenderer.on("vp:fit", (_e, info) => cb(info)),
};

contextBridge.exposeInMainWorld("vp", api);
export type VpApi = typeof api;
