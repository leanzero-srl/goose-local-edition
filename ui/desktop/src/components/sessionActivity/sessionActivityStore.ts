import { useSyncExternalStore } from 'react';
import type {
  BackgroundSessionDto,
  BackgroundWorkKind,
  FailedSessionDto,
  LoopStatus,
  LoopSummaryDto,
  NeedsYouItemDto,
  NotesWaitingDto,
  RunningSessionDto,
  StoppedSessionDto,
} from '@aaif/goose-sdk';
import {
  pendingAcpElicitations,
  subscribePendingAcpElicitations,
  type AcpElicitationRequest,
} from '../../acp/elicitationRequests';
import { acpResolveNeedsYou, acpSessionActivity } from '../../acp/needsYou';
import { AppEvents } from '../../constants/events';
import {
  RUNNING_ELSEWHERE_CHANNEL,
  isRunningElsewhereList,
  joinRunningRows,
  type RunningElsewhere,
} from '../../utils/runningElsewhere';

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
 *
 * `running` is THIS window's connection's read; `elsewhere` the turns the other windows'
 * connections run (Q-500, utils/runningElsewhere.ts), pushed by main. Every running claim reads the
 * two joined (`runningRowsOf`), never `running` alone.
 */
export interface SessionActivitySnapshot {
  running: RunningSessionDto[];
  /** Turns other windows' ACP connections run, each with the window holding it (main's push). */
  elsewhere: RunningElsewhere[];
  needsYou: NeedsYouItemDto[];
  /** Sessions whose LAST turn failed (a later completed turn clears it). */
  failed: FailedSessionDto[];
  /** Sessions whose LAST turn the person stopped (a later completed turn clears it). */
  stopped: StoppedSessionDto[];
  /**
   * goose's own calls in flight FOR a session — the fact check after the reply, a title (Q-185).
   * A session shows it only while no turn runs: the quieter "still working for you" state.
   */
  background: BackgroundSessionDto[];
  /**
   * The chats whose loop has not ended (session loops, Q-228), each with its status as the engine
   * reads it now; an unreadable loop record comes with its error instead of a status.
   */
  looping: LoopSummaryDto[];
  /** Chats with notes from the person's other chats waiting there (Q-358). */
  notesWaiting: NotesWaitingDto[];
  elicitations: AcpElicitationRequest[];
}

export type {
  BackgroundSessionDto,
  BackgroundWorkKind,
  FailedSessionDto,
  LoopStatus,
  LoopSummaryDto,
  NeedsYouItemDto,
  NotesWaitingDto,
  RunningSessionDto,
  StoppedSessionDto,
};

/** The poll only catches turns this window did not start; this window's own starts arrive at once. */
export const ACTIVITY_POLL_MS = 5000;

const EMPTY: SessionActivitySnapshot = {
  running: [],
  elsewhere: [],
  needsYou: [],
  failed: [],
  stopped: [],
  background: [],
  looping: [],
  notesWaiting: [],
  elicitations: [],
};

let snapshot: SessionActivitySnapshot = EMPTY;
/**
 * Whether the engine's answer has been read at least once. Before it, `running: []` is "not asked
 * yet", never "nothing runs" — a surface that claims a turn is NOT running waits for this.
 */
let engineRead = false;
let refreshGeneration = 0;
let started = false;
let pollTimer: ReturnType<typeof setInterval> | undefined;
let unsubscribeElicitations: (() => void) | undefined;
let unsubscribeElsewhere: (() => void) | undefined;
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
    const activity = await acpSessionActivity();
    const { running, needsYou, failed } = activity;
    // An engine older than Q-169 sends no `stopped` list: it records no stopped turns.
    const stopped = activity.stopped ?? [];
    // An engine older than Q-185 lists no background work: it tagged none.
    const background = activity.background ?? [];
    // An engine older than Q-228 runs no session loops.
    const looping = activity.looping ?? [];
    // An engine older than Q-358 carries no notes between chats.
    const notesWaiting = activity.notesWaiting ?? [];
    if (generation !== refreshGeneration) return;
    const firstRead = !engineRead;
    engineRead = true;
    // The poll re-reads every few seconds; an unchanged answer must not re-render every list.
    if (
      !firstRead &&
      JSON.stringify(running) === JSON.stringify(snapshot.running) &&
      JSON.stringify(needsYou) === JSON.stringify(snapshot.needsYou) &&
      JSON.stringify(failed) === JSON.stringify(snapshot.failed) &&
      JSON.stringify(stopped) === JSON.stringify(snapshot.stopped) &&
      JSON.stringify(background) === JSON.stringify(snapshot.background) &&
      JSON.stringify(looping) === JSON.stringify(snapshot.looping) &&
      JSON.stringify(notesWaiting) === JSON.stringify(snapshot.notesWaiting)
    ) {
      return;
    }
    emit({ ...snapshot, running, needsYou, failed, stopped, background, looping, notesWaiting });
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

