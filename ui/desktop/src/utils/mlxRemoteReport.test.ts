import { describe, expect, it } from 'vitest';
import {
  isMlxRemoteReport,
  remoteLiveBase,
  remoteTrayLine,
  toMlxRemoteReport,
} from './mlxRemoteReport';
import { buildMlxTrayModel } from './mlxTray';
import { INITIAL_SNAPSHOT, type MlxEngineSnapshot } from './mlxEngineMonitor';
import { EMPTY_BOOK, parseMlxLiveStatus } from '../components/leanzero-swarm/mlxLiveStats';
import {
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';

const RELAY = 'http://127.0.0.1:61001/relay/cafe';

const READY = {
  state: 'ready',
  peer: 'worksmacstudio-lan-9c1e2a',
  peerHostname: 'WorksMacStudio.lan',
  peerComputerName: "Work's Mac Studio",
  baseUrl: RELAY,
  modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  servedModelId: 'mihai-qwen3.8-27b',
  capacity: 8,
  activeRequests: 0,
  generationTps: 22.2,
};

function remoteSnapshot(body: unknown): MlxEngineSnapshot {
  const read = parseMlxLiveStatus(body);
  if (!read.ok) throw new Error(read.detail);
  return {
    ...INITIAL_SNAPSHOT,
    engine: 'remote',
    mode: 'running',
    baseUrl: RELAY,
    stats: read.stats,
    rates: EMPTY_BOOK,
  };
}

const labels = (model: ReturnType<typeof buildMlxTrayModel>) =>
  model.items.flatMap((i) => (i.type === 'separator' ? [] : [i.label]));

describe('the remote-single report main receives', () => {
  it('no route is null — chat stays on this Mac, nothing is claimed', () => {
    expect(toMlxRemoteReport(null)).toBeNull();
    expect(toMlxRemoteReport({ state: 'off' })).toBeNull();
    expect(isMlxRemoteReport(null)).toBe(true);
  });

  it('a live route projects its facts — the Mac by its owner’s name — and passes the IPC check', () => {
    const report = toMlxRemoteReport(READY);
    expect(report).toEqual({
      state: 'ready',
      peerName: "Work's Mac Studio",
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      baseUrl: RELAY,
      activeRequests: 0,
      lastError: null,
    });
    expect(isMlxRemoteReport(report)).toBe(true);
    expect(isMlxRemoteReport({ ...report, activeRequests: 'many' })).toBe(false);
    expect(isMlxRemoteReport({ ...report, baseUrl: 7 })).toBe(false);
    expect(isMlxRemoteReport({ state: 'ready' })).toBe(false);
  });

  it('an older backend without the computer name falls back to the hostname, never the node id', () => {
    const older = toMlxRemoteReport({ ...READY, peerComputerName: undefined, baseUrl: undefined })!;
    expect(older.peerName).toBe('WorksMacStudio.lan');
    expect(older.baseUrl).toBeNull();
    expect(remoteLiveBase(older)).toBeNull();
  });

  it('main reads the relay while the route serves — and while its Mac does not answer', () => {
    const report = toMlxRemoteReport(READY)!;
    expect(remoteLiveBase(report)).toBe(RELAY);
    expect(remoteLiveBase({ ...report, state: 'reconnecting' })).toBe(RELAY);
    expect(remoteLiveBase({ ...report, state: 'mounting' })).toBeNull();
    expect(remoteLiveBase({ ...report, state: 'failed' })).toBeNull();
    expect(remoteLiveBase(null)).toBeNull();
  });
});

describe('the tray while chat is served from a linked Mac', () => {
  const options = (remote: ReturnType<typeof toMlxRemoteReport>) => ({
    canAct: true,
    mountModelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
    distributed: null,
    remote,
  });

  it('WRITING on the peer: green, its live rate in the title, the live lines, and a Stop', () => {
    const report = toMlxRemoteReport(READY)!;
    const model = buildMlxTrayModel(remoteSnapshot(GENERATING_STATUS), options(report));
    expect(model.title).toBe("Work's Mac Studio · 19.9 tok/s");
    expect(model.phase).toBe('writing');
    expect(model.items[0]).toMatchObject({
      type: 'info',
      label: "Serving from Work's Mac Studio · Qwen3.8-27B-Atlassian-Q8-mlx",
      phase: 'writing',
    });
    expect(labels(model)).toContain('Writing 19.9 tok/s');
    expect(model.items).toContainEqual({
      type: 'action',
      label: "Stop serving from Work's Mac Studio",
      action: 'stop-remote',
      enabled: true,
    });
    // This Mac's own engine is set aside: no Mount offered over a live route.
    expect(model.items.some((i) => i.type === 'action' && i.action === 'mount')).toBe(false);
  });

  it('READING a prompt on the peer is blue; IDLE is grey — the last reply’s rate never paints it green', () => {
    const report = toMlxRemoteReport(READY)!;
    const reading = buildMlxTrayModel(remoteSnapshot(PREFILL_STATUS), options(report));
    expect(reading.phase).toBe('reading');
    expect(reading.title).toMatch(/^Work's Mac Studio · Reading/);
    const idle = buildMlxTrayModel(remoteSnapshot(IDLE_STATUS), options(report));
    expect(idle.phase).toBe('idle');
    expect(idle.title).toBe("Work's Mac Studio · Idle");
  });

  it('mounting there is amber and says so; failed is red with the peer’s words', () => {
    const mounting = toMlxRemoteReport({ ...READY, state: 'mounting' })!;
    expect(remoteTrayLine(mounting)).toBe(
      "Serving from Work's Mac Studio · Qwen3.8-27B-Atlassian-Q8-mlx · mounting"
    );
    const m = buildMlxTrayModel(INITIAL_SNAPSHOT, options(mounting));
    expect(m.title).toBe("Work's Mac Studio · mounting");
    expect(m.phase).toBe('loading');

    const failed = toMlxRemoteReport({
      ...READY,
      state: 'failed',
      lastError: "Work's Mac Studio's goose reports its engine failed (exit 137)",
    })!;
    const f = buildMlxTrayModel(INITIAL_SNAPSHOT, options(failed));
    expect(f.title).toBe("Work's Mac Studio · failed");
    expect(f.phase).toBe('failed');
    expect(labels(f)).toContain(
      "Error: Work's Mac Studio's goose reports its engine failed (exit 137)"
    );
  });

  it('a relay read that failed says why instead of inventing live lines', () => {
    const report = toMlxRemoteReport(READY)!;
    const model = buildMlxTrayModel(
      {
        ...INITIAL_SNAPSHOT,
        engine: 'remote',
        mode: 'unknown',
        statusDetail: 'timeout: no answer within 1500 ms',
      },
      options(report)
    );
    expect(model.title).toBe("Work's Mac Studio");
    expect(model.phase).toBe('idle');
    expect(labels(model)).toContain(
      'Rates unavailable over LeanZero Link: timeout: no answer within 1500 ms'
    );
    expect(labels(model).some((l) => /tok\/s/.test(l))).toBe(false);
  });

  it('LOST CONTACT (Q-47/Q-48): the title says it is reconnecting to that Mac, amber, the read’s words, and Stop stays', () => {
    // recovery-kill-link 11.6 s: the route still says ready, main's relay read timed out.
    const report = toMlxRemoteReport(READY)!;
    const model = buildMlxTrayModel(
      {
        ...INITIAL_SNAPSHOT,
        engine: 'remote',
        mode: 'reconnecting',
        statusDetail: 'timeout: no answer within 1500 ms',
      },
      options(report)
    );
    expect(model.title).toBe("Reconnecting to Work's Mac Studio");
    expect(model.phase).toBe('loading');
    expect(model.items[0]).toMatchObject({
      label: "Lost contact with Work's Mac Studio — reconnecting…",
      phase: 'loading',
    });
    expect(labels(model)).toContain('Last read: timeout: no answer within 1500 ms');
    expect(model.items.some((i) => i.type === 'action' && i.action === 'stop-remote')).toBe(true);
    expect(model.items.some((i) => i.type === 'action' && i.action === 'mount')).toBe(false);

    // The route itself says so (the backend's `reconnecting`), whatever main last read.
    const said = toMlxRemoteReport({ ...READY, state: 'reconnecting' })!;
    const fromRoute = buildMlxTrayModel(INITIAL_SNAPSHOT, options(said));
    expect(fromRoute.title).toBe("Reconnecting to Work's Mac Studio");
    expect(fromRoute.phase).toBe('loading');
  });

  it('`failed` stays red and means the peer answered that its engine failed — never "reconnecting"', () => {
    const failed = toMlxRemoteReport({ ...READY, state: 'failed', lastError: 'exit 137' })!;
    const f = buildMlxTrayModel(
      { ...INITIAL_SNAPSHOT, engine: 'remote', mode: 'failed' },
      options(failed)
    );
    expect(f.title).toBe("Work's Mac Studio · failed");
    expect(f.phase).toBe('failed');
  });
});
