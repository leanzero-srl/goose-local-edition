import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { SwarmRunPanel } from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * The stale-truth batch of the frontend truth review (findings 7, 8, 10, 12, 14, 15): what the panel
 * says when the ENGINE is gone must stop contradicting the heartbeat, and the states added by the
 * event-coverage batch must actually render. Every case drives the real panel through the poller mock
 * with heartbeat facts, never with a hand-built half-state.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];
const TS = '2026-08-17T13:54:13.000000+00:00';

const BASE_EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: TS },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
  {
    event: 'plan_loaded',
    task_count: 1,
    tasks: [{ id: 'store', description: 'Build the store', files: ['store.py'], deps: [] }],
  },
  { event: 'task_dispatched', task_id: 'store', device: POOL[0].id, model: POOL[0].model_id, ts: TS },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

type PayloadOver = {
  events?: Array<Record<string, unknown>>;
  heartbeat?: number | null;
  heartbeatExited?: boolean;
  pauseRequested?: boolean;
  clarify?: Record<string, unknown> | null;
};

const mockRun = (over: PayloadOver = {}) => {
  electron().readSwarmRun = vi.fn(async () => ({
    runId: 'stale-truth',
    dir: '/tmp/build',
    events: over.events ?? BASE_EVENTS,
    activity: {},
    activityMtimes: {},
    clarify: over.clarify ?? null,
    mtime: Date.now(),
    heartbeat: over.heartbeat === undefined ? Date.now() : over.heartbeat,
    heartbeatExited: over.heartbeatExited ?? false,
    pauseRequested: over.pauseRequested ?? false,
  }));
};

const mount = () =>
  render(
    <IntlTestWrapper>
      <SwarmRunPanel workingDir="/tmp/build" />
    </IntlTestWrapper>
  );

beforeEach(() => {
  const e = electron();
  e.fleetStatus = vi.fn(async () => ({}));
  e.swarmSetPaused = vi.fn(async () => true);
  e.swarmAddNote = vi.fn(async () => true);
  e.writeFile = vi.fn(async () => true);
  e.onSwarmDelta = vi.fn(() => () => {});
});

describe('finding 15 — the EXITED stamp finally ends the run on screen', () => {
  it("renders the 'Run stopped' terminal banner off the exit stamp, absorbing the warning banner", async () => {
    mockRun({ heartbeat: Date.now() - 10_000, heartbeatExited: true });
    mount();
    expect(await screen.findByText('Run stopped')).toBeInTheDocument();
    expect(
      screen.getByText(/The engine exited without a completion signal/)
    ).toBeInTheDocument();
    // The liveness WARNING keys on the same state — with ended true it yields to the terminal banner.
    expect(screen.queryByText(/Everything below is what it had reached/)).toBeNull();
  });

  it("a SILENT (hard-killed, resumable) engine stays a warning — no clock may end a run", async () => {
    mockRun({ heartbeat: Date.now() - 60_000 });
    mount();
    expect(await screen.findByText(/No heartbeat for/)).toBeInTheDocument();
    expect(screen.queryByText('Run stopped')).toBeNull();
  });
});

describe('finding 12 — clarify surfaces die with the engine', () => {
  const pendingClarify = {
    pending: true,
    questions: [{ question: 'which storage backend?', options: ['sqlite', 'postgres'] }],
    answerPath: '/tmp/build/.swarm/clarify-answers.json',
    mtime: null,
  };

  it("a dead engine mid-ask shows the interrupted notice, never 'Waiting for you' over a corpse", async () => {
    mockRun({ clarify: pendingClarify, heartbeat: Date.now() - 60_000 });
    mount();
    expect(await screen.findByTestId('clarify-interrupted')).toBeInTheDocument();
    expect(screen.queryByText('Waiting for you')).toBeNull();
    // The interactive form must not mount — answering a dead run is the lie this closes.
    expect(screen.queryByText('Send answers & build')).toBeNull();
  });

  it('a LIVE engine mid-ask keeps the chip and the form', async () => {
    mockRun({ clarify: pendingClarify });
    mount();
    expect(await screen.findByText('Waiting for you')).toBeInTheDocument();
    expect(screen.queryByTestId('clarify-interrupted')).toBeNull();
  });
});

describe('finding 14 — pause surfaces stop asserting live progress on a dead engine', () => {
  it("a stale pending pause reads 'Pause requested', never a spinning 'Pausing…'", async () => {
    mockRun({ pauseRequested: true, heartbeat: Date.now() - 60_000 });
    mount();
    expect(await screen.findByText('Pause requested')).toBeInTheDocument();
    expect(screen.queryByText('Pausing…')).toBeNull();
  });

  it("a held run whose engine died reads 'Paused — engine gone', not 'press ▶ to resume'", async () => {
    mockRun({
      events: [...BASE_EVENTS, { event: 'run_paused' }],
      pauseRequested: true,
      heartbeat: Date.now() - 60_000,
    });
    mount();
    expect(await screen.findByText('Paused — engine gone')).toBeInTheDocument();
  });

  it('a live held run keeps the resume promise', async () => {
    mockRun({ events: [...BASE_EVENTS, { event: 'run_paused' }], pauseRequested: true });
    mount();
    expect(await screen.findByText('Paused')).toBeInTheDocument();
    expect(await screen.findByText('Resume')).toBeInTheDocument();
    expect(screen.queryByText('Paused — engine gone')).toBeNull();
  });
});

describe('finding 7 — a resumed run wears its badge', () => {
  it('renders the solid Resumed chip off run_resumed', async () => {
    mockRun({
      events: [
        BASE_EVENTS[0],
        { event: 'run_resumed', tasks: 7, previously_completed: 3, detail: 'reused the previous plan' },
        ...BASE_EVENTS.slice(1),
      ],
    });
    mount();
    expect(await screen.findByTestId('run-resumed-chip')).toBeInTheDocument();
  });
});

describe('finding 10 — the Q&A card renders swarm answers', () => {
  it('shows the question, the answer and the answering model', async () => {
    mockRun({
      events: [
        ...BASE_EVENTS,
        {
          event: 'swarm_answer',
          question_file: 'q1.txt',
          question: 'which port does the vendor sim use?',
          answer: '8850',
          model: 'mihai-qwen3.6-27b',
        },
      ],
    });
    mount();
    expect(await screen.findByTestId('swarm-qa')).toBeInTheDocument();
    expect(screen.getByText('which port does the vendor sim use?')).toBeInTheDocument();
    expect(screen.getByText('8850')).toBeInTheDocument();
  });
});

describe('finding 8 — a re-streamed lane carries its cause on the board row', () => {
  it('renders the re-streamed chip from the judge_restream carry', async () => {
    mockRun({
      events: [
        ...BASE_EVENTS,
        {
          event: 'judge_restream',
          task_id: 'store',
          nudge: 2,
          reason: 'delivery defect',
          abandoned_thinking_chars: 12000,
          abandoned_tool_calls: 1,
          established_chars: 400,
        },
      ],
    });
    mount();
    expect(await screen.findByTestId('lane-restreamed')).toBeInTheDocument();
    expect(screen.getByText('re-streamed ×1')).toBeInTheDocument();
  });
});
