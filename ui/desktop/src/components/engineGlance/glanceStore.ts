import { useEffect, useSyncExternalStore } from 'react';
import {
  ENGINE_GLANCE_CHANNEL,
  isGlancePush,
  type GlanceNodeRef,
  type GlancePrefs,
  type GlancePush,
  type GlanceServingReport,
  type GlanceSessions,
} from '../../utils/engineGlance';
import type { GlancePipAction } from '../../engineGlanceDesktop';
import { activeSessions, useSessionActivity } from '../sessionActivity/sessionActivityStore';
import {
  nodesRead,
  nodesResidency,
  nodesServedLast,
  type NodesRead,
  type Residency,
} from '../../acp/nodes';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';

/**
 * main's engine glance in this window — one subscription however many surfaces read it (the docked
 * card and the desktop window's root). null until main has built one.
 */

interface GlanceBridge {
  engineGlanceRead?: () => Promise<GlancePush | null>;
  engineGlanceSessions?: (report: GlanceSessions) => void;
  engineGlancePip?: (action: GlancePipAction) => void;
  engineGlancePrefsSet?: (prefs: GlancePrefs) => Promise<void>;
  on?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
  off?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
}

function bridge(): GlanceBridge | undefined {
  return (window as unknown as { electron?: GlanceBridge }).electron;
}

let latest: GlancePush | null = null;
const listeners = new Set<() => void>();
let unsubscribe: (() => void) | null = null;

function emit(next: GlancePush): void {
  latest = next;
  listeners.forEach((l) => l());
  const key = nodesKeyOf(next);
  if (key !== nodesKey) {
    nodesKey = key;
    refreshGlanceNodes();
  }
}

function start(): void {
  if (unsubscribe) return;
  const electron = bridge();
  const onPush = (_event: unknown, ...args: unknown[]) => {
    if (isGlancePush(args[0])) emit(args[0]);
  };
  electron?.on?.(ENGINE_GLANCE_CHANNEL, onPush);
  unsubscribe = () => electron?.off?.(ENGINE_GLANCE_CHANNEL, onPush);
  electron
    ?.engineGlanceRead?.()
    .then((push) => {
      // A push that landed while the read was in flight is newer: it stands.
      if (latest == null && isGlancePush(push)) emit(push);
    })
    .catch(() => undefined);
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  start();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
  };
}

export function useEngineGlance(): GlancePush | null {
  return useSyncExternalStore(subscribe, () => latest);
}

export function glancePipAction(action: GlancePipAction): void {
  bridge()?.engineGlancePip?.(action);
}

export async function setGlancePrefs(prefs: GlancePrefs): Promise<void> {
  await bridge()?.engineGlancePrefsSet?.(prefs);
}

/** The window's session-state store as the glance reports it: running, and every open question. */
export function glanceSessionsOf(state: Parameters<typeof activeSessions>[0]): GlanceSessions {
  return {
    running: state.running.length,
    needsYou: activeSessions(state)
      .filter((s) => s.needsYou > 0)
      .map((s) => ({
        sessionId: s.sessionId,
        sessionName: s.sessionName,
        question: s.headline ?? '',
      })),
  };
}

/**
 * THE NODES AT A GLANCE — goosed's `nodes/read` + `nodes/residency` (design §4.2), read in this
 * window when its glance changes way, model, stage or served chat (an event: `emit` above) and
 * whenever a surface mounts, never on a clock. The glance's node line (through the sessions report
 * below), the Nodes nav chip's neighbours and My Macs' "Nodes on this Mac" all read THIS, so one
 * window never names two different nodes for one way. `refreshGlanceNodes()` re-reads it after a
 * node is saved, renamed or removed.
 */
export type GlanceNodesState =
  | { kind: 'unread' }
  | { kind: 'read'; read: NodesRead; residency: Residency; servedNode: string | null }
  | { kind: 'failed'; error: string };

let nodesState: GlanceNodesState = { kind: 'unread' };
const nodesListeners = new Set<() => void>();
let nodesKey: string | null = null;
let nodesSeq = 0;

/** What a node read depends on: the way, its model, its stage (a turn begins/ends) and the chat. */
function nodesKeyOf(push: GlancePush): string {
  const e = push.engine;
  return JSON.stringify([e.present, e.engine, e.modelId, e.stage, e.chat?.sessionId ?? null]);
}

