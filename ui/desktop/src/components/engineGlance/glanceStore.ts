import { useEffect, useSyncExternalStore } from 'react';
import {
  ENGINE_GLANCE_CHANNEL,
  isGlancePush,
  type GlancePrefs,
  type GlancePush,
  type GlanceSessions,
} from '../../utils/engineGlance';
import type { GlancePipAction } from '../../engineGlanceDesktop';
import { activeSessions, useSessionActivity } from '../sessionActivity/sessionActivityStore';

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
 * Hands main this window's running / needs-you whenever they change — the ONE session-state store
 * (sessionActivityStore.ts), so the desktop window says what the sidebar and the top bar say.
 */
export function useReportGlanceSessions(): void {
  const state = useSessionActivity();
  const report = glanceSessionsOf(state);
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
}
