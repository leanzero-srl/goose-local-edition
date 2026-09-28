import type { App } from 'electron';

/**
 * The Mac woke from sleep (DESIGN-SESSION-LOOPS §5.4, slice L10). goosed's loop runner keeps its
 * clock on tokio's `Instant`, which does not count the time a Mac slept, so a tick armed before
 * sleep would fire late by the sleep's length. On the system's `resume`, main tells every goose
 * window; each window's LoopDriver asks its goosed to re-read the wall clock (`loops/wake`), and
 * the runner runs one tick if one or more came due — never a burst.
 *
 * The floating engine glance (Q-226) is not a goose window: it is a BaseWindow, which
 * `BrowserWindow.getAllWindows()` never lists, and it mounts no LoopDriver. Its webContents id is
 * refused here as well, so the glance never hears a wake whichever window list is handed in.
 */

export const SYSTEM_RESUMED_CHANNEL = 'system-resumed';

/** The BrowserWindow surface the broadcast reads — Electron-free, so it is tested as data. */
export interface ResumedWindow {
  isDestroyed(): boolean;
  webContents: { id: number; send(channel: string): void };
}

/** Sends `system-resumed` to every live goose window; answers the webContents ids it reached. */
export function broadcastSystemResumed(
  windows: readonly ResumedWindow[],
  glanceWebContentsId: number | null
): number[] {
  const reached: number[] = [];
  for (const win of windows) {
    if (win.isDestroyed() || win.webContents.id === glanceWebContentsId) continue;
    win.webContents.send(SYSTEM_RESUMED_CHANNEL);
    reached.push(win.webContents.id);
  }
  return reached;
}

type SystemResumedDeps = {
  app: Pick<App, 'whenReady'>;
  powerMonitor: { on(event: 'resume', listener: () => void): unknown };
  gooseWindows: () => readonly ResumedWindow[];
  glanceWebContentsId: () => number | null;
};

/** `powerMonitor` cannot be used before the app is ready, so the listener goes on at ready. */
export function registerSystemResumed(deps: SystemResumedDeps): Promise<void> {
  return deps.app.whenReady().then(() => {
    deps.powerMonitor.on('resume', () => {
      broadcastSystemResumed(deps.gooseWindows(), deps.glanceWebContentsId());
    });
  });
}
