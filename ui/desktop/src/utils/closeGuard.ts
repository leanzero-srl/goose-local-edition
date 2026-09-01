// Closing a window with the MOUSE during a live session-driven swarm run kills the run: the
// traffic-light button (or File > Close by click) closes the window, `closed` releases the window's
// goose serve lease, and the lease's cleanup signals goosed's process group — `goose swarm run` is
// goosed's child, so it dies with the window (gooseServe.ts killGroupOrProcess, by design).
//
// The accelerator guard (shortcutGuard.ts) REFUSES the key chord outright: a stray Cmd+W is never the
// user meaning it. A click is different — it may well be meant — so it is ASKED, never refused and
// never silently obeyed. Main intercepts the BrowserWindow `close` for a PROTECTED window, keeps the
// window, and hands the question to that window's renderer, which shows a custom Studio dialog
// (ConfirmCloseRunDialog — never window.confirm). "Stop run and close" replies on
// CONFIRM_CLOSE_RUN_REPLY_CHANNEL; main marks the window confirmed and calls close() again, and the
// pass-through flag lets that second `close` through. "Keep running" leaves everything as it was.
//
// PROTECTED means exactly what it means to the accelerator guard: this window's renderer holds a live
// swarm-run subscription in swarmWatchers and the heartbeat stamp main cached from that renderer's
// own read-swarm-run poll is fresh by SWARM_HEARTBEAT_STALE_MS (main.ts windowHoldsLiveRun →
// isSwarmRunStampAlive). One predicate, two consumers; this module adds no liveness rule of its own.

/** main → renderer: "your window is being closed on a live run — ask the user". */
export const CONFIRM_CLOSE_RUN_CHANNEL = 'confirm-close-run';
/** renderer → main: `true` = stop the run and close; anything else = keep running. */
export const CONFIRM_CLOSE_RUN_REPLY_CHANNEL = 'confirm-close-run-reply';

export type LiveRunRef = { runId: string; runDir: string; workingDir: string };

/** What main tells the renderer: every live run this window's renderer is watching. */
export type CloseRunPayload = { runs: LiveRunRef[] };

export type CloseVerdict = 'pass' | 'ask';

export type CloseGuardInput = {
  /** The pass-through flag for this window, already CONSUMED by the caller (ConfirmedCloses.take). */
  confirmed: boolean;
  /** THE SAME PREDICATE the accelerator guard feeds for `close` (ShortcutGuardInput.windowHoldsLiveRun). */
  windowHoldsLiveRun: boolean;
  /** The renderer can still show the dialog and answer: its webContents is neither destroyed nor crashed. */
  rendererCanAnswer: boolean;
};

/**
 * Whether this `close` goes through untouched or is turned into a question.
 *
 * FAIL OPEN, by construction: a renderer that is destroyed or has crashed can never mount the dialog
 * nor send the reply, so preventing its close would leave a window nobody can close over a run nobody
 * can see. Such a window closes exactly as an unprotected one does — and the guard's stamp decays with
 * the renderer's poll anyway (isSwarmRunStampAlive), so the run reads dead here within one window.
 */
export function decideClose({
  confirmed,
  windowHoldsLiveRun,
  rendererCanAnswer,
}: CloseGuardInput): CloseVerdict {
  if (confirmed) return 'pass';
  if (!windowHoldsLiveRun) return 'pass';
  if (!rendererCanAnswer) return 'pass';
  return 'ask';
}

/**
 * The pass-through flag, per BrowserWindow id. Set by the renderer's confirmed reply, CONSUMED by the
 * very next `close` of that window (so a later, unrelated close on a new run is asked again), and
 * forgotten when the window is gone. A flag nobody takes is harmless: it dies with the id.
 */
export class ConfirmedCloses {
  private readonly ids = new Set<number>();

  confirm(windowId: number): void {
    this.ids.add(windowId);
  }

  /** True once per confirmation — reading it clears it. */
  take(windowId: number): boolean {
    return this.ids.delete(windowId);
  }

  has(windowId: number): boolean {
    return this.ids.has(windowId);
  }

  forget(windowId: number): void {
    this.ids.delete(windowId);
  }
}
