import { render, renderHook, waitFor, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { SwarmRunPanel, fleetNodeState } from './SwarmRunPanel';
import { buildActivity, deviceNoteMap, payloadFields, useSwarmRun } from './useSwarmRun';

/**
 * The fleet-truth event family the fold dropped (`default: break`) until 2026-09-05: a device the
 * engine excluded, a sidecar serving nothing or another alias, a failed residency probe, an admission
 * refusal, a failed model load. Field names are the emit sites' own (swarm_engine.rs 474/812/1653/
 * 1678, swarm.rs 34489/36116). Each renders as a feed line carrying its payload AND as the "why" on
 * the device's fleet row — a device the engine dropped is never silently absent or "idle".
 */
const ts = '2026-09-05T10:00:00Z';
const MIXED_POOL = [
  { id: 'workhorse-27b', model_id: 'workhorse-qwen3.8-27b', weight: 2, engine: 'lmstudio', node: 'workhorse' },
  {
    id: 'workhorse-mlx',
    model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
    weight: 1,
    engine: 'mlx-sidecar',
    node: 'workhorse-mlx',
  },
];
const LMS_ONLY = [MIXED_POOL[0]];

describe('deviceNoteMap — every event lands on the node it is about', () => {
  it('sidecar-device-excluded is an EXCLUDING note on the device id, even though the pool never lists it', () => {
    const notes = deviceNoteMap([
      { event: 'sidecar-device-excluded', id: 'workhorse-mlx', reason: 'engine not registered', ts },
      { event: 'run_started', pool: LMS_ONLY, ts },
      { event: 'pool_resolved', devices: LMS_ONLY },
    ]);
    expect(Object.keys(notes.byDevice)).toEqual(['workhorse-mlx']);
    expect(notes.byDevice['workhorse-mlx'][0]).toMatchObject({
      kind: 'excluded',
      text: 'excluded from the pool — engine not registered',
      at: ts,
    });
    expect(notes.fleet).toEqual([]);
  });

  it('sidecar-unmounted-and-load-disabled notes every listed device; serves-other-alias names both aliases', () => {
    const notes = deviceNoteMap([
      { event: 'sidecar-unmounted-and-load-disabled', devices: ['workhorse-mlx', 'gabee-mlx'] },
      {
        event: 'sidecar-device-serves-other-alias',
        id: 'workhorse-mlx',
        serving: 'leanzero-mlx',
        wanted: 'workhorse-qwen3.5-9b-4bit-mlx',
      },
    ]);
    expect(notes.byDevice['gabee-mlx'].map((n) => n.kind)).toEqual(['unmounted']);
    expect(notes.byDevice['workhorse-mlx'].map((n) => n.kind)).toEqual(['unmounted', 'other-alias']);
    expect(notes.byDevice['workhorse-mlx'][1].text).toBe(
      'excluded — wants workhorse-qwen3.5-9b-4bit-mlx but the mlx-sidecar serves leanzero-mlx'
    );
  });

  it('fleet-probe-failed / fleet-residency-empty land on every node of THAT engine, via the pool\'s `engine` field', () => {
    const notes = deviceNoteMap([
      { event: 'fleet-probe-failed', engine: 'lmstudio', error: 'connect ECONNREFUSED 127.0.0.1:1234' },
      { event: 'run_started', pool: MIXED_POOL },
      { event: 'fleet-residency-empty', engine: 'mlx-sidecar' },
    ]);
    expect(notes.byDevice['workhorse'].map((n) => n.kind)).toEqual(['probe-failed']);
    expect(notes.byDevice['workhorse'][0].text).toBe(
      'residency probe failed on lmstudio — connect ECONNREFUSED 127.0.0.1:1234'
    );
    expect(notes.byDevice['workhorse-mlx'].map((n) => n.kind)).toEqual(['residency-empty']);
  });

  it('a probe failure for an engine with no pool nodes yet is a FLEET note, never dropped', () => {
    const notes = deviceNoteMap([{ event: 'fleet-probe-failed', engine: 'lmstudio', error: 'timeout' }]);
    expect(notes.byDevice).toEqual({});
    expect(notes.fleet.map((n) => n.kind)).toEqual(['probe-failed']);
  });

  it('lm-probe-unauthorized and fleet-slots-snapshot-fallback are fleet-level facts', () => {
    const notes = deviceNoteMap([
      { event: 'lm-probe-unauthorized', host: 'http://gabee.local:1234', token_key: 'LMSTUDIO_API_KEY', token_present: false },
      { event: 'fleet-slots-snapshot-fallback', reason: 'lms ps returned []', snapshot_len: 0 },
    ]);
    expect(notes.fleet.map((n) => n.kind)).toEqual(['probe-unauthorized', 'slots-fallback']);
    expect(notes.fleet[0].text).toBe(
      'LM Studio at http://gabee.local:1234 refused the probe — wants an API token (LMSTUDIO_API_KEY absent)'
    );
    expect(notes.fleet[1].text).toBe('slot snapshot fell back — lms ps returned [] (0 entries)');
  });

  it('sidecar-admission-cap is a past-tense note on the device; lms-load-failed joins the model id through the pool map', () => {
    const notes = deviceNoteMap([
      { event: 'pool_resolved', devices: MIXED_POOL },
      {
        event: 'sidecar-admission-cap',
        device: 'workhorse-mlx',
        model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
        in_flight: 2,
        task_id: 'ledgerd-core',
        attempt: 1,
      },
      { event: 'lms-load-failed', model: 'workhorse-qwen3.8-27b', error: 'not enough memory' },
    ]);
    expect(notes.byDevice['workhorse-mlx'][0]).toMatchObject({
      kind: 'admission-cap',
      text: 'refused admission for ledgerd-core (attempt 1, 2 in flight) — re-dispatched after backoff',
    });
    expect(notes.byDevice['workhorse'][0]).toMatchObject({
      kind: 'load-failed',
      text: 'loading workhorse-qwen3.8-27b failed — not enough memory',
    });
  });

  it('an MLX-only run whose sidecar model id is the digest model files the note under `workhorse-mlx`', () => {
    const notes = deviceNoteMap([
      { event: 'pool_resolved', devices: [MIXED_POOL[1]] },
      { event: 'lms-load-failed', model: 'workhorse-qwen3.5-9b-4bit-mlx', error: 'x' },
    ]);
    expect(Object.keys(notes.byDevice)).toEqual(['workhorse-mlx']);
  });
});

describe('buildActivity — each event is a feed line whose sub IS the payload', () => {
  const START = { event: 'run_started', ts, pool: MIXED_POOL };

  it('renders the nine events with their payload fields', () => {
    const { activity } = buildActivity([
      START,
      { event: 'fleet-probe-failed', engine: 'lmstudio', error: 'connect ECONNREFUSED' },
      { event: 'lm-probe-unauthorized', host: 'http://gabee.local:1234', token_key: 'LMSTUDIO_API_KEY', token_present: true },
      { event: 'sidecar-device-excluded', id: 'workhorse-mlx', reason: 'engine not registered' },
      { event: 'sidecar-unmounted-and-load-disabled', devices: ['workhorse-mlx'] },
      { event: 'sidecar-device-serves-other-alias', id: 'workhorse-mlx', serving: 'leanzero-mlx', wanted: 'wh-mlx' },
      { event: 'sidecar-admission-cap', device: 'workhorse-mlx', model_id: 'wh-mlx', in_flight: 2, task_id: 'ledgerd-core', attempt: 1 },
      { event: 'fleet-residency-empty', engine: 'mlx-sidecar' },
      { event: 'fleet-slots-snapshot-fallback', reason: 'lms ps returned []', snapshot_len: 0 },
      { event: 'lms-load-failed', model: 'workhorse-qwen3.8-27b', error: 'not enough memory' },
    ]);
    const texts = activity.map((r) => r.text);
    expect(texts).toContain('Residency probe failed on lmstudio');
    expect(texts).toContain('LM Studio at http://gabee.local:1234 refused the probe — API token present but refused');
    expect(texts).toContain('workhorse-mlx excluded from the pool — engine not registered');
    expect(texts).toContain('The mlx-sidecar serves nothing and loading is disabled — workhorse-mlx excluded');
    expect(texts).toContain('workhorse-mlx wants wh-mlx but the mlx-sidecar serves leanzero-mlx — excluded');
    expect(texts).toContain(
      'workhorse-mlx refused admission for ledgerd-core (2 in flight) — re-dispatched after backoff'
    );
    expect(texts).toContain('mlx-sidecar reports no resident model');
    expect(texts).toContain('Slot snapshot fell back — lms ps returned []');
    expect(texts).toContain('Loading workhorse-qwen3.8-27b failed');

    const excluded = activity.find((r) => r.text.startsWith('workhorse-mlx excluded'));
    expect(excluded?.tone).toBe('bad');
    expect(excluded?.kind).toBe('fail');
    // The sub is the payload, field by field — what the row's expand and the Clipped reveal open.
    expect(excluded?.sub).toBe('id: workhorse-mlx · reason: engine not registered');
    const cap = activity.find((r) => r.text.includes('refused admission'));
    expect(cap?.kind).toBe('retry');
    expect(cap?.tone).toBe('warn');
    expect(cap?.sub).toBe(
      'device: workhorse-mlx · model_id: wh-mlx · in_flight: 2 · task_id: ledgerd-core · attempt: 1'
    );
    const load = activity.find((r) => r.text.startsWith('Loading'));
    expect(load?.sub).toBe('model: workhorse-qwen3.8-27b · error: not enough memory');
  });

  it('payloadFields drops only the envelope (event/ts/seq) and serialises non-strings', () => {
    expect(payloadFields({ event: 'x', ts, seq: 3, devices: ['a', 'b'], n: 2, ok: false })).toBe(
      'devices: ["a","b"] · n: 2 · ok: false'
    );
  });
});

describe('fleetNodeState — a note that ends participation owns the label; unknown in-flight is not idle', () => {
  const note = (kind: 'excluded' | 'unmounted' | 'other-alias' | 'load-failed' | 'admission-cap' | 'probe-failed') => ({
    kind,
    text: 't',
    seq: 0,
  });
  it('excluding kinds read as an err state, live or not', () => {
    expect(fleetNodeState(undefined, true, undefined, { note: note('excluded') })).toMatchObject({ tone: 'err', label: 'excluded' });
    expect(fleetNodeState(undefined, false, undefined, { note: note('unmounted') })).toMatchObject({ tone: 'err', label: 'unmounted' });
    expect(fleetNodeState(undefined, true, undefined, { note: note('other-alias') })).toMatchObject({ tone: 'err', label: 'wrong alias' });
    expect(fleetNodeState(undefined, true, undefined, { note: note('load-failed') })).toMatchObject({ tone: 'err', label: 'load failed' });
  });
  it('a past-tense note (admission cap, probe failed) leaves the state to the live facts', () => {
    expect(fleetNodeState(undefined, true, undefined, { note: note('admission-cap') })).toMatchObject({ label: 'idle' });
    expect(fleetNodeState(undefined, true, 'generating', { note: note('probe-failed') })).toMatchObject({ label: 'generating' });
  });
  it('a lane beats every note — the engine dispatched there, so it is working', () => {
    const lane = { taskId: 't', device: 'workhorse-mlx', status: 'running' as const, seq: 0 };
    expect(fleetNodeState(lane, true, undefined, { note: note('excluded') })).toMatchObject({ label: 'working' });
  });
  it('busy-unknown is its own warn state, never idle', () => {
    expect(fleetNodeState(undefined, true, undefined, { busyUnknown: true })).toMatchObject({
      tone: 'warn',
      label: 'in flight unknown',
    });
    expect(fleetNodeState(undefined, true, undefined, { busyUnknown: false })).toMatchObject({ label: 'idle' });
  });
});

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

describe('SwarmRunPanel — an excluded device is a fleet row that says why', () => {
  const EVENTS = [
    { event: 'sidecar-device-excluded', id: 'workhorse-mlx', reason: 'engine not registered', ts },
    { event: 'run_started', prompt: '# Build `ledgerd`', pool: LMS_ONLY, ts },
    { event: 'pool_resolved', devices: LMS_ONLY, worker_count: 1 },
    { event: 'lm-probe-unauthorized', host: 'http://gabee.local:1234', token_key: 'LMSTUDIO_API_KEY', token_present: false },
    { event: 'phase', phase: 'open' },
  ];
  beforeEach(() => {
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-excluded',
      dir: '/tmp/excluded',
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

  it('renders the excluded sidecar beside the pool node, labelled from the engine\'s own reason, and the host-level refusal once', async () => {
    const { result, unmount } = renderHook(() => useSwarmRun('/tmp/excluded'));
    await waitFor(() => expect(result.current.present).toBe(true));
    const hostRun = result.current;
    unmount();
    expect(hostRun.deviceNotes.byDevice['workhorse-mlx']?.[0]?.kind).toBe('excluded');

    render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/excluded" run={hostRun} />
      </IntlTestWrapper>
    );
    const rows = await screen.findAllByTestId('fleet-node');
    expect(rows.map((r) => r.getAttribute('data-device'))).toEqual(['workhorse', 'workhorse-mlx']);
    const mlxRow = rows[1];
    const why = mlxRow.querySelector('[data-testid="fleet-node-why"]');
    expect(why?.getAttribute('data-note-kind')).toBe('excluded');
    expect(why).toHaveTextContent('excluded from the pool — engine not registered');
    expect(mlxRow.querySelector('[data-testid="fleet-node-state"]')).toHaveTextContent('excluded');
    // The pool node has no note: no "why", the ordinary idle cell.
    expect(rows[0].querySelector('[data-testid="fleet-node-why"]')).toBeNull();
    // The header counts what the body shows — both rows.
    const fleetCount = screen
      .getAllByTestId('lz-section-count')
      .find((c) => c.closest('header, div')?.textContent?.includes('Fleet'));
    expect(fleetCount?.textContent).toBe('2');
    expect(screen.getByTestId('fleet-notes')).toHaveTextContent(
      'LM Studio at http://gabee.local:1234 refused the probe — wants an API token (LMSTUDIO_API_KEY absent)'
    );
  });
});
