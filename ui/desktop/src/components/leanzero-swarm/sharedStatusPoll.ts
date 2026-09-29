import { useCallback, useSyncExternalStore } from 'react';

/**
 * ONE read of a goosed status for every surface of this window that watches it (Q-208, design D9).
 * Before this, each surface ran its own timer on the same method — the composer alone read the
 * engine status three times per tick (the served-by chip, the model bar, the no-node notice).
 *
 * Each watcher states what it needs — how often, whether it keeps reading while the window is
 * hidden, and an optional argument the read must carry (the Engine tab's `fitModelId`). The store
 * reads at the SHORTEST interval any live watcher asked for, carries the argument a watcher asked
 * for, and hands every read to all of them.
 *
 * Truth rules, the hooks' own before the fold:
 *  - a failed read INVALIDATES the value (`value` null, `error` set): no claim outlives its read;
 *  - with no watcher left the store forgets everything, so a surface that opens later never shows a
 *    read nobody was taking;
 *  - a read overtaken by a newer one says nothing about now and is dropped (no reordering);
 *  - while the window is hidden only a `whileHidden` watcher keeps the reads going.
 */

export interface PollSnapshot<T> {
  /** The last read's answer; null before the first read lands and after a failed one. */
  value: T | null;
  /** The last read failed; `error` is what it threw. */
  failed: boolean;
  error: unknown;
  /** Reads landed since the first watcher arrived: a new snapshot object per read. */
  reads: number;
}

export interface PollNeed<A> {
  intervalMs: number;
  whileHidden?: boolean;
  /** What the read must carry for this watcher (null = nothing). The newest watcher's wins. */
  arg?: A | null;
}

export interface SharedPoll<T, A> {
  subscribe: (listener: () => void, need: PollNeed<A>) => () => void;
  snapshot: () => PollSnapshot<T>;
  /** Read now for every watcher; resolves when that read lands. A no-op with no watcher. */
  refresh: () => Promise<void>;
  /** How many reads the store has started — the one-poll-per-tick proof reads this. */
  readsStarted: () => number;
}

interface Watcher<A> {
  listener: () => void;
  need: PollNeed<A>;
}

const EMPTY: PollSnapshot<never> = { value: null, failed: false, error: null, reads: 0 };

export function createSharedPoll<T, A = never>(
  read: (arg: A | null) => Promise<T>
): SharedPoll<T, A> {
  const watchers = new Map<symbol, Watcher<A>>();
  let snap: PollSnapshot<T> = EMPTY;
  let timer: ReturnType<typeof setInterval> | null = null;
  let timerMs: number | null = null;
  let arg: A | null = null;
  let generation = 0;
  let started = 0;
  let landed = 0;
  let inFlight = 0;
  let listening = false;

  const visible = () =>
    typeof document === 'undefined' || document.visibilityState === 'visible';

  const active = (): Watcher<A>[] => {
    const all = [...watchers.values()];
    return visible() ? all : all.filter((w) => w.need.whileHidden === true);
  };

  const wantedArg = (): A | null => {
    let found: A | null = null;
    for (const w of watchers.values()) if (w.need.arg != null) found = w.need.arg;
    return found;
  };

  const emit = () => {
    for (const w of [...watchers.values()]) w.listener();
  };

  const readNow = async (): Promise<void> => {
    const gen = generation;
    const seq = ++started;
    inFlight += 1;
    try {
      const value = await read(arg);
      if (gen !== generation || seq < landed) return;
      landed = seq;
      snap = { value, failed: false, error: null, reads: snap.reads + 1 };
    } catch (error) {
      if (gen !== generation || seq < landed) return;
      landed = seq;
      snap = { value: null, failed: true, error, reads: snap.reads + 1 };
    } finally {
      inFlight -= 1;
    }
    emit();
  };

  // Surfaces mounted in one render subscribe one after another in the same task: their "read now"
  // asks coalesce into ONE read, taken with the argument the last of them left.
  let readQueued = false;
  const queueRead = () => {
    if (readQueued) return;
    readQueued = true;
    void Promise.resolve().then(() => {
      readQueued = false;
      if (watchers.size > 0) void readNow();
    });
  };

  const tick = () => {
    // A slow goosed never gets a second read of the same thing stacked on the first.
    if (inFlight > 0) return;
    void readNow();
  };

  const stopTimer = () => {
    if (timer != null) clearInterval(timer);
    timer = null;
    timerMs = null;
  };

  /** Re-arm to the shortest live interval; `readFirst` reads now (the old hooks' start()). */
  const arm = (readFirst: boolean) => {
    const live = active();
    if (live.length === 0) {
      stopTimer();
      return;
    }
    const ms = Math.min(...live.map((w) => w.need.intervalMs));
    if (readFirst) queueRead();
    if (timer != null && timerMs === ms && !readFirst) return;
    stopTimer();
    timerMs = ms;
    timer = setInterval(tick, ms);
  };

  const onVisibility = () => arm(visible());

  const subscribe = (listener: () => void, need: PollNeed<A>) => {
    const id = Symbol('watcher');
    const before = active();
    const beforeMs = before.length ? Math.min(...before.map((w) => w.need.intervalMs)) : null;
    watchers.set(id, { listener, need });
    if (!listening && typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisibility);
      listening = true;
    }
    const nextArg = wantedArg();
    const argChanged = nextArg !== arg;
    arg = nextArg;
    const live = active().some((w) => w.listener === listener);
    // A read is taken for a watcher only when the running reads do not already serve it: the
    // first watcher, a shorter interval than the store runs at, or an argument the reads lack.
    const serves =
      beforeMs != null && beforeMs <= need.intervalMs && !(argChanged && need.arg != null);
    if (live) arm(!serves);
    return () => {
      if (!watchers.delete(id)) return;
      if (watchers.size === 0) {
        stopTimer();
        if (listening) document.removeEventListener('visibilitychange', onVisibility);
        listening = false;
        generation += 1;
        snap = EMPTY;
        arg = null;
        return;
      }
      arg = wantedArg();
      arm(false);
    };
  };

  return {
    subscribe,
    snapshot: () => snap,
    refresh: () => (watchers.size === 0 ? Promise.resolve() : readNow()),
    readsStarted: () => started,
  };
}

/**
 * Watch `poll` while `need` is given; null = not watching (the old hooks' `enabled: false`): no
 * subscription, no read, and null back.
 */
export function useSharedPoll<T, A>(
  poll: SharedPoll<T, A>,
  need: PollNeed<A> | null
): PollSnapshot<T> | null {
  const on = need != null;
  const intervalMs = need?.intervalMs ?? 0;
  const whileHidden = need?.whileHidden === true;
  const arg = need?.arg ?? null;
  const subscribe = useCallback(
    (listener: () => void) =>
      on ? poll.subscribe(listener, { intervalMs, whileHidden, arg }) : () => undefined,
    [poll, on, intervalMs, whileHidden, arg]
  );
  return useSyncExternalStore(subscribe, () => (on ? poll.snapshot() : null));
}
