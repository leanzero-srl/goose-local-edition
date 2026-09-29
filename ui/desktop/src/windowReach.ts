/**
 * Sending to a goose window from main, and what main does with an error nobody caught (Q-490).
 *
 * `BrowserWindow.isDestroyed()` is NOT "its renderer can still be sent to". Measured on Electron
 * 41.0.0, the version the app ships: inside a closing window's webContents `destroyed` event the
 * window is still listed by `BrowserWindow.getAllWindows()`, answers `isDestroyed() === false`, and
 * its `webContents.send` throws `TypeError: Object has been destroyed`. main redraws the engine
 * glance from those very `destroyed` handlers (a window's session report leaves with it), and the
 * glance was broadcast with `if (!win.isDestroyed()) win.webContents.send(...)` — so closing a
 * window whose chat was mid-turn (its report changed the glance) threw from the broadcast. The
 * uncaught-exception handler then broadcast `fatal-error` the same way, threw again inside the
 * handler, and Node ended the process with exit code 7 ("the fatal exception handler itself threw"):
 * no main.log line (the handler wrote to console, which the packaged app does not keep), no crash
 * report, goosed saw its stdin close and tore down, and the engine went with it.
 *
 * So every send to a window goes through `canReach`, and the fatal-error report never throws.
 */

export interface ReachableWindow {
  isDestroyed(): boolean;
  webContents: {
    isDestroyed(): boolean;
    send(channel: string, ...args: unknown[]): void;
  };
}

/** The window AND its renderer are alive: a send reaches it instead of throwing. */
export function canReach(win: ReachableWindow): boolean {
  return !win.isDestroyed() && !win.webContents.isDestroyed();
}

/** Sends to one window when it can be reached; answers whether it was sent. */
export function sendToWindow(win: ReachableWindow, channel: string, ...args: unknown[]): boolean {
  if (!canReach(win)) return false;
  win.webContents.send(channel, ...args);
  return true;
}

/** Sends to every window that can be reached; answers how many were sent to. */
export function broadcastToWindows(
  windows: Iterable<ReachableWindow>,
  channel: string,
  ...args: unknown[]
): number {
  let sent = 0;
  for (const win of windows) {
    if (sendToWindow(win, channel, ...args)) sent += 1;
  }
  return sent;
}

export type FatalOrigin = 'uncaughtException' | 'unhandledRejection';

export interface FatalErrorDeps {
  windows: () => Iterable<ReachableWindow>;
  /** main.log's error line — the file transport writes synchronously, so it lands even at exit. */
  logError: (message: string, detail: string) => void;
  formatError: (error: unknown) => string;
}

/**
 * main's handler for an error nobody caught. It must NEVER throw: a throw from inside Node's
 * `uncaughtException` handler ends the process with exit code 7 and nothing logged. The error goes
 * to main.log first, then to every window that can still show it; a window that cannot is skipped,
 * and a send that throws anyway is logged, never rethrown.
 */
export function reportFatalError(origin: FatalOrigin, error: unknown, deps: FatalErrorDeps): void {
  const message =
    (error instanceof Error ? error.message : String(error)) || 'An unexpected error occurred';
  try {
    deps.logError(`[main] ${origin}: ${message}`, deps.formatError(error));
  } catch {
    // The log itself failed; nothing is left to tell, and throwing here would end the app.
  }
  let windows: ReachableWindow[];
  try {
    windows = [...deps.windows()];
  } catch (listError) {
    safeLog(deps, '[main] could not list the windows to show the error in', listError);
    return;
  }
  for (const win of windows) {
    try {
      sendToWindow(win, 'fatal-error', message);
    } catch (sendError) {
      safeLog(deps, '[main] could not show the error in a window', sendError);
    }
  }
}

function safeLog(deps: FatalErrorDeps, message: string, error: unknown): void {
  try {
    deps.logError(message, deps.formatError(error));
  } catch {
    // As above: a failed log line must not become an exit.
  }
}
