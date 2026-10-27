// Shared preload-bridge typing for all renderer scripts (global, no imports).
interface VpBridge {
  getAgenda(): Promise<unknown>;
  setAgenda(agenda: unknown): Promise<unknown>;
  newAgenda(title: string): Promise<unknown>;
  openAgenda(): Promise<unknown>;
  /** Open a native VideoPsalm service plan (`.vpagd`) and import its items. */
  openVideopsalmAgenda(): Promise<unknown>;
  /** Import a native VideoPsalm service plan by path. */
  importVideopsalmAgenda(file: string): Promise<unknown>;
  saveAgenda(filePath?: string): Promise<unknown>;
  /** Export the agenda back to the native VideoPsalm plan shape (`.vpagd`). */
  saveVideopsalmAgenda(): Promise<unknown>;
  /** The same export by path (test hook + scripts; no dialog). */
  saveVideopsalmAgendaPath(file: string): Promise<unknown>;
  /** Open an agenda JSON file by path (test hook + scripts; no dialog). */
  openAgendaPath(file: string): Promise<unknown>;
  /** True only when launched with `--test-hook` (exposes `window.vpTest`). */
  testHookEnabled: boolean;
  importSampleData(): Promise<unknown>;
  /** Open one native VideoPsalm `Songbooks/*.json` file and import/write-through. */
  openVideopsalmSongbook(): Promise<unknown>;
  /** Pick the VideoPsalm document folder (Songbooks + Images + Videos + styles). */
  openVideopsalmFolder(): Promise<unknown>;
  /** Rescan a previously selected VideoPsalm document folder. */
  scanVideopsalmFolder(folder: string): Promise<unknown>;
  /** Auto-detected `Documents/VideoPsalm` folder, or null. */
  defaultVideopsalmFolder(): Promise<string | null>;
  /** Installed Bible translation documents (`data/bible.sample*.json` + `bibles/*.xml`). */
  getBible(): Promise<unknown>;
  /** Read + parse a Bible XML file (OSIS / Zefania / VideoPsalm local). */
  importBible(filePath: string): Promise<unknown>;
  /** Open a file picker for a Bible XML translation ("" when cancelled). */
  pickBibleXml(): Promise<string>;
  findSongs(query: string, field?: string): Promise<unknown>;
  /** `edits` lists the native metadata fields the operator changed. */
  saveSong(song: unknown, edits?: string[]): Promise<unknown>;
  /** Blocking save for the window-close path (a pending edit must not be lost). */
  saveSongSync(song: unknown): unknown;
  /** Open the projector window, optionally on a specific display index. */
  openProjector(monitor?: number): Promise<unknown>;
  openStage(): Promise<unknown>;
  project(slide: unknown, next?: string, monitor?: number): Promise<unknown>;
  stop(): Promise<unknown>;
  alert(text: string, visible: boolean): Promise<unknown>;
  clock(text: string): Promise<unknown>;
  getProjectorState(): Promise<unknown>;
  getStageState(): Promise<unknown>;
  libraryBackend(): Promise<unknown>;
  /** Number of attached displays (single-screen guard, UX-RESEARCH §7). */
  displayCount(): Promise<number>;
  onRendered(cb: (data: unknown) => void): void;
  onStageRender(cb: (data: unknown) => void): void;
  onClock(cb: (text: string) => void): void;
  onProjected(cb: (slide: unknown) => void): void;
  /** The projector reports `{ shrunk, factor }` after fitting a slide. */
  reportFit(info: unknown): void;
  onFit(cb: (info: unknown) => void): void;
}

interface Window {
  vp: VpBridge;
}