interface ElsewhereBridge {
  on?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
  off?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
}

/** main's push of the turns the other windows' connections run (Q-500). */
function subscribeElsewhere(): () => void {
  const electron = (window as unknown as { electron?: ElsewhereBridge }).electron;
  const onPush = (_event: unknown, ...args: unknown[]) => {
    const elsewhere = args[0];
    if (!isRunningElsewhereList(elsewhere)) return;
    if (JSON.stringify(elsewhere) === JSON.stringify(snapshot.elsewhere)) return;
    emit({ ...snapshot, elsewhere });
  };
  electron?.on?.(RUNNING_ELSEWHERE_CHANNEL, onPush);
  return () => electron?.off?.(RUNNING_ELSEWHERE_CHANNEL, onPush);
}

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
  unsubscribeElsewhere = subscribeElsewhere();
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
    unsubscribeElsewhere?.();
  }
  started = false;
  refreshGeneration = 0;
  engineRead = false;
  snapshot = next;
  for (const listener of listeners) {
    listener();
  }
}

/** Tests seed the store without starting the engine sync. */
export function seedSessionActivityForTests(next: Partial<SessionActivitySnapshot>): void {
  started = true;
  engineRead = true;
  emit({ ...EMPTY, ...next });
}

/** Every turn goosed runs, whichever window's connection runs it: THE running read (Q-500). */
export function runningRowsOf(
  state: Pick<SessionActivitySnapshot, 'running' | 'elsewhere'>
): RunningSessionDto[] {
  return state.elsewhere.length === 0
    ? state.running
    : joinRunningRows(state.running, state.elsewhere);
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
  /**
   * The other window (its webContents id) whose connection holds the running turn's prompt (Q-500);
   * undefined = no other window holds one. Only that window can stop it or stream it.
   */
  turnWindow?: number;
  /** Open questions plus live elicitations. */
  needsYou: number;
  /** When the last turn failed; undefined = it did not. */
  failedAt?: string;
  failedReason?: string;
  /** When the person stopped the last turn; undefined = they did not. */
  stoppedAt?: string;
  /** How long the stopped turn had run. */
  stoppedElapsedMs?: number;
  /** The output tokens the stopped turn had written; undefined = not counted. */
  stoppedOutputTokens?: number;
  /** goose's oldest call in flight FOR the session besides its turn (Q-185); undefined = none. */
  background?: BackgroundWorkKind;
  /** The chat's loop as read now (Q-228); undefined = no loop, or it ended. */
  loopStatus?: LoopStatus;
  /** When a waiting loop's next tick starts (RFC 3339). */
  loopNextTickAt?: string;
  /** The chat's loop record could not be read: the engine's words, never read as "no loop". */
  loopError?: string;
  /** Notes from the person's other chats waiting here (Q-358); 0 = none. */
  notesWaiting: number;
  /** The chat the oldest waiting note came from. */
  noteFrom?: string;
}

export function activityOf(state: SessionActivitySnapshot, sessionId: string): SessionActivity {
  const running = runningRowsOf(state).find((row) => row.sessionId === sessionId);
  const turnWindow = state.elsewhere.find(
    (row) => row.sessionId === sessionId && row.window !== null
  )?.window;
  const failed = state.failed.find((row) => row.sessionId === sessionId);
  const stopped = state.stopped.find((row) => row.sessionId === sessionId);
  const background = state.background.find((row) => row.sessionId === sessionId);
  const loop = state.looping.find((row) => row.sessionId === sessionId);
  const notes = state.notesWaiting.find((row) => row.sessionId === sessionId);
  // `looping` lists no ended loop; an ended one that still arrives is not looping either.
  const loopStatus = loop?.status && loop.status !== 'ended' ? loop.status : undefined;
  const needsYou =
    state.needsYou.filter((item) => item.sessionId === sessionId).length +
    state.elicitations.filter((request) => request.sessionId === sessionId).length;
  return {
    runningSince: running?.startedAt,
    turnWindow: turnWindow ?? undefined,
    needsYou,
    failedAt: failed?.failedAt,
    failedReason: failed?.reason ?? undefined,
    stoppedAt: stopped?.stoppedAt,
    stoppedElapsedMs: stopped?.elapsedMs,
    stoppedOutputTokens: stopped?.outputTokens ?? undefined,
    background: background?.kind,
    loopStatus,
    loopNextTickAt: loopStatus ? (loop?.nextTickAt ?? undefined) : undefined,
    loopError: loop?.error ?? undefined,
    notesWaiting: notes?.count ?? 0,
    noteFrom: notes?.fromName,
  };
}

