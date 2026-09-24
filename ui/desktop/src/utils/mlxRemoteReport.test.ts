import { describe, expect, it } from 'vitest';
import { isMlxRemoteReport, remoteTrayLine, toMlxRemoteReport } from './mlxRemoteReport';
import { buildMlxTrayModel } from './mlxTray';
import { INITIAL_SNAPSHOT } from './mlxEngineMonitor';

const READY = {
  state: 'ready',
  peer: 'worksmacstudio-lan-9c1e2a',
  peerHostname: 'worksmacstudio-lan-9c1e2a',
  modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  servedModelId: 'mihai-qwen3.8-27b',
  capacity: 8,
  activeRequests: 0,
  generationTps: 22.2,
};

describe('the remote-single report main receives', () => {
  it('no route is null — chat stays on this Mac, nothing is claimed', () => {
    expect(toMlxRemoteReport(null)).toBeNull();
    expect(toMlxRemoteReport({ state: 'off' })).toBeNull();
    expect(isMlxRemoteReport(null)).toBe(true);
  });

  it('a live route projects its facts and passes the IPC check; a forged one does not', () => {
    const report = toMlxRemoteReport(READY);
    expect(report).toEqual({
      state: 'ready',
      peerHostname: 'worksmacstudio-lan-9c1e2a',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      generationTps: 22.2,
      activeRequests: 0,
      lastError: null,
    });
    expect(isMlxRemoteReport(report)).toBe(true);
    expect(isMlxRemoteReport({ ...report, generationTps: 'fast' })).toBe(false);
    expect(isMlxRemoteReport({ state: 'ready' })).toBe(false);
  });

  it('the tray says where chat goes: "Serving from <peer>", the model, the peer’s own rate', () => {
    const report = toMlxRemoteReport(READY)!;
    expect(remoteTrayLine(report)).toBe(
      'Serving from worksmacstudio-lan-9c1e2a · Qwen3.8-27B-Atlassian-Q8-mlx · 22.2 tok/s last reply'
    );
    const model = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      canAct: true,
      mountModelId: null,
      distributed: null,
      remote: report,
    });
    expect(model.title).toBe('Remote · 22.2 tok/s');
    expect(model.items[0]).toMatchObject({ type: 'info' });
    expect((model.items[0] as { label: string }).label).toMatch(
      /^Serving from worksmacstudio-lan-9c1e2a · Qwen3\.8-27B/
    );

    const mounting = toMlxRemoteReport({ ...READY, state: 'mounting', generationTps: undefined })!;
    expect(remoteTrayLine(mounting)).toBe(
      'Serving from worksmacstudio-lan-9c1e2a · Qwen3.8-27B-Atlassian-Q8-mlx · mounting'
    );
    expect(
      buildMlxTrayModel(INITIAL_SNAPSHOT, {
        canAct: true,
        mountModelId: null,
        distributed: null,
        remote: mounting,
      }).title
    ).toBe('Remote · mounting');
  });
});
