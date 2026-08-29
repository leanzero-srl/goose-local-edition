import fsSync from 'node:fs';

/** What a subscriber is currently looking at. Re-stated on every read; the registry diffs it. */
export interface SwarmWatchTarget {
  /** The workingDir string the renderer asked about, echoed back verbatim so a panel pointed at a
   *  different directory can ignore a delta that is not its own. */
  workingDir: string;
  /** `<runDir>/.swarm`, as resolved through the current-run breadcrumb. */
  swarmDir: string;
  /** The directory holding the run's event log. Equals swarmDir except in the benchmark layout,
   *  where the harness writes `run.jsonl` beside `.swarm` instead of inside it. */
  eventsDir: string;
  runId: string;
}

/**
 * A CHANGE HINT — deliberately carries no run data.
 *
 * fs.watch coalesces rapid appends, can double-fire, and on some filesystems drops events outright,
 * so a receiver must treat this as "read again now" and get its facts from the incremental reader.
 * A payload the renderer trusted as complete would fold a half-written JSONL line, and a dropped
 * event would freeze the panel forever.
 */
export interface SwarmDelta {
  workingDir: string;
  runId: string;
  /** The watched directory that changed, for diagnostics only. */
  source: string;
}

export interface WatchHandle {
  close(): void;
}

export type WatchFactory = (dir: string, onChange: () => void) => WatchHandle | null;

const defaultWatch: WatchFactory = (dir, onChange) => {
  const w = fsSync.watch(dir, { persistent: false }, () => onChange());
  // A watcher that errors (the directory was removed, the descriptor limit was hit) must not take the
  // process with it; the poll is the net that keeps the panel moving without it.
  w.on('error', () => closeQuietly(w));
  return w;
};

interface Entry {
  target: SwarmWatchTarget;
  send: (delta: SwarmDelta) => void;
  watchers: Map<string, WatchHandle>;
  timer: ReturnType<typeof setTimeout> | null;
  lastEmit: number;
  pendingSource: string;
}

export interface SwarmWatchOptions {
  watch?: WatchFactory;
  /** Longest a change can wait to be announced, and the shortest gap between two announcements. */
  debounceMs?: number;
  now?: () => number;
  setTimer?: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  clearTimer?: (t: ReturnType<typeof setTimeout>) => void;
}

/**
 * Push half of the realtime panel: watches a run's directories and tells subscribers to re-read.
 *
 * The engine appends to `.swarm/run-<id>.jsonl` and rewrites `.swarm/activity/<task>.json` as work
 * happens; without this the renderer only learns about it on its next poll, so the panel is stale by
 * up to the poll interval no matter how cheap the read became. The poll REMAINS — it is the
 * reconciling net for the updates fs.watch drops.
 */
export class SwarmWatchRegistry {
  private readonly entries = new Map<string, Entry>();
  private readonly watch: WatchFactory;
  private readonly debounceMs: number;
  private readonly now: () => number;
  private readonly setTimer: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  private readonly clearTimer: (t: ReturnType<typeof setTimeout>) => void;

  constructor(opts: SwarmWatchOptions = {}) {
    this.watch = opts.watch ?? defaultWatch;
    this.debounceMs = opts.debounceMs ?? 100;
    this.now = opts.now ?? Date.now;
    this.setTimer = opts.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = opts.clearTimer ?? ((t) => clearTimeout(t));
  }

  /**
   * Point `subscriber` at `target`, arming any directory not already watched. Idempotent, and called
   * on every read, which is what re-targets the watch when the run moves. Returns true only the first
   * time a subscriber is seen, so the caller registers its teardown hook exactly once.
   *
   * `subscriber` must identify the SUBSCRIPTION, not the window. One renderer routinely mounts several
   * useSwarmRun hooks on different working dirs -- BaseChat and NavigationPanel can both be live -- and
   * keying on the webContents id alone made them overwrite each other's target: whichever read last
   * won, the other silently stopped receiving deltas, and a torn-down hook released the watch out from
   * under a live one. Callers pass `${webContentsId}::${workingDir}`.
   */
  ensure(subscriber: string, target: SwarmWatchTarget, send: (delta: SwarmDelta) => void): boolean {
    const existing = this.entries.get(subscriber);
    if (!existing) {
      const entry: Entry = {
        target,
        send,
        watchers: new Map(),
        timer: null,
        lastEmit: 0,
        pendingSource: target.swarmDir,
      };
      this.entries.set(subscriber, entry);
      this.arm(subscriber, entry);
      return true;
    }
    existing.target = target;
    existing.send = send;
    this.arm(subscriber, existing);
    return false;
  }