export type SessionState =
  | 'running'
  | 'needs-you'
  | 'background'
  | 'looping'
  | 'failed'
  | 'stopped'
  | 'idle';

/** The order a session's states are ranked in, most urgent first (session loops §8.6). */
export const SESSION_STATE_ORDER: readonly SessionState[] = [
  'needs-you',
  'running',
  'background',
  'looping',
  'failed',
  'stopped',
  'idle',
];

/**
 * Every state that holds, most urgent first (`SESSION_STATE_ORDER`). A new turn on a session whose
 * last turn failed is RUNNING (the failure is history once a turn starts); needs-you and running
 * can hold together. BACKGROUND (Q-185) is goose still working for the session with no turn
 * running — the fact check after the reply: quieter than running, never idle. LOOPING (Q-228) is a
 * chat whose loop has not ended, between its ticks: a tick in flight is a turn and reads RUNNING,
 * its reviewers read BACKGROUND, and a tick that asked the person reads NEEDS-YOU. A loop outranks
 * the last turn's failed or stopped mark: one failed tick is on its row in the rail, and a tick
 * the person stopped pauses the loop, which the Looping pill says ("Paused").
 */
export function sessionStates(activity: SessionActivity): SessionState[] {
  const states: SessionState[] = [];
  if (activity.needsYou > 0) states.push('needs-you');
  if (activity.runningSince) states.push('running');
  else if (activity.background) states.push('background');
  else if (activity.loopStatus || activity.loopError !== undefined) states.push('looping');
  if (states.length === 0 && activity.failedAt) states.push('failed');
  if (states.length === 0 && activity.stoppedAt) states.push('stopped');
  return states.length > 0 ? states : ['idle'];
}

/** A session leads its list when it runs, waits on the person, or holds a note for them (Q-358). */
export function isActive(
  activity: Pick<SessionActivity, 'needsYou' | 'runningSince' | 'notesWaiting'>
): boolean {
  return activity.needsYou > 0 || activity.runningSince !== undefined || activity.notesWaiting > 0;
}

/** True once the engine's activity has been read: until then no session is known NOT to run. */
export function useSessionActivityRead(): boolean {
  return useSyncExternalStore(subscribe, () => engineRead);
}

/** Selector hooks: a row re-renders only when ITS session's answer changes. */
export function useActivityOf(sessionId: string): SessionActivity {
  const runningSince = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).runningSince
  );
  const turnWindow = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).turnWindow
  );
  const needsYou = useSyncExternalStore(subscribe, () => activityOf(snapshot, sessionId).needsYou);
  const failedAt = useSyncExternalStore(subscribe, () => activityOf(snapshot, sessionId).failedAt);
  const failedReason = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).failedReason
  );
  const stoppedAt = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).stoppedAt
  );
  const stoppedElapsedMs = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).stoppedElapsedMs
  );
  const stoppedOutputTokens = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).stoppedOutputTokens
  );
  const background = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).background
  );
  const loopStatus = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).loopStatus
  );
  const loopNextTickAt = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).loopNextTickAt
  );
  const loopError = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).loopError
  );
  const notesWaiting = useSyncExternalStore(
    subscribe,
    () => activityOf(snapshot, sessionId).notesWaiting
  );
  const noteFrom = useSyncExternalStore(subscribe, () => activityOf(snapshot, sessionId).noteFrom);
  return {
    runningSince,
    turnWindow,
    needsYou,
    failedAt,
    failedReason,
    stoppedAt,
    stoppedElapsedMs,
    stoppedOutputTokens,
    background,
    loopStatus,
    loopNextTickAt,
    loopError,
    notesWaiting,
    noteFrom,
  };
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
  /** The oldest open question (the one `waitingSince` dates), else a live elicitation's message. */
  headline?: string;
  /** When the oldest open question was asked (a live elicitation carries no time). */
  waitingSince?: string;
  /** Notes from the person's other chats waiting here (Q-358). */
  notesWaiting: number;
  /** The chat the oldest waiting note came from. */
  noteFrom?: string;
}

