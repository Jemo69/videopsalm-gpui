// Opt-in auto-update (electron-updater, GitHub releases provider).
//
// Offline-first contract: NOTHING runs at startup and no network happens
// unless the operator explicitly picks "Check for Updates…" from the menu.
// When packaged, that check queries the perez-js GitHub releases feed,
// downloads only with consent, and installs on quit. In dev (`npm start`)
// or when electron-updater is not installed, every entry point is a safe
// no-op that reports "unavailable" instead of throwing.

import { app, dialog, BrowserWindow } from "electron";

interface UpdaterShape {
  autoDownload: boolean;
  checkForUpdates(): Promise<unknown>;
  downloadUpdate(): Promise<unknown>;
  quitAndInstall(isSilent?: boolean, isForceRunAfter?: boolean): void;
  on(event: string, listener: (...args: unknown[]) => void): unknown;
  logger: unknown;
}

function loadUpdater(): UpdaterShape | null {
  if (!app.isPackaged) return null;
  try {
    // Lazy: electron-updater is a dist-only dep; dev/test trees may lack it.
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    const mod = require("electron-updater") as { autoUpdater: UpdaterShape };
    return mod.autoUpdater;
  } catch {
    return null;
  }
}

function controlWindow(): BrowserWindow | null {
  const wins = BrowserWindow.getAllWindows();
  return wins.length > 0 ? wins[0] : null;
}

function notify(status: string, detail?: string): void {
  controlWindow()?.webContents.send("vp:update-status", { status, detail });
}

/** Wire one-shot listeners; call once at startup (no network on its own). */
export function initUpdater(): void {
  const updater = loadUpdater();
  if (!updater) return;
  updater.autoDownload = false;
  updater.logger = null;
  updater.on("update-available", (info: unknown) => {
    const version =
      (info as { version?: string } | null)?.version ?? "unknown version";
    notify("available", version);
    const win = controlWindow();
    if (!win) return;
    void dialog
      .showMessageBox(win, {
        type: "question",
        title: "Perez JS update",
        message: `Version ${version} is available. Download it now?`,
        buttons: ["Download", "Later"],
        defaultId: 0,
        cancelId: 1,
      })
      .then(({ response }) => {
        if (response === 0) void updater.downloadUpdate().catch(() => undefined);
      });
  });
  updater.on("update-downloaded", (info: unknown) => {
    const version =
      (info as { version?: string } | null)?.version ?? "unknown version";
    notify("downloaded", version);
    const win = controlWindow();
    if (!win) return;
    void dialog
      .showMessageBox(win, {
        type: "question",
        title: "Perez JS update",
        message: `Version ${version} downloaded. Restart and install now?`,
        buttons: ["Restart now", "On next launch"],
        defaultId: 0,
        cancelId: 1,
      })
      .then(({ response }) => {
        if (response === 0) updater.quitAndInstall(false, true);
      });
  });
  updater.on("error", (err: unknown) => {
    notify("error", err instanceof Error ? err.message : String(err));
  });
}

/** Menu entry point: the operator's explicit consent to use the network. */
export function checkForUpdatesNow(): void {
  const updater = loadUpdater();
  if (!updater) {
    notify("unavailable", "Updates are only checked in installed builds.");
    const win = controlWindow();
    if (win) {
      void dialog.showMessageBox(win, {
        type: "info",
        title: "Perez JS update",
        message: "Automatic updates run in installed builds only — this is a development run.",
      });
    }
    return;
  }
  notify("checking");
  void updater
    .checkForUpdates()
    .then((result) => {
      if (!result || !(result as { updateInfo?: unknown }).updateInfo) {
        notify("current", "You are on the latest version.");
      }
    })
    .catch((e: unknown) => {
      notify("error", e instanceof Error ? e.message : String(e));
    });
}
