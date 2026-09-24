import { describe, expect, it } from 'vitest';
import {
  distributedLiveBase,
  isMlxDistributedReport,
  toMlxDistributedReport,
} from './mlxDistributedReport';
import {
  FLASH_READY,
  FLASH_SERVING,
  HOSTING_RANK_1,
  STOPPED_WITH_CONFIG,
} from '../components/leanzero-swarm/mlxDistributed.fixtures';

describe('toMlxDistributedReport — what main is told after every status read', () => {
  it('carries each rank’s layers, peak and the budget from the last preflight', () => {
    const report = toMlxDistributedReport(FLASH_READY);
    expect(report.mode).toBe('distributed');
    expect(report.nodeNames).toEqual(['MacBook Pro', 'workhorse']);
    expect(report.nodes[0]).toMatchObject({
      name: 'MacBook Pro',
      layers: { kind: 'layers', first: 0, last: 19, count: 20 },
      peakGb: 61.0,
    });
    expect(report.nodes[0].budgetGb).toBeCloseTo(83.43, 2);
    expect(report.nodes[1].budgetGb).toBeCloseTo(55.44, 2);
    expect(report.restarts).toBe(1);
    expect(report.lastAlarm).toEqual({
      kind: 'restart',
      node: null,
      message: 'restart 1 of the breaker window',
    });
    expect(isMlxDistributedReport(report)).toBe(true);
  });

  it('stopped: single mode, the configured node names, no nodes, no alarm', () => {
    const report = toMlxDistributedReport(STOPPED_WITH_CONFIG);
    expect(report).toMatchObject({
      mode: 'single',
      state: 'stopped',
      nodeNames: ['MacBook Pro', 'workhorse'],
      nodes: [],
      lastAlarm: null,
      inflight: null,
    });
    expect(isMlxDistributedReport(report)).toBe(true);
  });

  it("rank 0's base rides along; main reads it only while the run owns the Mac and is up", () => {
    const ready = toMlxDistributedReport(FLASH_READY);
    expect(ready.baseUrl).toBe(FLASH_READY.baseUrl);
    expect(distributedLiveBase(ready)).toBe(FLASH_READY.baseUrl);
    expect(distributedLiveBase(toMlxDistributedReport(FLASH_SERVING))).toBe(FLASH_READY.baseUrl);
    expect(distributedLiveBase({ ...ready, state: 'starting' })).toBeNull();
    expect(distributedLiveBase(toMlxDistributedReport(STOPPED_WITH_CONFIG))).toBeNull();
    expect(distributedLiveBase(toMlxDistributedReport(HOSTING_RANK_1))).toBeNull();
    expect(distributedLiveBase(null)).toBeNull();
    expect(isMlxDistributedReport({ ...ready, baseUrl: 8091 })).toBe(false);
  });

  it('IPC is a trust boundary: a malformed payload is not a report', () => {
    const good = toMlxDistributedReport(FLASH_READY);
    expect(isMlxDistributedReport(null)).toBe(false);
    expect(isMlxDistributedReport({ ...good, mode: 'both' })).toBe(false);
    expect(isMlxDistributedReport({ ...good, restarts: '1' })).toBe(false);
    expect(
      isMlxDistributedReport({ ...good, nodes: [{ ...good.nodes[0], layers: { kind: 'x' } }] })
    ).toBe(false);
    expect(isMlxDistributedReport({ ...good, lastAlarm: { kind: 'hang' } })).toBe(false);
    expect(isMlxDistributedReport({ ...good, hosting: { rank: '1' } })).toBe(false);
    expect(isMlxDistributedReport({ ...good, hosting: undefined })).toBe(false);
  });

  it('a rank served for another Mac rides along, and validates', () => {
    const report = toMlxDistributedReport(HOSTING_RANK_1);
    expect(report.mode).toBe('single');
    expect(report.hosting).toEqual({
      rank: 1,
      requester: 'MacBook Pro',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
      backend: 'jaccl',
      state: 'serving',
      load: null,
    });
    expect(isMlxDistributedReport(report)).toBe(true);
    expect(toMlxDistributedReport(FLASH_READY).hosting).toBeNull();
  });
});
