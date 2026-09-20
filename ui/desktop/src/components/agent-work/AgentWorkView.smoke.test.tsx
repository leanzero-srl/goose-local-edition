import { fireEvent, render as renderBase, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import { assertStudioClean } from '../lz/assertStudioClean';
import { foldDesk, type AgentWorkRead, type DeskState } from './agentWorkModel';
import { TickClock } from './TickClock';
import { LaneBoard } from './LaneBoard';
import { NeedsYou } from './NeedsYou';
import { AgentResults } from './AgentResults';
import { LedgerPanel } from './LedgerPanel';

function render(ui: React.ReactNode) {
  return renderBase(<IntlProvider locale="en">{ui}</IntlProvider>);
}

const NOW = Date.parse('2026-09-07T08:00:00Z');

const state: DeskState = {
  status: 'ticking',
  agent: 'web-research',
  title: 'Research desk',
  pid: 7,
  run_id: 'r',
  tick: 4,
  phase: 'lanes',
  phase_started_at: new Date(NOW - 90_000).toISOString(),
  next_tick_at: null,
  next_tick_reason: '',
  next_tick_local: '',
  cadence: '30m',
  timezone: 'Europe/Zurich',
  window_open: true,
  last_tick: {
    tick: 3,
    started_at: '',
    ended_at: '',
    outcome: 'done',
    summary: 'answered DEMO-1',
    lanes: 2,
    staged: 1,
    posted: 0,
    asks: 0,
    lane_secs: 300,
    wall_secs: 400,
  },
  lanes_planned: 1,
  lanes_done: 0,
  hold_reason: null,
  decisions_applied: 0,
  planner_model: 'big-27b',
  devices: [{ id: 'a', model_id: 'small-9b', weight: 2, supervision: false }],
  updated_at: '',
};

const read: AgentWorkRead = {
  dir: '/desks/web-research',
  manifest: {
    name: 'web-research',
    title: 'Research desk',
    cadence: '30m',
    timezone: 'Europe/Zurich',
    window: { days: ['mon', 'fri'], from: '09:00', to: '18:00' },
    post: { command: 'x', approval: 'human' },
  },
  state,
  pid: 7,
  heartbeatMs: NOW - 1000,
  events: [
    {
      event: 'lane_dispatched',
      tick: 4,
      key: 't4-demo-77',
      lane: 'demo-77',
      surgeon: 'access',
      item: 'DEMO-77',
      model: 'small-9b',
    },
  ],
  ticks: [
    {
      tick: 4,
      orient: {
        summary: 'one access item',
        lanes: [
          {
            id: 'demo-77',
            surgeon: 'access',
            item: 'DEMO-77',
            objective: 'who owns the group',
            kind: 'research',
          },
        ],
        asks: [],
        drop: [],
      },
    },
    {
      tick: 3,
      outcome: 'done',
      summary: 'answered DEMO-1',
      lanes: [{}, {}],
      synthesis: { staged: ['d'], asks: [], facts: [], log_line: 'x', handoff: '', pending: [] },
      posted: [],
      lane_secs: 300,
      wall_secs: 400,
    },
  ],
  lanes: {
    't4-demo-77': {
      model: 'small-9b',
      last_thinking: 'the group ugroups says two owners',
      tool_calls: 2,
      forming: [],
    },
  },
  laneMtimes: { 't4-demo-77': NOW - 500 },
  prepared: [
    {
      id: 'web-research-t3-demo-1',
      tick: 3,
      lane: 'demo-1',
      surgeon: 'access',
      target: 'DEMO-1',
      kind: 'comment',
      body: 'I have added you to the group, please retry.',
      evidence: [],
      status: 'staged',
      staged_at: '',
      review: [{ lens: 'voice', verdict: 'PASS', notes: 'plain' }],
    },
  ],
  asks: [
    {
      id: 'ask-t3-1',
      tick: 3,
      question: 'Which of the two groups is the right one?',
      why: 'both grant the space',
      status: 'open',
      raised_at: '',
    },
  ],
  ledger: {
    kinds: {
      tick: [{ tick: 3, lanes: 2, staged: 1, posted: 0, lane_secs: 300 }],
      fact: [{ tick: 3, fact: 'DEMO-1 is owned by IAM' }],
      ask: [{}],
    },
  },
  scratchpad: 'Goal: keep the queue empty',
  pending: '- [ ] ask-t3-1: which group',
  dailyLog: 'Mon 09:30 Europe/Zurich tick 3 — answered DEMO-1',
  engineLog: '',
  now: NOW,
};

describe('Agent Work desk surfaces', () => {
  it('uses the recorded finding for a completed lane while retaining its raw transcript', () => {
    const model = foldDesk(
      {
        ...read,
        events: [...read.events, { event: 'lane_done', tick: 4, key: 't4-demo-77' }],
        ticks: [{ tick: 4, lanes: [{ key: 't4-demo-77', finding: 'The verified finding.' }] }],
      },
      NOW
    )!;
    expect(model.lanes[0].liveLine).toBe('The verified finding.');
    expect(model.lanes[0].thinkingTail).toBe('the group ugroups says two owners');
  });

  it('leads with the source finding, keeps caveats visible and discloses full evidence', () => {
    const model = foldDesk(
      {
        ...read,
        pid: null,
        ticks: [
          {
            tick: 4,
            outcome: 'done',
            lanes: [
              {
                key: 't4-source',
                finding: 'Verified page finding.',
                next_step: 'Exact title remains unverified.',
              },
            ],
            synthesis: {
              source: { mode: 'lane_report', lane: 'source', key: 't4-source' },
              handoff: 'Full source evidence preserved.',
              facts: [],
              asks: [],
              staged: [],
              pending: [],
              log_line: 'Delivered.',
            },
          },
        ],
      },
      NOW
    )!;
    render(
      <IntlProvider locale="en">
        <AgentResults model={model} />
      </IntlProvider>
    );
    expect(screen.getByText('Verified page finding.')).toBeTruthy();
    expect(screen.getByText('Exact title remains unverified.')).toBeTruthy();
    expect(screen.queryByText('Full source evidence preserved.')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Evidence and full report' }));
    expect(screen.getByRole('heading', { name: 'Finding' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Original handoff' }));
    expect(screen.getByText('Full source evidence preserved.')).toBeTruthy();
  });

  it('shows direct delivery as a handoff without inventing a synthesis call', () => {
    const model = foldDesk({ ...read, state: { ...state, phase: 'handoff' } }, NOW)!;
    const { container } = render(
      <TickClock
        model={model}
        busy={false}
        onStart={vi.fn()}
        onStop={vi.fn()}
        onTickNow={vi.fn()}
        onPause={vi.fn()}
      />
    );
    expect(container.querySelector('[data-phase="handoff"]')?.getAttribute('data-state')).toBe(
      'live'
    );
    expect(screen.getByText('Handoff')).toBeTruthy();
    expect(screen.queryByText('Synthesis')).toBeNull();
    expect(model.lanes.some((lane) => lane.kind === 'synthesis')).toBe(false);
  });

  it('the tick clock shows the live tick, the phase ribbon and the controls, studio-clean', () => {
    const model = foldDesk(read, NOW)!;
    const onTickNow = vi.fn();
    const { container } = render(
      <TickClock
        model={model}
        busy={false}
        onStart={vi.fn()}
        onStop={vi.fn()}
        onTickNow={onTickNow}
        onPause={vi.fn()}
      />
    );
    assertStudioClean(container);
    expect(screen.getByTestId('next-tick-countdown').textContent).toBe('tick 4 live');
    const live = container.querySelector('[data-phase="lanes"]');
    expect(live?.getAttribute('data-state')).toBe('live');
    expect(live?.textContent).toContain('1m 30s');
    expect(container.querySelector('[data-phase="orient"]')?.getAttribute('data-state')).toBe(
      'done'
    );
    expect(screen.getByText('Stop')).toBeTruthy();
    expect((screen.getByText('Tick now').closest('button') as HTMLButtonElement).disabled).toBe(
      true
    );
  });

  it('a stopped incomplete attempt is never presented as running or a first assignment', () => {
    const model = foldDesk({ ...read, pid: null, ticks: [] }, NOW)!;
    render(
      <>
        <AgentResults model={model} />
        <LaneBoard model={model} dir={read.dir} selected={null} onSelect={vi.fn()} />
      </>
    );
    expect(screen.getByText('Stopped without a result')).toBeTruthy();
    expect(screen.queryByText('Ready for the first assignment')).toBeNull();
    expect(screen.getByTestId('lane-row').textContent).toContain('Stopped before completion');
    expect(screen.getByLabelText('interrupted')).toBeTruthy();
    expect(screen.queryByLabelText('running')).toBeNull();
  });

  it('the clock counts down when the desk waits', () => {
    const waiting = foldDesk(
      {
        ...read,
        state: {
          ...state,
          status: 'waiting',
          phase: 'idle',
          next_tick_at: new Date(NOW + 754_000).toISOString(),
          next_tick_reason: 'cadence',
          next_tick_local: 'Mon 10:30 Europe/Zurich',
        },
      },
      NOW
    )!;
    render(
      <TickClock
        model={waiting}
        busy={false}
        onStart={vi.fn()}
        onStop={vi.fn()}
        onTickNow={vi.fn()}
        onPause={vi.fn()}
      />
    );
    expect(screen.getByTestId('next-tick-countdown').textContent).toBe('in 12m 34s');
    expect(screen.getByText(/Mon 10:30 Europe\/Zurich — cadence/)).toBeTruthy();
  });

  it('the lane board shows each lane with its node, surgeon and live words, and opens the inspector', () => {
    const model = foldDesk(read, NOW)!;
    const onSelect = vi.fn();
    const { container } = render(
      <LaneBoard model={model} dir="/desks/web-research" selected={null} onSelect={onSelect} />
    );
    assertStudioClean(container);
    const row = screen.getByTestId('lane-row');
    expect(row.textContent).toContain('Surgeon · access');
    expect(row.textContent).toContain('DEMO-77');
    expect(row.textContent).toContain('small-9b');
    expect(screen.getByTestId('lane-live-line').textContent).toBe(
      'the group ugroups says two owners'
    );
    fireEvent.click(row);
    expect(onSelect).toHaveBeenCalledWith('t4-demo-77');
    render(
      <LaneBoard
        model={model}
        dir="/desks/web-research"
        selected="t4-demo-77"
        onSelect={onSelect}
      />
    );
    expect(screen.getByTestId('lane-inspector').textContent).toContain('who owns the group');
    fireEvent.click(screen.getByRole('button', { name: 'thinking' }));
    expect(screen.getByTestId('lane-thinking').textContent).toContain('two owners');
  });

  it('needs-you lists the open ask and the staged draft with decisions that write a decision row', async () => {
    const model = foldDesk(read, NOW)!;
    const onDecide = vi.fn(async () => {});
    const { container } = render(
      <NeedsYou model={model} onDecide={onDecide} requiresApproval hasPostCommand />
    );
    assertStudioClean(container);
    expect(screen.getByTestId('ask-item').textContent).toContain('Which of the two groups');
    expect(screen.getByTestId('draft-item').textContent).toContain('comment → DEMO-1');
    expect(screen.getByTestId('draft-item').textContent).toContain('waits for your approval');
    fireEvent.change(screen.getByLabelText('Answer ask-t3-1'), {
      target: { value: 'the second one' },
    });
    fireEvent.click(screen.getByText('Answer'));
    expect(onDecide).toHaveBeenCalledWith('ask-t3-1', 'reply', 'the second one');
    fireEvent.click(screen.getByText('Approve — post next tick'));
    expect(onDecide).toHaveBeenCalledWith('web-research-t3-demo-1', 'approve', '');
  });

  it('the ledger shows ticks with their cost, facts, drafts and the desk files', () => {
    const model = foldDesk(read, NOW)!;
    const { container } = render(<LedgerPanel model={model} read={read} />);
    assertStudioClean(container);
    expect(container.textContent).toContain('answered DEMO-1');
    expect(container.textContent).toContain('5.0');
    fireEvent.click(screen.getByText('Facts (1)'));
    expect(screen.getByTestId('facts-list').textContent).toContain('DEMO-1 is owned by IAM');
    fireEvent.click(screen.getByText('Scratchpad'));
    expect(container.textContent).toContain('keep the queue empty');
  });
});
