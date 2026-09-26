import { leaveCause, type LeaveCause } from './leaveCause';

/**
 * Has contact with the Mac a route serves chat from been LOST — the one rule the composer bar
 * (chatServedBy `lostContactWith`) and the tray (mlxTray) both read.
 *
 * ONE source decides "back": main's own read of that Mac's engine through the relay. The route's
 * `reconnecting` is read by goosed from the Link registry's Offline mark FIRST (observe_route: no
 * dial), and that mark follows the registry's own poll — it can be set, or still be set, seconds
 * after the relay answers again (Q-64, 3.0.39: the bar cleared at 182.2 s on main's read, came back
 * at 184.4 s on the route's mark while main read `running`, cleared at 188.7 s). So:
 *  - main read the route and it answered (`running`): back — except when the Mac itself said it is
 *    leaving (Link's leave words in the route's reason, fdc737969): its own word, never stale;
 *  - main's read failed (`reconnecting`): lost — a genuine second drop shows the moment main's
 *    next read fails;
 *  - main has no read of the route (another engine read, none yet, no bridge): the route's word and
 *    the renderer's read of it decide, as before.
 * Returns the failed read's words (null when none given), or null while contact is not lost.
 */
export interface RouteWord {
  state: string;
  lastError?: string | null;
}

export interface MainRead {
  engine: string;
  mode: string;
  statusDetail: string | null;
}

export function routeContactLost(
  route: RouteWord,
  routeReadError: string | null,
  main: MainRead | null
): { why: string | null } | null {
  const routeSays = route.state === 'reconnecting';
  if (main?.engine === 'remote') {
    if (main.mode === 'reconnecting') return { why: main.statusDetail };
    if (main.mode === 'running') {
      return routeSays && leaveCause(route.lastError) ? { why: route.lastError ?? null } : null;
    }
  }
  if (routeSays) return { why: route.lastError ?? null };
  if (routeReadError != null) return { why: routeReadError };
  return null;
}

/**
 * What main has MEASURED about the route's contact with its Mac (utils/mlxEngineMonitor.ts): how
 * long the current wait has lasted, the longest wait this route ever came back from, and whether
 * the Mac said it quit goose during this wait. It rides main's snapshot of the route, so the tray
 * and every chat surface read the same numbers.
 */
export interface RouteContact {
  /** When main first read the route not answering in this wait (its clock); null while it answers. */
  lostSinceMs: number | null;
  /** How long main has read the route not answering, as of this read; null while it answers. */
  lostForMs: number | null;
  /**
   * The longest wait — a mount or a lost contact — after which this route answered again, as main
   * measured it in this app's life; null = none measured yet. A wait already called away below is
   * never one of them: it measured a Mac that was away, not a blip.
   */
  longestComebackMs: number | null;
  /** How many waits came back. */
  comebacks: number;
  /** The Mac said "quit goose" during this wait (Q-51's notice), kept until it answers again. */
  saidQuit: boolean;
  /** The cadence main reads the route at — present from the first read, before any comeback. */
  pollMs: number;
}

/**
 * ratio: a wait this many times the comeback this route is expected to make is no blip. Receipt:
 * Q-111 — the Studio's goose, relaunched, re-mounted the route in ~25 s with no click, so a route
 * whose longest comeback is that relaunch is called away at ~75 s; the overnight quit said
 * "reconnecting…" for hours.
 */
export const GONE_PAST_LONGEST_COMEBACK = 3; // ratio: of the expected comeback

/**
 * measured: Q-111 — the relaunch re-mounted the route in ~25 s (08:28:58 → 08:29:23) against the
 * 2 s status poll main reads it at: 12.5 polls. A relaunch is the longest a Mac takes to come back
 * on its own, so it is the comeback expected of a route before one has been measured — and the
 * floor under a measured one (a route that only ever blipped for 2 s still takes a relaunch to
 * come back from a restart that sent no notice).
 */
export const RELAUNCH_IN_POLLS = 12.5; // measured: Q-111 relaunch, 25 s / 2 s poll

/** The comeback expected of this route: the longest measured, never under a relaunch's polls. */
export function expectedComebackMs(
  contact: Pick<RouteContact, 'longestComebackMs' | 'pollMs'>
): number {
  return Math.max(contact.longestComebackMs ?? 0, RELAUNCH_IN_POLLS * contact.pollMs);
}

/** The wait is well past the comeback this route is expected to make. */
export function waitedPastComebacks(
  waitedMs: number,
  contact: Pick<RouteContact, 'longestComebackMs' | 'pollMs'>
): boolean {
  return waitedMs > expectedComebackMs(contact) * GONE_PAST_LONGEST_COMEBACK;
}

/**
 * The route's Mac is not a blip away. Two facts, said differently because they ARE different:
 *  - `said-quit`: its goose said it quit — "its goose isn't running";
 *  - `silent`: no word from it well past the comeback it is expected to make — "hasn't answered
 *    since <time>": its goose may be closed, or the Mac is offline; a network outage is never
 *    reported as a closed app.
 * Null = "reconnecting", a blip.
 */
export type PeerGone =
  | { because: 'said-quit' }
  | { because: 'silent'; lostSinceMs: number; lostForMs: number };

export function routePeerGone(
  contact: RouteContact | null,
  cause: LeaveCause | null
): PeerGone | null {
  if (cause === 'quit' || contact?.saidQuit) return { because: 'said-quit' };
  if (contact?.lostForMs == null || contact.lostSinceMs == null) return null;
  return waitedPastComebacks(contact.lostForMs, contact)
    ? { because: 'silent', lostSinceMs: contact.lostSinceMs, lostForMs: contact.lostForMs }
    : null;
}