export function activeSessions(state: SessionActivitySnapshot): ActiveSession[] {
  const rows = new Map<string, ActiveSession>();
  const row = (sessionId: string, sessionName: string, workingDir: string) => {
    let existing = rows.get(sessionId);
    if (!existing) {
      existing = { sessionId, sessionName, workingDir, needsYou: 0, notesWaiting: 0 };
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
    // The headline is the question "waiting since" dates (Q-489): the top bar's menu shows the
    // two on one item, so they name the same question.
    if (
      Number.isFinite(Date.parse(item.createdAt)) &&
      (!r.waitingSince || Date.parse(item.createdAt) < Date.parse(r.waitingSince))
    ) {
      r.waitingSince = item.createdAt;
      r.headline = item.question;
    }
  }
  for (const request of state.elicitations) {
    const r = row(request.sessionId, '', '');
    r.needsYou += 1;
    r.headline ??= request.request.message;
  }
  for (const running of runningRowsOf(state)) {
    row(running.sessionId, running.sessionName, running.workingDir).runningSince =
      running.startedAt;
  }
  for (const notes of state.notesWaiting) {
    const r = row(notes.sessionId, notes.sessionName, notes.workingDir);
    r.notesWaiting = notes.count;
    r.noteFrom = notes.fromName;
  }
  return [...rows.values()].sort((a, b) => {
    if (a.needsYou > 0 !== b.needsYou > 0) return a.needsYou > 0 ? -1 : 1;
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

const normalized = (text: string) => text.trim().toLowerCase();

/** What may follow an option inside the recommendation when the rest is only its reason. */
const REASON_BREAK = /^\s*(?:[.;:,!?]|[—–]\s|\()/;

/**
 * Q-319: the "Or pick" chips are the alternatives to the recommendation. An option the
 * recommendation already IS — the same words, or the same words followed by its reason ("migrate —
 * apply the lead override in every case. The rule is satisfied…") — would repeat the recommended
 * button as the first chip, so it is left out. An option that is only a shared first word ("migrate"
 * beside "migrate + a marker") stays: the recommendation goes on with more choice, not a reason.
 */
export function pickOptions(
  item: Pick<NeedsYouItemDto, 'recommendedAnswer' | 'options'>
): string[] {
  const recommended = normalized(item.recommendedAnswer);
  return item.options.filter((option) => {
    const choice = normalized(option);
    if (choice.length === 0 || choice === recommended) return false;
    return !(recommended.startsWith(choice) && REASON_BREAK.test(recommended.slice(choice.length)));
  });
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
    const ordered = [...group].sort((a, b) => (a.createdAt ?? '').localeCompare(b.createdAt ?? ''));
    ordered.forEach((session, index) =>
      out.set(session.id, index === 0 ? name : `${name} · ${index + 1}`)
    );
  }
  return out;
}

/** One list row's name: the bare name it was grouped by and the label it shows (" · 3" added). */
export interface ListedName {
  base: string;
  label: string;
}

const listedNames = new Map<string, ListedName>();
const listedNameListeners = new Set<() => void>();
/** Bumped on every change, so a surface naming many sessions re-renders when any name changes. */
let listedNamesVersion = 0;

/**
 * The sidebar lists publish the names they show, so the chat header reads the same " · 3" as the
 * row it was opened from (Q-171: the row read "Hi. I'm starting a · 3", the header dropped the
 * " · 3"). An entry is replaced only when it changed, so an unchanged list re-renders nothing.
 */
export function publishListedNames(rows: ReadonlyArray<{ id: string } & ListedName>): void {
  let changed = false;
  for (const { id, base, label } of rows) {
    const current = listedNames.get(id);
    if (current?.base === base && current.label === label) continue;
    listedNames.set(id, { base, label });
    changed = true;
  }
  if (!changed) return;
  listedNamesVersion += 1;
  for (const listener of listedNameListeners) listener();
}

function subscribeListedNames(listener: () => void): () => void {
  listedNameListeners.add(listener);
  return () => {
    listedNameListeners.delete(listener);
  };
}

export function useListedName(sessionId: string | undefined): ListedName | undefined {
  return useSyncExternalStore(subscribeListedNames, () =>
    sessionId === undefined ? undefined : listedNames.get(sessionId)
  );
}

/** The name as the lists show it: the row's label while the row was grouped under this very name. */
export function listedTitle(name: string, listed: ListedName | undefined): string {
  return listed && listed.base === name ? listed.label : name;
}

/**
 * A session's name as the lists show it, read at the moment (Q-185): the Engine card and the cut
 * guard name "Jira Migration Kickoff Notes · 5", never the bare title two rows share.
 */
export function listedTitleOf(sessionId: string, name: string): string {
  return listedTitle(name, listedNames.get(sessionId));
}

/** For a surface naming several sessions: re-render whenever any listed name changes. */
export function useListedNamesVersion(): number {
  return useSyncExternalStore(subscribeListedNames, () => listedNamesVersion);
}

export function resetListedNamesForTests(): void {
  listedNames.clear();
  listedNamesVersion += 1;
  for (const listener of listedNameListeners) listener();
}
