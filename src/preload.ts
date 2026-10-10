// Preload: minimal, auditable IPC bridge (context isolation ON).
import { contextBridge, ipcRenderer } from "electron";

const api = {
  getAgenda: () => ipcRenderer.invoke("vp:get-agenda"),
  setAgenda: (agenda: unknown) => ipcRenderer.invoke("vp:set-agenda", agenda),
  newAgenda: (title: string) => ipcRenderer.invoke("vp:new-agenda", title),
  openAgenda: () => ipcRenderer.invoke("vp:open-agenda"),
  saveAgenda: (filePath?: string) => ipcRenderer.invoke("vp:save-agenda", filePath),
  importSampleData: () => ipcRenderer.invoke("vp:import-sample-data"),
  findSongs: (query: string, field?: string) => ipcRenderer.invoke("vp:find-songs", query, field),
  openProjector: () => ipcRenderer.invoke("vp:open-projector"),
  openStage: () => ipcRenderer.invoke("vp:open-stage"),
  project: (slide: unknown, next?: string) => ipcRenderer.invoke("vp:project", slide, next),
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
};

contextBridge.exposeInMainWorld("vp", api);
export type VpApi = typeof api;
