import { describe, it, expect } from 'vitest';
import path from 'node:path';
import { SwarmWatchRegistry, type SwarmDelta, type WatchFactory } from './swarmWatch';

/** A fake filesystem watcher: the test decides when a directory "changes", and can see what is armed. */
function fakeWatch() {
  const open = new Map<string, () => void>();
  const opened: string[] = [];
  const closed: string[] = [];
  const factory: WatchFactory = (dir, onChange) => {
    open.set(dir, onChange);
    opened.push(dir);
    return {
      close() {
        open.delete(dir);
        closed.push(dir);
      },
    };
  };
  return {
    factory,
    opened,
    closed,
    change(dir: string) {
      const cb = open.get(dir);
      if (!cb) throw new Error(`not watching ${dir}`);
      cb();
    },
    isWatching: (dir: string) => open.has(dir),
    openCount: () => open.size,
  };
}

/** A clock the test advances by hand, driving both the registry's `now` and its timers. */
function fakeClock() {
  let t = 1_000;
  const timers: Array<{ at: number; fn: () => void; id: number }> = [];
  let next = 1;
  return {
    now: () => t,
    setTimer: (fn: () => void, ms: number) => {
      const id = next++;
      timers.push({ at: t + ms, fn, id });
      return id as unknown as ReturnType<typeof setTimeout>;
    },
    clearTimer: (h: ReturnType<typeof setTimeout>) => {
      const i = timers.findIndex((x) => x.id === (h as unknown as number));
      if (i >= 0) timers.splice(i, 1);
    },
    advance(ms: number) {
      t += ms;
      for (const fired of timers.filter((x) => x.at <= t)) {
        timers.splice(timers.indexOf(fired), 1);
        fired.fn();
      }
    },
    pending: () => timers.length,
  };
}

const RUN = '/build/.swarm';
const target = (
  over: Partial<{ workingDir: string; swarmDir: string; eventsDir: string; runId: string }> = {}
) => ({
  workingDir: '/build',
  swarmDir: RUN,
  eventsDir: RUN,
  runId: 'r1',
  ...over,
});

function makeRegistry(debounceMs = 100) {
  const w = fakeWatch();
  const clock = fakeClock();
  const sent: SwarmDelta[] = [];
  const reg = new SwarmWatchRegistry({
    watch: w.factory,
    debounceMs,
    now: clock.now,
    setTimer: clock.setTimer,
    clearTimer: clock.clearTimer,
  });
  return { reg, w, clock, sent, send: (d: SwarmDelta) => sent.push(d) };
}

