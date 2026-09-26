import { useSyncExternalStore } from 'react';
import type { FailedSessionDto, NeedsYouItemDto, RunningSessionDto } from '@aaif/goose-sdk';
import {
  pendingAcpElicitations,
  subscribePendingAcpElicitations,
  type AcpElicitationRequest,
} from '../../acp/elicitationRequests';
import { acpResolveNeedsYou, acpSessionActivity } from '../../acp/needsYou';
import { AppEvents } from '../../constants/events';

/**
 * THE one source of what every session is doing: running / needs you / failed / idle.
 *
 * `running` is the engine's busy set (a turn holding a cancel token — streaming, waiting on the
 * model, or running tools), `needsYou` the engine's open `ask_user` items, `failed` the sessions
 * whose last turn the engine recorded as failed, and `elicitations` the live MCP elicitations a
 * tool call is waiting on in this window. Every session list, the pinned card and the top bar read
 * THIS store, so no two surfaces can disagree about a session.
 *
 * Refreshed when the engine says a run started or ended (the `activeRunId` session update), when a
 * turn finishes, on focus, and on a steady poll for turns started by something other than this
 * window (a schedule, a linked Mac).
 */
export interface SessionActivitySnapshot {
  running: RunningSessionDto[];
  needsYou: NeedsYouItemDto[];
  /** Sessions whose LAST turn failed (a later completed turn clears it). */
  failed: FailedSessionDto[];
  elicitations: AcpElicitationRequest[];
}

export type { FailedSessionDto, NeedsYouItemDto, RunningSessionDto };

/** The poll only catches turns this window did not start; this window's own starts arrive at once. */
export const ACTIVITY_POLL_MS = 5000;

const EMPTY: SessionActivitySnapshot = {
  running: [],
  needsYou: [],
  failed: [],
  elicitations: [],
};

let snapshot: SessionActivitySnapshot = EMPTY;
let refreshGeneration = 0;
let started = false;
let pollTimer: ReturnType<typeof setInterval> | undefined;
let unsubscribeElicitations: (() => void) | undefined;
const listeners = new Set<() => void>();

function emit(next: SessionActivitySnapshot): void {
  snapshot = next;
  for (const listener of listeners) {
    listener();
  }
}

export function getSessionActivitySnapshot(): SessionActivitySnapshot {
  return snapshot;
}

export async function refreshSessionActivity(): Promise<void> {
  const generation = ++refreshGeneration;
  try {
    const { running, needsYou, failed } = await acpSessionActivity();
    if (generation !== refreshGeneration) return;
    // The poll re-reads every few seconds; an unchanged answer must not re-render every list.
    if (
      JSON.stringify(running) === JSON.stringify(snapshot.running) &&
      JSON.stringify(needsYou) === JSON.stringify(snapshot.needsYou) &&
      JSON.stringify(failed) === JSON.stringify(snapshot.failed)
    ) {
      return;
    }
    emit({ ...snapshot, running, needsYou, failed });
  } catch (error) {
    console.warn('Failed to read session activity:', error);
  }
}

/** Close an item on the engine, drop it here at once, then re-read the engine's view. */
export async function resolveNeedsYou(
  item: Pick<NeedsYouItemDto, 'id' | 'sessionId'>,
  action: 'answer' | 'dismiss',
  answer?: string
): Promise<void> {
  await acpResolveNeedsYou(item.sessionId, item.id, action, answer);
  refreshGeneration += 1;
  emit({ ...snapshot, needsYou: snapshot.needsYou.filter((open) => open.id !== item.id) });
  void refreshSessionActivity();
}

function syncElicitations(): void {
  emit({ ...snapshot, elicitations: pendingAcpElicitations() });
}

const refreshOnEvent = () => void refreshSessionActivity();

const WINDOW_EVENTS = [
  AppEvents.SESSION_ACTIVITY_CHANGED,
  AppEvents.MESSAGE_STREAM_FINISHED,
  AppEvents.SESSION_DELETED,
  'focus',
] as const;

/** Idempotent: the first subscriber starts the sync for the lifetime of the window. */
export function startSessionActivitySync(): void {
  if (started) return;
  started = true;
  unsubscribeElicitations = subscribePendingAcpElicitations(syncElicitations);
  for (const name of WINDOW_EVENTS) {
    window.addEventListener(name, refreshOnEvent);
  }
  pollTimer = setInterval(refreshOnEvent, ACTIVITY_POLL_MS);
  syncElicitations();
  void refreshSessionActivity();
}

export function resetSessionActivityForTests(next: SessionActivitySnapshot = EMPTY): void {
  if (started) {
    for (const name of WINDOW_EVENTS) {
      window.removeEventListener(name, refreshOnEvent);
    }
    clearInterval(pollTimer);
    unsubscribeElicitations?.();
  }
  started = false;
  refreshGeneration = 0;
  snapshot = next;
  for (const listener of listeners) {
    listener();
  }
}

/** Tests seed the store without starting the engine sync. */
export function seedSessionActivityForTests(next: Partial<SessionActivitySnapshot>): void {
  started = true;
  emit({ ...EMPTY, ...next });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  startSessionActivitySync();
  return () => {
    listeners.delete(listener);
  };
}

export function useSessionActivity(): SessionActivitySnapshot {
  return useSyncExternalStore(subscribe, getSessionActivitySnapshot);
}

export interface SessionActivity {
  /** When the turn in flight began; undefined = no turn running. */
  runningSince?: string;
  /** Open questions plus live elicitations. */
  needsYou: number;
  /** When the last turn failed; undefined = it did not. */
  failedAt?: string;
  failedReason?: string;
}

