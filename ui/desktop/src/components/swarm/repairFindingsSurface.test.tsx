import { cleanup, render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { resetFoldCache, resetLiveChannelMemory, useSwarmRun } from './useSwarmRun';

/** The Repair findings surface: REPAIR v2's repro verdict and promotion decision per shard, as solid
 *  chips in the engine's own words. A shard that never re-ran its check must not read like a fix. */
const S = (round: number, shard: string, task_id: string) => ({ round, shard, task_id });
const A = S(1, 'app/api.py', 'complete-fix::app-api');
const B = S(1, 'app/db.py', 'complete-fix::app-db');
const EVENTS = [
  { event: 'run_started', prompt: '# Build app', pool: [{ id: 'mac-a', model_id: 'a-qwen' }] },
  { event: 'pool_resolved', devices: [{ id: 'mac-a', model_id: 'a-qwen' }] },
  { event: 'plan_loaded', tasks: [{ id: 'app-api', files: ['app/api.py'], depends_on: [] }] },
  { event: 'complete_verify', findings: 2 },
  { event: 'complete_fix_dispatched', ...A, finding_index: 0, model: 'a-qwen', baseline_findings: 2, owned: ['app/api.py'], conflict_retry: false },
  { event: 'repro_confirmed', ...A, finding: 'GET /records 500s', check: 'probe:/records', calls: 2, unparseable_rows: 0, detail: { call: 'curl /records' } },
  { event: 'finding_flipped', ...A, finding: 'GET /records 500s', check: 'probe:/records', fails_before: 1, fails_after: 0 },
  { event: 'complete_fix_completed', ...A, finding_index: 0, model: 'a-qwen', secs: 60, agent_ok: true, promoted: true, shard_changed: true, conflicted: false, merge_unavailable: false, setup_failed: null },
  { event: 'complete_fix_dispatched', ...B, finding_index: 1, model: 'a-qwen', baseline_findings: 1, owned: ['app/db.py'], conflict_retry: false },
  { event: 'repro_never_ran', ...B, finding: 'schema missing index', check: 'smoke:schema', calls: 1, unparseable_rows: 0, detail: {} },
  { event: 'finding_still_failing', ...B, finding: 'schema missing index', check: 'smoke:schema', fails_on_preview: 1, quote: 'no such index' },
  { event: 'complete_fix_completed', ...B, finding_index: 1, model: 'a-qwen', secs: 40, agent_ok: true, promoted: false, shard_changed: true, conflicted: false, merge_unavailable: false, setup_failed: null },
];

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('Repair findings — the per-shard proof on screen', () => {
  beforeEach(() => {
    resetFoldCache();
    resetLiveChannelMemory();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-3node-r0',
      dir: '/tmp/build',
      events: EVENTS,
      activity: {},
      activityMtimes: {},
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    e.fleetStatus = vi.fn(async () => ({}));
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('one row per shard: repro chip, decision chip, promoted — and the header counts the promoted', async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    expect(result.current.repairFindings).toHaveLength(2);
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
    const zone = await screen.findByTestId('repair-findings');
    expect(zone.textContent).toContain("1 of 2 shards promoted on the finding's own flip");
    expect(screen.getByTestId('repair-findings-count-chip').textContent).toBe('1/2');
    const rows = zone.querySelectorAll('[data-testid="repair-finding"]');
    expect(rows).toHaveLength(2);
    const [a, b] = [...rows] as HTMLElement[];
    expect(a.dataset).toMatchObject({ repro: 'confirmed', decision: 'flipped', promoted: 'true' });
    expect(a.textContent).toBe('r1app/api.pyrepro confirmedflipped 1 → 0promoted');
    expect(b.dataset).toMatchObject({ repro: 'never_ran', decision: 'still_failing', promoted: 'false' });
    expect(b.textContent).toBe('r1app/db.pyrepro never ranstill failingnot promoted');
    // No left rail, no tint, no faded pulse anywhere in the surface.
    expect(zone.innerHTML).not.toMatch(/border-l|\/10\b|opacity-|animate-pulse/);
  });
});
