import { describe, expect, it, vi } from 'vitest';
import {
  GlanceCoverage,
  parseWindowList,
  visibleShareInList,
  windowNumberOf,
  type ListedWindow,
} from './engineGlanceCoverage';
import { GLANCE_COVERED_SHARE } from './utils/engineGlanceRules';

const GOOSE_PID = 2145;

// Front to back, as the window server listed the critic's desk (Q-313, 3.0.68): Windows App's
// 2048×1280 window over goose's 2056×1289 on the built-in display, a chat app behind goose.
const Q313_LIST: ListedWindow[] = [
  { number: 122785, pid: 43682, bounds: { x: 8, y: 49, width: 2048, height: 1280 } },
  { number: 122557, pid: GOOSE_PID, bounds: { x: 0, y: 40, width: 2056, height: 1289 } },
  { number: 21798, pid: 75108, bounds: { x: 0, y: 40, width: 2056, height: 1289 } },
];

const deferred = () => {
  let resolve!: (v: string) => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<string>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('the window server’s list — what covers a goose window (Q-313)', () => {
  it('the critic’s desk: goose covered but for an 8×9 px L — under the covered share', () => {
    const share = visibleShareInList(Q313_LIST, GOOSE_PID, 122557);
    expect(share).toBeCloseTo(28_744 / (2056 * 1289), 10);
    expect(share!).toBeLessThan(GLANCE_COVERED_SHARE);
  });

  it('only windows IN FRONT of goose, and never goose’s own (the glance panel is goose’s)', () => {
    const glanceOnTop: ListedWindow = {
      number: 9,
      pid: GOOSE_PID,
      bounds: { x: 0, y: 40, width: 2056, height: 1289 },
    };
    expect(visibleShareInList([glanceOnTop, ...Q313_LIST.slice(1)], GOOSE_PID, 122557)).toBe(1);
  });

  it('a goose window not in the on-screen list (minimized, another Space): not read', () => {
    expect(visibleShareInList(Q313_LIST, GOOSE_PID, 1)).toBeNull();
  });

  it('parses the script’s output, and refuses a shape it does not know — never an empty list', () => {
    expect(parseWindowList(JSON.stringify(Q313_LIST))).toEqual(Q313_LIST);
    expect(() => parseWindowList('{}')).toThrow('not an array');
    expect(() => parseWindowList('[{"number":1,"pid":2}]')).toThrow('entry 0');
    expect(() => parseWindowList('')).toThrow();
  });

  it('the window number inside Electron’s media source id', () => {
    expect(windowNumberOf('window:122557:0')).toBe(122557);
    expect(windowNumberOf('screen:1:0')).toBeNull();
  });
});

describe('GlanceCoverage — reads on request, one at a time, loud on failure', () => {
  function setup() {
    const reads: ReturnType<typeof deferred>[] = [];
    const onChanged = vi.fn();
    const warn = vi.fn();
    const coverage = new GlanceCoverage({
      ownPid: GOOSE_PID,
      readWindowList: () => {
        const d = deferred();
        reads.push(d);
        return d.promise;
      },
      onChanged,
      warn,
    });
    return { coverage, reads, onChanged, warn };
  }

  it('unread until a read lands; a landed read that changes goose’s share re-decides the glance', async () => {
    const { coverage, reads, onChanged } = setup();
    expect(coverage.visibleShareOf('window:122557:0')).toBeNull();
    coverage.measure();
    reads[0].resolve(JSON.stringify(Q313_LIST));
    await flush();
    expect(coverage.visibleShareOf('window:122557:0')).toBeLessThan(GLANCE_COVERED_SHARE);
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('a read that changes no goose share (another app moved elsewhere) does not refresh again', async () => {
    const { coverage, reads, onChanged } = setup();
    coverage.measure();
    reads[0].resolve(JSON.stringify(Q313_LIST));
    await flush();
    coverage.measure();
    const moved = Q313_LIST.map((w) =>
      w.pid === 75108 ? { ...w, bounds: { ...w.bounds, x: -3000 } } : w
    );
    reads[1].resolve(JSON.stringify(moved));
    await flush();
    expect(onChanged).toHaveBeenCalledTimes(1);
  });

  it('one read in flight: requests during it ask for exactly one more after it', async () => {
    const { coverage, reads } = setup();
    coverage.measure();
    coverage.measure();
    coverage.measure();
    expect(reads).toHaveLength(1);
    reads[0].resolve(JSON.stringify(Q313_LIST));
    await flush();
    expect(reads).toHaveLength(2);
  });

  it('a failed read is said once per reason and leaves the shares unread', async () => {
    const { coverage, reads, warn } = setup();
    coverage.measure();
    reads[0].resolve(JSON.stringify(Q313_LIST));
    await flush();
    coverage.measure();
    reads[1].reject(new Error('osascript: execution error'));
    await flush();
    coverage.measure();
    reads[2].reject(new Error('osascript: execution error'));
    await flush();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0]).toContain('osascript: execution error');
    expect(coverage.visibleShareOf('window:122557:0')).toBeNull();
  });

  it('forget (goose came to the front): the list goes, and a read already in flight lands on nothing', async () => {
    const { coverage, reads, onChanged } = setup();
    coverage.measure();
    coverage.forget();
    reads[0].resolve(JSON.stringify(Q313_LIST));
    await flush();
    expect(coverage.visibleShareOf('window:122557:0')).toBeNull();
    expect(onChanged).not.toHaveBeenCalled();
  });
});
