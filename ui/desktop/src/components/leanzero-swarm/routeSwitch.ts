import { mlxEngineUnmount } from '../../acp/mlx-engine';
import {
  latestMlxRemoteSingleReadError,
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStop,
} from '../../acp/mlx-remote-single';
import { routePeerName } from './macs';

/**
 * Moving chat OFF a route — the ONE path every switch and Stop takes: the composer's "Run on this
 * Mac instead", Run it's switch (PlacementCard `stopForSwitch`) and Stop (`stop`), the Engine
 * view's Stop serving (`onStopRemote`) and the menu-bar tray's Stop (useMlxTrayActions
 * `stop-remote`, run in the renderer on main's behalf).
 *
 * The route is a record on THIS Mac: withdrawing it never needs the linked Mac. Freeing that Mac's
 * engine does, and a switch must never wait on — or fail because of — a Mac that is not answering
 * (the recovery recordings: the Studio gone for ~10 s; the old switch either hung on the peer's
 * unmount or ended "switch failed" with the route already gone; a Stop spun on it). So:
 *  - reachable: the route's Stop as before (withdraw + unmount there, answered in one call);
 *  - not answering (route `reconnecting`, or the last route read failed): withdraw only, then ask
 *    that Mac to free its engine in the background;
 *  - gone (its goose quit, or away well past every comeback): withdraw only — nothing to free.
 * Either way a peer that keeps its model is a quiet fact (`PeerHeld`), never the switch's failure.
 * The one caller that waits for `settled` is a switch to the split: the split runs on that Mac
 * too, and its preflight must find the route's engine — loading or running — already gone (Q-112).
 * Throws only when the route itself could not be withdrawn (another window owns it).
 */

export type PeerHeld =
  | { phase: 'asking'; peerNodeId: string; peerName: string }
  | { phase: 'held'; peerNodeId: string; peerName: string; detail: string };

let held: PeerHeld | null = null;
const listeners = new Set<() => void>();

function publish(next: PeerHeld | null): void {
  held = next;
  for (const listener of listeners) listener();
}

export function latestPeerHeld(): PeerHeld | null {
  return held;
}

export function subscribePeerHeld(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function dismissPeerHeld(): void {
  publish(null);
}

/** The route's Mac is not answering right now, as this window last read it. */
export function routeUnreachable(): boolean {
  return (
    latestMlxRemoteSingleStatus()?.state === 'reconnecting' ||
    latestMlxRemoteSingleReadError() != null
  );
}

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

export interface RouteDrop {
  /** Resolves once the route is withdrawn on this Mac; rejects only if it could not be. */
  routeGone: Promise<void>;
  /** Resolves once that Mac answered the request to free its engine (or never needed asking). */
  settled: Promise<void>;
}

/**
 * How the route's Mac stands: `answers`; `unreachable` — not answering now, asked to free its
 * engine in the background; `gone` — its goose quit or has stayed away well past every comeback
 * (routeContact.ts `routePeerGone`): its engine went with the app (Q-34), so it is not asked at all
 * and no "still holds the model" line is raised about a Mac that holds nothing (Q-111).
 */
export type PeerReach = 'answers' | 'unreachable' | 'gone';

export function dropRoute(
  peer: PeerReach = routeUnreachable() ? 'unreachable' : 'answers'
): RouteDrop {
  const route = latestMlxRemoteSingleStatus();
  const peerNodeId = route?.peer ?? null;
  const peerName = route ? routePeerName(route) : '';
  if (peer === 'gone') {
    const routeGone = mlxRemoteSingleStop(true).then(() => publish(null));
    return { routeGone, settled: routeGone.catch(() => undefined) };
  }
  if (peer === 'answers') {
    const routeGone = mlxRemoteSingleStop(false).then(({ unmountError }) => {
      publish(
        unmountError && peerNodeId
          ? { phase: 'held', peerNodeId, peerName, detail: unmountError }
          : null
      );
    });
    return { routeGone, settled: routeGone.catch(() => undefined) };
  }
  const routeGone = mlxRemoteSingleStop(true).then(() => undefined);
  const settled = routeGone.then(
    async () => {
      if (!peerNodeId) {
        publish(null);
        return;
      }
      publish({ phase: 'asking', peerNodeId, peerName });
      try {
        await mlxEngineUnmount(peerNodeId);
        if (held?.peerNodeId === peerNodeId) publish(null);
      } catch (e) {
        if (held?.peerNodeId === peerNodeId) {
          publish({ phase: 'held', peerNodeId, peerName, detail: messageOf(e) });
        }
      }
    },
    () => undefined
  );
  return { routeGone, settled };
}
