// Shared preload-bridge typing for all renderer scripts (global, no imports).
interface VpBridge {
  getAgenda(): Promise<unknown>;
  setAgenda(agenda: unknown): Promise<unknown>;
  newAgenda(title: string): Promise<unknown>;
  openAgenda(): Promise<unknown>;
  saveAgenda(filePath?: string): Promise<unknown>;
  importSampleData(): Promise<unknown>;
  /** Installed Bible translation documents (`data/bible.sample*.json`). */
  getBible(): Promise<unknown>;
  findSongs(query: string, field?: string): Promise<unknown>;
  openProjector(): Promise<unknown>;
  openStage(): Promise<unknown>;
  project(slide: unknown, next?: string): Promise<unknown>;
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
}

interface Window {
  vp: VpBridge;
}
