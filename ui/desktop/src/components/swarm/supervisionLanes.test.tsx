import { fireEvent, render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import {
  deriveFleet,
  foldSupervision,
  resetFoldCache,
  supervisionLaneKind,
  supervisionRollingCaption,
  useSwarmRun,
} from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * r6 SUPERVISION LANES (engine efa2014ab / 1aee43921 — Mihai: "we need the judge generations to be
 * captured in our window same as everything else"). The engine now keys every supervision call —
 * `judge-<task>` (ONE rolling lane per supervised task; digest `attempt` = look number, 1-BASED),
 * `replan-r<N>` (digit-exact), `prereview-<task>`, `tail-review-<dim>` — and stamps
 * `"supervision": true` into those digests (ABSENT, never false, on worker lanes and all pre-r6
 * logs). This file pins the UI half: labels derived from the key class (killing the hardcoded
 * "review/test-gen" guess), the supervision flag riding the ONE join, the also-row for a node
 * running worker + supervision at once, the judge lane's honest rolling caption, and the
 * pre-r6 spans surviving only where no real lane exists.
 */

const NOW = 1_756_600_000_000;

describe('supervisionLaneKind mirrors the engine derivation, digit-exact on replan', () => {
  it('classifies each minted key shape', () => {
    expect(supervisionLaneKind('judge-ledgerd-core')).toBe('judge');
    expect(supervisionLaneKind('judge-verify::web')).toBe('judge');
    expect(supervisionLaneKind('replan-r0')).toBe('replan');
    expect(supervisionLaneKind('replan-r12')).toBe('replan');
    expect(supervisionLaneKind('prereview-store-core')).toBe('prereview');
    expect(supervisionLaneKind('tail-review-wiring')).toBe('tailreview');
    // The mirror had drifted: the engine grew schedjudge/verify-N/ask-answer/pillars/reflect and the
    // panel's copy classified none of them — the reflect lane (the post-verdict persona call, r5's
    // 3.5-minute silent tail) fell through to a bare humanized id.
    expect(supervisionLaneKind('schedjudge-web-viz')).toBe('schedjudge');
    expect(supervisionLaneKind('verify-3')).toBe('verify');
    expect(supervisionLaneKind('ask-answer')).toBe('ask');
    expect(supervisionLaneKind('pillars')).toBe('pillars');
    expect(supervisionLaneKind('reflect')).toBe('reflect');
  });

  it('leaves the measured MODEL-chosen worker ids alone — the engine pins these by test', () => {
    expect(supervisionLaneKind('replan-extra')).toBeNull();
    expect(supervisionLaneKind('replan-r2b')).toBeNull();
    expect(supervisionLaneKind('replan-r')).toBeNull();
    expect(supervisionLaneKind('ledgerd-core')).toBeNull();
    expect(supervisionLaneKind('verify::web')).toBeNull();
    // Digit-exact: `verify-endpoints` is a name a plan could give a build task (engine pins the same).
    expect(supervisionLaneKind('verify-endpoints')).toBeNull();
    expect(supervisionLaneKind('verify-')).toBeNull();
  });
});

describe('the judge lane admits it is ROLLING — look N, earlier looks folded', () => {
  it('captions a supervised judge lane from its 1-based look number and folded history', () => {
    expect(
      supervisionRollingCaption({
        taskId: 'judge-ledgerd-core',
        supervision: true,
        attempt: 3,
        superseded: [{ attempt: 1 }, { attempt: 2 }],
      })
    ).toBe('look 3 · earlier looks folded');
    expect(
      supervisionRollingCaption({ taskId: 'judge-ledgerd-core', supervision: true, attempt: 1 })
    ).toBe('look 1');
  });

  it('is OPTIONAL-TOLERANT: no supervision stamp, no attempt, or a non-judge key → no caption', () => {
    // Pre-r6 archived digests carry neither the key class nor the field — every read must tolerate that.
    expect(supervisionRollingCaption({ taskId: 'judge-x', attempt: 2 })).toBeNull();
    expect(
      supervisionRollingCaption({ taskId: 'judge-x', supervision: true })
    ).toBeNull();
    expect(
      supervisionRollingCaption({ taskId: 'replan-r0', supervision: true, attempt: 2 })
    ).toBeNull();
  });
});

describe('deriveFleet mints supervision lanes from their digests, labeled from the key class', () => {
  const judgeDigest = {
    model: 'mihai-qwen3.6-27b',
    supervision: true,
    attempt: 2,
    superseded: [{ attempt: 1, last_text: 'look 1: ok' }],
    thinking_chars: 900,
    full_thinking: 'reading the worker stream against the spec',
  };

  it('a replan-r0 digest becomes a WORKING lane saying Replanning — the misattribution fix', () => {
    const fleet = deriveFleet({
      pool: ['gabee', 'mihai'],
      laneSources: [],
      digests: { 'replan-r0': { model: 'gabee-qwen3.6-27b', supervision: true, thinking_chars: 400 } },
      digestMtimes: { 'replan-r0': NOW },
      now: NOW,
    });
    const lane = fleet.workingByDevice.get('gabee');
    expect(lane?.description).toBe('Replanning · round 0');
    // The flag rides the ONE join — never hand-copied.
    expect(lane?.supervision).toBe(true);
  });

  it('a node running a worker lane AND a judge lane shows BOTH — the second is an also-row', () => {
    const fleet = deriveFleet({
      pool: ['gabee', 'mihai'],
      laneSources: [{ taskId: 'ledgerd-core', device: 'mihai', status: 'running', seq: 0 }],
      digests: { 'judge-ledgerd-core': judgeDigest },
      digestMtimes: { 'judge-ledgerd-core': NOW },
      now: NOW,
    });
    expect(fleet.workingByDevice.get('mihai')?.taskId).toBe('ledgerd-core');
    const also = fleet.alsoRunningByDevice.get('mihai') ?? [];
    expect(also.map((l) => l.taskId)).toEqual(['judge-ledgerd-core']);
    expect(also[0]?.description).toBe('Supervising · ledgerd-core');
    expect(also[0]?.supervision).toBe(true);
  });

  it('a MODEL-chosen replan-extra digest is NOT dressed as supervision (pre-r6 tolerance too)', () => {
    const fleet = deriveFleet({
      pool: ['gabee'],
      laneSources: [],
      digests: { 'replan-extra': { model: 'gabee-qwen3.6-27b', thinking_chars: 10 } },
      digestMtimes: { 'replan-extra': NOW },
      now: NOW,
    });
    const lane = fleet.workingByDevice.get('gabee');
    expect(lane?.description?.startsWith('Replanning')).toBe(false);
    expect(lane?.supervision).toBeUndefined();
  });

  describe('the span pseudo-lane survives ONLY where no real judge lane exists', () => {
    const spanEvents = [
      {
        event: 'judge_look_dispatched',
        task_id: 'ledgerd-core',
        ts: new Date(NOW - 30_000).toISOString(),
      },
    ];

    it('pre-r6 stream (no judge digest): the span still attaches to the busy node', () => {
      const fleet = deriveFleet({
        pool: ['gabee', 'mihai', 'workhorse'],
        laneSources: [],
        digests: {},
        digestMtimes: {},
        now: NOW,
        supervision: foldSupervision(spanEvents),
        busyNodes: ['workhorse'],
      });
      expect(fleet.workingByDevice.get('workhorse')?.phase).toBe('supervision');
    });

    it('r6 stream (judge digest exists): the REAL lane wins; the span mints no guessed twin', () => {
      const fleet = deriveFleet({
        pool: ['gabee', 'mihai', 'workhorse'],
        laneSources: [],
        digests: { 'judge-ledgerd-core': judgeDigest },
        digestMtimes: { 'judge-ledgerd-core': NOW },
        now: NOW,
        supervision: foldSupervision(spanEvents),
        busyNodes: ['workhorse'],
      });
      // The judge digest names ITS node (mihai, from the model); workhorse gets no guessed lane
      // and nothing lands unattributed — the misattribution class, closed.
      expect(fleet.workingByDevice.get('mihai')?.taskId).toBe('judge-ledgerd-core');
      expect(fleet.workingByDevice.has('workhorse')).toBe(false);
      expect(fleet.unattributed).toHaveLength(0);
    });
  });
});

// ── The rendered strip: labels, the also-row, the retired guess string, the inspector caption. ──

const POOL = [
  { id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 },
  { id: 'local-gabee-qwen3.6-27b', model_id: 'gabee-qwen3.6-27b', weight: 2 },
];
// The hook DROPS digests whose mtime predates the run's first event — the fixture clock is relative.
const ts = new Date(Date.now() - 60_000).toISOString();

const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts },
  { event: 'pool_resolved', devices: POOL, worker_count: 2 },
  {
    event: 'task_dispatched',
    ts,
    task_id: 'ledgerd-core',
    device: 'mihai-qwen3.6-27b',
    model: 'mihai-qwen3.6-27b',
  },
];

