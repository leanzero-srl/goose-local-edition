import { describe, it, expect } from 'vitest';
import { deriveFleet } from './useSwarmRun';

/**
 * A NODE RUNS TWO LANES; THE STRIP SHOWED ONE.
 *
 * `workingByDevice` is a Map keyed by device and the loop was first-wins, but every LM Studio node in
 * this fleet is configured PARALLEL: 2. Measured on run swarm-20260829-100743413: gabee was running
 * open-coverage-1 — 68,393 reasoning characters, the largest lane in the run — alongside
 * slice-index-html, and only the second had a cell. Five live lanes, three cells, and the two biggest
 * were invisible however hard they worked.
 */
const lane = (taskId: string, device: string) =>
  ({ taskId, device, status: 'running', description: taskId, seq: 0 }) as never;

const base = {
  pool: ['gabee', 'mihai'],
  digests: {},
  digestMtimes: {},
  now: Date.now(),
  supervision: [],
  busyNodes: ['gabee', 'mihai'],
};

describe('a node with two live lanes', () => {
  it('keeps the second lane instead of dropping it', () => {
    const f = deriveFleet({
      ...base,
      laneSources: [lane('open-coverage-1', 'gabee'), lane('slice-index-html', 'gabee')],
    } as never);
    expect(f.workingByDevice.get('gabee')?.taskId).toBe('open-coverage-1');
    expect((f.alsoRunningByDevice.get('gabee') ?? []).map((l) => l.taskId)).toEqual([
      'slice-index-html',
    ]);
  });

  it('keeps the primary stable and does not duplicate it into the extras', () => {
    const f = deriveFleet({
      ...base,
      laneSources: [lane('a', 'gabee'), lane('a', 'gabee'), lane('b', 'gabee')],
    } as never);
    expect(f.workingByDevice.get('gabee')?.taskId).toBe('a');
    expect((f.alsoRunningByDevice.get('gabee') ?? []).map((l) => l.taskId)).toEqual(['b']);
  });

  it('leaves a single-lane node with no extras', () => {
    const f = deriveFleet({ ...base, laneSources: [lane('solo', 'mihai')] } as never);
    expect(f.alsoRunningByDevice.get('mihai')).toBeUndefined();
  });

  it('separates lanes by device rather than pooling them', () => {
    const f = deriveFleet({
      ...base,
      laneSources: [lane('g1', 'gabee'), lane('g2', 'gabee'), lane('m1', 'mihai')],
    } as never);
    expect((f.alsoRunningByDevice.get('gabee') ?? []).map((l) => l.taskId)).toEqual(['g2']);
    expect(f.alsoRunningByDevice.get('mihai')).toBeUndefined();
  });
});