export function activityOf(state: SessionActivitySnapshot, sessionId: string): SessionActivity {
  const running = state.running.find((row) => row.sessionId === sessionId);
  const failed = state.failed.find((row) => row.sessionId === sessionId);
  const needsYou =
    state.needsYou.filter((item) => item.sessionId === sessionId).length +
    state.elicitations.filter((request) => request.sessionId === sessionId).length;
  return {
    runningSince: running?.startedAt,
    needsYou,
    failedAt: failed?.failedAt,
    failedReason: failed?.reason ?? undefined,
  };
}

export type SessionState = 'running' | 'needs-you' | 'failed' | 'idle';

/**
 * Every state that holds, most urgent first. A new turn on a session whose last turn failed is
 * RUNNING (the failure is history once a turn starts); needs-you and running can hold together.
 */
export function sessionStates(activity: SessionActivity): SessionState[] {
  const states: SessionState[] = [];
  if (activity.needsYou > 0) states.push('needs-you');
  if (activity.runningSince) states.push('running');
  if (states.length === 0 && activity.failedAt) states.push('failed');
  return states.length > 0 ? states : ['idle'];
}

/** A session leads its list when it runs or waits on the person. */
export function isActive(activity: SessionActivity): boolean {
  return activity.needsYou > 0 || activity.runningSince !== undefined;
}

/** Selector hooks: a row re-renders only when ITS session's answer changes. */
export function useActivityOf(sessionId: string): SessionActivity {
  const runningSince = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).runningSince
  );
  const needsYou = useSyncExternalStore(subscribe, () => activityOf(snapshot, sessionId).needsYou);
  const failedAt = useSyncExternalStore(subscribe, () => activityOf(snapshot, sessionId).failedAt);
  const failedReason = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).failedReason
  );
  return { runningSince, needsYou, failedAt, failedReason };
}

/** A live MCP elicitation is shown pinned above the composer, so its inline copy steps aside. */
export function useElicitationIsPinned(elicitationId: string | undefined): boolean {
  return useSyncExternalStore(
    subscribe,
    () =>
      elicitationId !== undefined &&
      snapshot.elicitations.some((request) => request.id === elicitationId)
  );
}

/** One row per active session for the "Active now" group and the top bar: waiting first, oldest first. */
export interface ActiveSession {
  sessionId: string;
  sessionName: string;
  workingDir: string;
  runningSince?: string;
  needsYou: number;
  /** The first open question, or a live elicitation's message. */
  headline?: string;
}

export function activeSessions(state: SessionActivitySnapshot): ActiveSession[] {
  const rows = new Map<string, ActiveSession>();
  const row = (sessionId: string, sessionName: string, workingDir: string) => {
    let existing = rows.get(sessionId);
    if (!existing) {
      existing = { sessionId, sessionName, workingDir, needsYou: 0 };
      rows.set(sessionId, existing);
    }
    if (!existing.sessionName && sessionName) existing.sessionName = sessionName;
    if (!existing.workingDir && workingDir) existing.workingDir = workingDir;
    return existing;
  };
  for (const item of state.needsYou) {
    const r = row(item.sessionId, item.sessionName, item.workingDir);
    r.needsYou += 1;
    r.headline ??= item.question;
  }
  for (const request of state.elicitations) {
    const r = row(request.sessionId, '', '');
    r.needsYou += 1;
    r.headline ??= request.request.message;
  }
  for (const running of state.running) {
    row(running.sessionId, running.sessionName, running.workingDir).runningSince =
      running.startedAt;
  }
  return [...rows.values()].sort((a, b) => {
    if ((a.needsYou > 0) !== (b.needsYou > 0)) return a.needsYou > 0 ? -1 : 1;
    return (a.runningSince ?? '').localeCompare(b.runningSince ?? '');
  });
}

export function sessionHref(sessionId: string): string {
  return `/pair?resumeSessionId=${encodeURIComponent(sessionId)}`;
}

/** For surfaces that may render outside the router (the engine tile): the HashRouter follows the hash. */
export function openSessionFromAnywhere(sessionId: string): void {
  const target = `#${sessionHref(sessionId)}`;
  if (window.location.hash !== target) {
    window.location.hash = target;
  }
}

/** The chat message that carries an answer to the model: its question, then the person's words. */
export function answerMessage(question: string, answer: string): string {
  return `Answer to your question "${question.trim()}": ${answer.trim()}`;
}

/** "27m", "1h 05m", "40s" — the live elapsed of a running turn. */
export function elapsedLabel(sinceIso: string, now: number): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(sinceIso)) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${String(minutes % 60).padStart(2, '0')}m`;
}

/**
 * Same-title sessions in one list get " · 2", " · 3" by age (the oldest keeps the bare name), so two
 * "Jira Migration Kickoff Notes" rows can be told apart at a glance.
 */
export function disambiguatedNames<T extends { id: string; createdAt?: string }>(
  sessions: readonly T[],
  nameOf: (session: T) => string
): Map<string, string> {
  const byName = new Map<string, T[]>();
  for (const session of sessions) {
    const name = nameOf(session);
    const group = byName.get(name) ?? [];
    group.push(session);
    byName.set(name, group);
  }
  const out = new Map<string, string>();
  for (const [name, group] of byName) {
    if (group.length === 1) {
      out.set(group[0].id, name);
      continue;
    }
    const ordered = [...group].sort((a, b) =>
      (a.createdAt ?? '').localeCompare(b.createdAt ?? '')
    );
    ordered.forEach((session, index) =>
      out.set(session.id, index === 0 ? name : `${name} · ${index + 1}`)
    );
  }
  return out;
}