describe('SwarmWatchRegistry — the push half of the realtime panel', () => {
  it('arms the run dir and its events dir, and DELIBERATELY NOT activity/', () => {
    const { reg, w, send } = makeRegistry();
    reg.ensure('1', target(), send);
    expect(reg.watchedDirs('1')).toEqual([RUN]);
    // The engine rewrites activity/<task>.json ~2.5x/sec PER LANE. Watching it turned the push into
    // ~10 deltas/sec against a 2/sec poll, and digests are rewritten in place so the incremental
    // reader cannot make those reads cheap. Push belongs on the append-only event log.
    expect(w.opened).not.toContain(path.join(RUN, 'activity'));
  });

  it('watches the benchmark layout, where the event log sits beside .swarm', () => {
    const { reg, send } = makeRegistry();
    reg.ensure('1', target({ eventsDir: '/build' }), send);
    expect(reg.watchedDirs('1')).toEqual(['/build', RUN]);
  });

  it('announces the first change immediately — a quiet lane must not wait out the debounce', () => {
    const { reg, w, sent, send } = makeRegistry();
    reg.ensure('1', target(), send);
    w.change(RUN);
    expect(sent).toEqual([{ workingDir: '/build', runId: 'r1', source: RUN }]);
  });

  it('coalesces a burst but never swallows the LAST change of it', () => {
    const { reg, w, clock, sent, send } = makeRegistry(100);
    reg.ensure('1', target({ eventsDir: '/build' }), send);

    w.change(RUN); // leading edge
    clock.advance(10);
    for (let i = 0; i < 20; i++) w.change(RUN);
    clock.advance(10);
    w.change('/build'); // the final write of the burst, on the other watched dir

    expect(sent).toHaveLength(1);
    clock.advance(100);
    expect(sent).toHaveLength(2);
    expect(sent[1].source).toBe('/build');
  });

  it('rate-limits sustained churn to one delta per window and keeps going', () => {
    const { reg, w, clock, sent, send } = makeRegistry(100);
    reg.ensure('1', target(), send);
    for (let i = 0; i < 10; i++) {
      w.change(RUN);
      clock.advance(30);
    }
    expect(sent.length).toBeGreaterThan(1);
    expect(sent.length).toBeLessThanOrEqual(4);
  });

  it('carries no run data — a delta is a hint to re-read, never the change itself', () => {
    const { reg, w, sent, send } = makeRegistry();
    reg.ensure('1', target(), send);
    w.change(RUN);
    expect(Object.keys(sent[0]).sort()).toEqual(['runId', 'source', 'workingDir']);
  });

  it('does not arm activity/ even once the engine has created it', () => {
    const w = fakeWatch();
    const activity = path.join(RUN, 'activity');
    const reg = new SwarmWatchRegistry({ watch: w.factory, debounceMs: 100 });
    reg.ensure('1', target(), () => {});
    reg.ensure('1', target(), () => {});
    expect(reg.watchedDirs('1')).toEqual([RUN]);
    expect(w.opened).not.toContain(activity);
  });

  it('retargets on a run move, closing the old watchers instead of leaking them', () => {
    const { reg, w, send } = makeRegistry();
    reg.ensure('1', target(), send);
    reg.ensure('1',
      target({
        workingDir: '/other',
        swarmDir: '/other/.swarm',
        eventsDir: '/other/.swarm',
        runId: 'r2',
      }),
      send
    );

    expect(w.closed).toEqual([RUN]);
    expect(reg.watchedDirs('1')).toEqual(['/other/.swarm']);
    expect(w.openCount()).toBe(1);
  });

  it('keeps the same watchers when only the run id rolls, and stamps deltas with the new one', () => {
    const { reg, w, sent, send } = makeRegistry();
    reg.ensure('1', target(), send);
    reg.ensure('1', target({ runId: 'r2' }), send);
    expect(w.closed).toEqual([]);
    w.change(RUN);
    expect(sent[0].runId).toBe('r2');
  });

  it('reports the first ensure only once, so a teardown hook is registered once per subscriber', () => {
    const { reg, send } = makeRegistry();
    expect(reg.ensure('1', target(), send)).toBe(true);
    expect(reg.ensure('1', target(), send)).toBe(false);
    expect(reg.ensure('2', target(), send)).toBe(true);
  });

  it('releases every watcher and pending timer when a renderer goes away', () => {
    const { reg, w, clock, sent, send } = makeRegistry(100);
    reg.ensure('1', target(), send);
    w.change(RUN);
    w.change(RUN); // leaves a trailing timer pending
    expect(clock.pending()).toBe(1);

    reg.release('1');
    expect(w.openCount()).toBe(0);
    expect(reg.size()).toBe(0);
    expect(clock.pending()).toBe(0);
    clock.advance(1000);
    expect(sent).toHaveLength(1);
  });

  it('survives a filesystem that refuses to watch, arming what it can', () => {
    const w = fakeWatch();
    const reg = new SwarmWatchRegistry({
      watch: (dir, cb) => {
        if (dir === RUN) throw new Error('EMFILE');
        return w.factory(dir, cb);
      },
      debounceMs: 100,
    });
    expect(() => reg.ensure('1', target({ eventsDir: '/build' }), () => {})).not.toThrow();
    expect(reg.watchedDirs('1')).toEqual(['/build']);
  });

  it('routes each subscriber only its own deltas', () => {
    const w = fakeWatch();
    const a: SwarmDelta[] = [];
    const b: SwarmDelta[] = [];
    const reg = new SwarmWatchRegistry({ watch: w.factory, debounceMs: 100 });
    reg.ensure('1', target(), (d) => a.push(d));
    reg.ensure('2',
      target({ workingDir: '/other', swarmDir: '/other/.swarm', eventsDir: '/other/.swarm' }),
      (d) => b.push(d)
    );

    w.change('/other/.swarm');
    expect(a).toHaveLength(0);
    expect(b).toHaveLength(1);
    expect(b[0].workingDir).toBe('/other');
  });

  it('releaseAll drops every subscriber', () => {
    const { reg, w, send } = makeRegistry();
    reg.ensure('1', target(), send);
    reg.ensure('2',
      target({ workingDir: '/other', swarmDir: '/other/.swarm', eventsDir: '/other/.swarm' }),
      send
    );
    reg.releaseAll();
    expect(reg.size()).toBe(0);
    expect(w.openCount()).toBe(0);
  });
});

describe('a watcher that dies is re-armed', () => {
  it('forgets an errored handle so the next ensure() recreates it', () => {
    const opened: string[] = [];
    const errs: Array<() => void> = [];
    const reg = new SwarmWatchRegistry({
      watch: (dir, _onChange, onError) => {
        opened.push(dir);
        if (onError) errs.push(onError);
        return { close() {} };
      },
      debounceMs: 100,
    });
    reg.ensure('1', target(), () => {});
    expect(opened).toEqual([RUN]);

    // A re-arm while the handle is healthy must NOT open a second watcher on the same directory.
    reg.ensure('1', target(), () => {});
    expect(opened).toEqual([RUN]);

    // The watcher dies. arm() skips any directory already in the map, so without the forget it would
    // never come back — the push for that directory silently gone for the rest of the run.
    errs[0]();
    reg.ensure('1', target(), () => {});
    expect(opened).toEqual([RUN, RUN]);
  });
});
