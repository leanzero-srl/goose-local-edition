import { leaveCause } from './leaveCause';

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
