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
//
// STILL EVERY WINDOW, with one goosed shared by all of them (Q-257). The run does not live with
// goosed but with the window's own ACP connection: its close aborts the connection's task, which
// drops `on_prompt` mid-await (acp_connection_close_test.rs, Q9), and `goose swarm run` is spawned
// `kill_on_drop` (providers/swarm.rs). So closing the window whose chat runs the build kills the build
// even while other windows keep goosed alive — the question is asked whichever window it is.
//
// A CHAT TURN is the same shape (Q-490). A prompt runs on the ACP connection of the window that sent
// it; closing that window drops the prompt mid-answer (goosed: "a prompt dropped before it ended (its
// connection closed …); its run is cleared"). The guard used to see swarm runs only, so a window whose
// chat was generating closed without a word. Each renderer now reports the prompts it has in flight
// on its own connection (TURNS_IN_FLIGHT_CHANNEL, from acpChatSessionStore's active prompt attempts),
// and a window holding one is asked exactly like a window holding a run.

/** main → renderer: "your window is being closed on a live run — ask the user". */
export const CONFIRM_CLOSE_RUN_CHANNEL = 'confirm-close-run';
/** renderer → main: `true` = stop the run and close; anything else = keep running. */
export const CONFIRM_CLOSE_RUN_REPLY_CHANNEL = 'confirm-close-run-reply';
/** renderer → main: the prompts this window has in flight on its own ACP connection, on every change. */
export const TURNS_IN_FLIGHT_CHANNEL = 'turns-in-flight';

export type LiveRunRef = { runId: string; runDir: string; workingDir: string };

/** A chat whose prompt is in flight on this window's connection. `sessionName` is null until named. */
export type TurnInFlight = { sessionId: string; sessionName: string | null };

/** What main tells the renderer: every live run this window's renderer is watching, and every
 *  prompt it has in flight. */
export type CloseRunPayload = { runs: LiveRunRef[]; turns: TurnInFlight[] };

export function isTurnsInFlight(value: unknown): value is TurnInFlight[] {
  return (
    Array.isArray(value) &&
    value.every((t: unknown) => {
      if (t == null || typeof t !== 'object') return false;
      const turn = t as Record<string, unknown>;
      return (
        typeof turn.sessionId === 'string' &&
        (turn.sessionName === null || typeof turn.sessionName === 'string')
      );
    })
  );
}

export type CloseVerdict = 'pass' | 'ask';

export type CloseGuardInput = {
  /** The pass-through flag for this window, already CONSUMED by the caller (ConfirmedCloses.take). */
  confirmed: boolean;
  /** THE SAME PREDICATE the accelerator guard feeds for `close` (ShortcutGuardInput.windowHoldsLiveRun). */
  windowHoldsLiveRun: boolean;
  /** This window's renderer reported a prompt in flight on its own ACP connection (Q-490). */
  windowHoldsLiveTurn: boolean;
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
  windowHoldsLiveTurn,
  rendererCanAnswer,
}: CloseGuardInput): CloseVerdict {
  if (confirmed) return 'pass';
  if (!windowHoldsLiveRun && !windowHoldsLiveTurn) return 'pass';
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