const ACTIVITY = {
  'ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    thinking_chars: 2000,
    last_thinking: 'writing the ledger core',
    full_thinking: 'writing the ledger core',
  },
  'judge-ledgerd-core': {
    model: 'mihai-qwen3.6-27b',
    supervision: true,
    attempt: 2,
    superseded: [{ attempt: 1, last_text: 'look 1: on track' }],
    thinking_chars: 900,
    full_thinking: 'reading the worker stream against the spec',
  },
  'replan-r0': {
    model: 'gabee-qwen3.6-27b',
    supervision: true,
    thinking_chars: 400,
    full_thinking: 'weighing follow-up tasks on the completed work',
  },
};

const electron = () => (window as unknown as { electron: Record<string, unknown> }).electron;

describe('the fleet strip renders supervision lanes as their own visible class', () => {
  beforeEach(() => {
    resetFoldCache();
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-supervision',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: Object.fromEntries(Object.keys(ACTIVITY).map((k) => [k, Date.now()])),
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

  const mount = async () => {
    const { result } = renderHook(() => useSwarmRun('/tmp/build'));
    await waitFor(() => expect(result.current.present).toBe(true));
    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" run={result.current} />
      </IntlTestWrapper>
    );
  };

  it('labels derive from the key class, and the hardcoded review/test-gen guess is GONE', async () => {
    await mount();
    const cells = await screen.findAllByTestId('fleet-node');
    const gabee = cells.find((c) => c.getAttribute('data-device')?.includes('gabee'));
    expect(gabee?.textContent).toContain('Replanning · round 0');
    expect(document.body.textContent).not.toContain('review/test-gen');
  });

  it('a worker + judge node shows the judge lane as a clickable also-row that opens ITS inspector', async () => {
    await mount();
    const also = await screen.findByTestId('fleet-node-also');
    expect(also.getAttribute('data-task')).toBe('judge-ledgerd-core');
    expect(also.textContent).toContain('Supervising · ledgerd-core');

    fireEvent.click(also);
    const dialog = await screen.findByRole('dialog');
    // The supervision chip and the honest rolling caption — look 2, look 1 folded into superseded.
    expect(await screen.findByTestId('inspector-supervision')).toBeTruthy();
    expect(dialog.textContent).toContain('look 2 · earlier looks folded');
  });
});