  /** Drop a subscriber's watchers. Without it every window close, and every run switch, leaks a
   *  file handle per watched directory for the life of the process. */
  release(subscriber: string): void {
    const entry = this.entries.get(subscriber);
    if (!entry) return;
    for (const w of entry.watchers.values()) closeQuietly(w);
    entry.watchers.clear();
    if (entry.timer !== null) this.clearTimer(entry.timer);
    entry.timer = null;
    this.entries.delete(subscriber);
  }

  releaseAll(): void {
    for (const id of [...this.entries.keys()]) this.release(id);
  }

  /** Directories currently armed for a subscriber — the leak check's only honest witness. */
  watchedDirs(subscriber: string): string[] {
    return [...(this.entries.get(subscriber)?.watchers.keys() ?? [])].sort();
  }

  size(): number {
    return this.entries.size;
  }

  /**
   * THE EVENT LOG ONLY — deliberately NOT `activity/`.
   *
   * Watching the activity directory made the push counterproductive. The engine rewrites
   * `activity/<task>.json` roughly 2.5 times a second PER LANE, so a nine-lane run generates ~22
   * changes/sec; debounced at 100ms that is 10 deltas/sec, each one triggering a full read. The poll it
   * was meant to improve on runs at 2/sec, so the push made the main process do five times the work.
   *
   * And it is work the incremental reader cannot make cheap. Digests are REWRITTEN IN PLACE, not
   * appended, so `readTail`/`readEvents` have nothing to skip: every delta re-reads and re-parses every
   * digest in full. The run log is the opposite — append-only, low frequency, and the thing whose
   * latency actually matters, because a task starting or finishing is what the panel is waiting to
   * show. So push on the event stream and leave the digests to the 500ms poll, which already bounds
   * their cost and was never the reason the panel felt slow.
   */
  private wantedDirs(target: SwarmWatchTarget): string[] {
    return [...new Set([target.swarmDir, target.eventsDir])];
  }

  private arm(subscriber: string, entry: Entry): void {
    const wanted = this.wantedDirs(entry.target);
    for (const [dir, w] of entry.watchers) {
      if (!wanted.includes(dir)) {
        closeQuietly(w);
        entry.watchers.delete(dir);
      }
    }
    for (const dir of wanted) {
      if (entry.watchers.has(dir)) continue;
      // A directory that cannot be watched — it does not exist yet, or the process is out of
      // descriptors — is skipped, never fatal: the next read re-arms it, and until then the poll
      // covers it. One unwatchable directory must not cost the others their push.
      try {
        const handle = this.watch(dir, () => this.schedule(subscriber, dir));
        if (handle) entry.watchers.set(dir, handle);
      } catch {
        /* re-armed on the next ensure */
      }
    }
  }

  /**
   * Leading edge fires at once so the first change of a quiet lane is instant; anything during the
   * window is announced by a trailing timer. The trailing half is not optional: a plain rate limiter
   * would swallow the LAST write of a run — exactly the delta that says the run ended — and leave
   * the panel showing a lane mid-flight until the next poll.
   */
  private schedule(subscriber: string, dir: string): void {
    const entry = this.entries.get(subscriber);
    if (!entry) return;
    entry.pendingSource = dir;
    if (entry.timer !== null) return;
    const since = this.now() - entry.lastEmit;
    if (since >= this.debounceMs) {
      this.fire(entry);
      return;
    }
    entry.timer = this.setTimer(() => {
      entry.timer = null;
      this.fire(entry);
    }, this.debounceMs - since);
  }

  private fire(entry: Entry): void {
    entry.lastEmit = this.now();
    entry.send({
      workingDir: entry.target.workingDir,
      runId: entry.target.runId,
      source: entry.pendingSource,
    });
  }
}

function closeQuietly(w: WatchHandle): void {
  try {
    w.close();
  } catch {
    /* already closed */
  }
}