async function readNodes(chatSessionId: string | null): Promise<GlanceNodesState> {
  let read: NodesRead;
  let residency: Residency;
  try {
    [read, residency] = await Promise.all([nodesRead(), nodesResidency()]);
  } catch (e) {
    return { kind: 'failed', error: mlxErrorMessage(e, String(e)) };
  }
  // The chat's own served node only ORDERS the nodes that already name the serving way (it leads
  // when two do); an unreadable record leaves that order as the defs' and changes no answer.
  let servedNode: string | null = null;
  if (chatSessionId) {
    try {
      servedNode = (await nodesServedLast(chatSessionId)).record?.node ?? null;
    } catch {
      servedNode = null;
    }
  }
  return { kind: 'read', read, residency, servedNode };
}

export function refreshGlanceNodes(): void {
  if (nodesListeners.size === 0) return;
  const seq = ++nodesSeq;
  void readNodes(latest?.engine.chat?.sessionId ?? null).then((next) => {
    // A read that a newer one overtook says nothing about now.
    if (seq !== nodesSeq) return;
    nodesState = next;
    nodesListeners.forEach((l) => l());
  });
}

function subscribeNodes(listener: () => void): () => void {
  const first = nodesListeners.size === 0;
  nodesListeners.add(listener);
  const offGlance = subscribe(() => undefined);
  if (first) refreshGlanceNodes();
  return () => {
    nodesListeners.delete(listener);
    offGlance();
  };
}

export function useGlanceNodes(): GlanceNodesState {
  return useSyncExternalStore(subscribeNodes, () => nodesState);
}

/**
 * The report main names the glance's node from (engineGlance.ts `glanceServedBy`): the way goosed
 * says serves, and every node that names it — the chat's served node first, then nodes pinned to a
 * way, then nodes that follow this Mac's engine (they name whatever serves, so they say least). A
 * loading node counts only while the serving way itself is loading. null = nothing read yet, or
 * nothing serves.
 */
export function servingReportOf(state: GlanceNodesState): GlanceServingReport | null {
  if (state.kind === 'unread') return null;
  if (state.kind === 'failed') return { error: state.error };
  const way = state.residency.serving;
  if (!way) {
    return state.residency.servingError ? { error: state.residency.servingError } : null;
  }
  const live = new Set(
    state.residency.nodes
      .filter(
        (r) =>
          r.residency.kind === 'serving' ||
          (r.residency.kind === 'loading' && way.loadPhase != null)
      )
      .map((r) => r.node)
  );
  const rank = (def: NodesRead['nodes'][number]['def']): number =>
    def.id === state.servedNode
      ? 0
      : def.placement == null || def.placement.kind === 'follows'
        ? 2
        : 1;
  const nodes: GlanceNodeRef[] = state.read.nodes
    .map((n) => n.def)
    .filter((def) => live.has(def.id))
    .map((def, i) => ({ def, i }))
    .sort((a, b) => rank(a.def) - rank(b.def) || a.i - b.i)
    .map(({ def }) => ({ id: def.id, name: def.name }));
  return {
    way: { kind: way.kind, modelId: way.modelId, servedModelId: way.servedModelId },
    nodes,
  };
}

/**
 * Hands main this window's running / needs-you whenever they change — the ONE session-state store
 * (sessionActivityStore.ts), so the desktop window says what the sidebar and the top bar say — and
 * the node its goosed says serves (`servingReportOf`), so every surface of the glance can name it.
 */
export function useReportGlanceSessions(): void {
  const state = useSessionActivity();
  const serving = servingReportOf(useGlanceNodes());
  const report: GlanceSessions = { ...glanceSessionsOf(state), serving };
  const key = JSON.stringify(report);
  useEffect(() => {
    bridge()?.engineGlanceSessions?.(JSON.parse(key) as GlanceSessions);
  }, [key]);
}

export function resetEngineGlanceForTests(next: GlancePush | null = null): void {
  if (unsubscribe) unsubscribe();
  unsubscribe = null;
  listeners.clear();
  latest = next;
  nodesListeners.clear();
  nodesState = { kind: 'unread' };
  nodesKey = null;
  nodesSeq += 1;
}
