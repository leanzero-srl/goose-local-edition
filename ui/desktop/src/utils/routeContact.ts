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
  /** How long main has read the route not answering, as of this read; null while it answers. */
  lostForMs: number | null;
  /**
   * The longest wait — a mount or a lost contact — after which this route answered again, as main
   * measured it in this app's life; null = none measured yet. A wait already past the verdict below
   * is never one of them: it measured a Mac that was gone, not a blip.
   */
  longestComebackMs: number | null;
  /** How many waits came back. */
  comebacks: number;
  /** The Mac said "quit goose" during this wait (Q-51's notice), kept until it answers again. */
  saidQuit: boolean;
}

/**
 * ratio: a wait this many times the longest this route ever came back from is no blip. Receipt:
 * Q-111 — the Studio's goose, relaunched, re-mounted the route in ~25 s with no click, so a Mac whose
 * longest comeback is that relaunch is called gone at ~75 s; the overnight quit said
 * "reconnecting…" for hours.
 */
export const GONE_PAST_LONGEST_COMEBACK = 3;

/** The wait is well past every comeback this route has been measured to make. */
export function waitedPastComebacks(waitedMs: number, longestComebackMs: number | null): boolean {
  return longestComebackMs != null && waitedMs > longestComebackMs * GONE_PAST_LONGEST_COMEBACK;
}

/**
 * The route's Mac is not a blip away: its goose SAID it quit, or it has stayed unreachable well past
 * every wait this Mac has measured it come back from. Null = "reconnecting" — including while
 * nothing has been measured yet, when no wait can honestly be called too long.
 */
export type PeerGone =
  | { because: 'said-quit' }
  | { because: 'unreachable'; lostForMs: number; longestComebackMs: number };

export function routePeerGone(
  contact: RouteContact | null,
  cause: LeaveCause | null
): PeerGone | null {
  if (cause === 'quit' || contact?.saidQuit) return { because: 'said-quit' };
  if (contact?.lostForMs == null || contact.longestComebackMs == null) return null;
  return waitedPastComebacks(contact.lostForMs, contact.longestComebackMs)
    ? {
        because: 'unreachable',
        lostForMs: contact.lostForMs,
        longestComebackMs: contact.longestComebackMs,
      }
    : null;
}
