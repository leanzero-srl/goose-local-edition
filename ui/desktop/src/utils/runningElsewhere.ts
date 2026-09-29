import type { RunningSessionDto } from '@aaif/goose-sdk';
import type { TurnInFlight } from './closeGuard';

/**
 * A chat's turn is RUNNING whichever window looks at it (Q-500).
 *
 * One goosed serves every window (Q-257), but every window talks to it over its OWN ACP connection,
 * and goosed keeps a busy set per connection (acp/server/needs_you.rs `busy_sessions` reads the
 * connection's AgentManager and the process-wide one). A turn started in window A is therefore
 * absent from window B's `session/activity` read: B showed that chat idle — no Running pill, no
 * Stop, missing from Active now — while the engine glance in B named it, 17 minutes into a 200K
 * prompt. goosed's cancel is per connection too (`on_cancel` looks only in the connection's own
 * `active_prompt_runs`), so B cannot stop it through its own connection either.
 *
 * main holds every window's report — its goosed `running` rows (the glance sessions report) and
 * the prompts it has in flight on its connection (TURNS_IN_FLIGHT_CHANNEL, Q-490) — and hands each
 * window the rows the OTHER windows' connections run, each with the window that holds its prompt.
 * The renderer's session-activity store joins them to its own read (`runningRowsOf`), so every
 * surface that reads a chat's running state reads the same union in every window.
 */

/** main → renderer: the turns other windows' connections run (`RunningElsewhere[]`). */
export const RUNNING_ELSEWHERE_CHANNEL = 'session-running-elsewhere';
/** renderer → main: bring forward the window whose connection holds this chat's turn. */
export const SHOW_TURN_WINDOW_CHANNEL = 'show-turn-window';
/** renderer → main: stop this chat's turn in the window whose connection holds it. */
export const STOP_TURN_ELSEWHERE_CHANNEL = 'stop-turn-elsewhere';
/** main → the holding renderer: stop your prompt for this chat (another window asked). */
export const STOP_TURN_CHANNEL = 'stop-turn-for-another-window';

export interface RunningElsewhere extends RunningSessionDto {
  /**
   * The window (its webContents id) whose connection holds this turn's prompt; null = no other
   * window holds one — goosed's process-wide agents run it (a loop tick, a schedule).
   */
  window: number | null;
}

/** The window other than `self` whose connection has a prompt in flight for `sessionId`. */
export function turnHolderOf(
  sessionId: string,
  turns: ReadonlyMap<number, readonly TurnInFlight[]>,
  self: number
): number | null {
  for (const [window, held] of turns) {
    if (window !== self && held.some((t) => t.sessionId === sessionId)) return window;
  }
  return null;
}

/**
 * What window `self` must add to its own read: every other window's goosed rows, once per chat (the
 * earliest start — the turn began when the first connection saw it), with the window holding it.
 */
export function runningElsewhereFor(
  self: number,
  running: ReadonlyMap<number, readonly RunningSessionDto[]>,
  turns: ReadonlyMap<number, readonly TurnInFlight[]>
): RunningElsewhere[] {
  const rows = new Map<string, RunningSessionDto>();
  for (const [window, list] of running) {
    if (window === self) continue;
    for (const row of list) {
      const seen = rows.get(row.sessionId);
      if (!seen || row.startedAt < seen.startedAt) rows.set(row.sessionId, row);
    }
  }
  return [...rows.values()]
    .sort((a, b) => a.startedAt.localeCompare(b.startedAt))
    .map((row) => ({ ...row, window: turnHolderOf(row.sessionId, turns, self) }));
}

/**
 * THE join every running claim reads (Q-500): this window's own goosed read plus the other windows'
 * rows, once per chat, the earliest start kept.
 */
export function joinRunningRows(
  own: readonly RunningSessionDto[],
  elsewhere: readonly RunningElsewhere[]
): RunningSessionDto[] {
  const rows = new Map<string, RunningSessionDto>();
  for (const row of [...own, ...elsewhere]) {
    const seen = rows.get(row.sessionId);
    if (!seen || row.startedAt < seen.startedAt) {
      rows.set(row.sessionId, {
        sessionId: row.sessionId,
        sessionName: row.sessionName || seen?.sessionName || '',
        workingDir: row.workingDir || seen?.workingDir || '',
        startedAt: row.startedAt,
      });
    }
  }
  return [...rows.values()];
}

export function isRunningRow(value: unknown): value is RunningSessionDto {
  if (value == null || typeof value !== 'object') return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.sessionId === 'string' &&
    typeof r.sessionName === 'string' &&
    typeof r.workingDir === 'string' &&
    typeof r.startedAt === 'string'
  );
}

export function isRunningElsewhereList(value: unknown): value is RunningElsewhere[] {
  return (
    Array.isArray(value) &&
    value.every(
      (r) =>
        isRunningRow(r) &&
        ((r as RunningElsewhere).window === null ||
          typeof (r as RunningElsewhere).window === 'number')
    )
  );
}
