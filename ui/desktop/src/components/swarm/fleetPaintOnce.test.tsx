import { render, cleanup, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel, { laneSiblingTitle } from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * EACH FACT IS PAINTED ONCE.
 *
 * Measured on the live app over CDP (2026-08-29): "Coverage N · what the request names that nothing owns"
 * was painted twice per coverage lane — in full on the WORK board's row, and again truncated into 40% of
 * the compact sibling line under the node's fleet cell — and "— on an idle node (the verdict names it
 * when it lands)" four times in a column, once under every unattributed judge span. The board keeps the
 * caption; the fleet keeps the identity; the supervision caption is painted once for the group.
 *
 * AND THE SIBLING ROW IS A CONTROL. Measured r1 t+20m: the primary cell opened the node inspector and the
 * row under it did not, so the run's largest lane (open-coverage-1, 23,975 reasoning chars, under gabee's
 * cell) could not be opened at all. The row now opens the inspector on ITS task, by mouse and keyboard.
 */

const POOL = [
  { id: 'mac-gabee-qwen3.6-27b', model_id: 'gabee-qwen3.6-27b', weight: 2 },
  { id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 },
];

const NOW_ISO = new Date().toISOString();

const EVENTS = [
  { event: 'run_started', prompt: '# Build `vendorsync`', pool: POOL, ts: NOW_ISO },
  { event: 'pool_resolved', devices: POOL, worker_count: 2 },
  { event: 'phase', phase: 'open' },
  // Two open judge spans; LM Studio reports no node busy, so neither can be pinned and both are unattributed.
  { event: 'judge_observed', task_id: 'store', ts: NOW_ISO },
  { event: 'judge_observed', task_id: 'api', ts: NOW_ISO },
];

const COVERAGE_LABEL = 'Coverage 1 · what the request names that nothing owns';

// gabee runs a slice AND a coverage lane (PARALLEL: 2): the slice is its cell, the coverage lane is the
// compact sibling line under it.
// Both lanes carry DURABLE text (a narration log, a counted thinking run), which is what makes a cell
// expandable — the measured coverage lane was thinking-only, 23,975 chars, and had to open.
const ACTIVITY = {
  'slice-store': {
    model: POOL[0].model_id,
    phase: 'working',
    reasoning: 'the store owns the ledger',
    full_reasoning: 'the store owns the ledger file and nothing else writes it',
  },
  'open-coverage-1': {
    model: POOL[0].model_id,
    phase: 'working',
    thinking_chars: 23975,
    last_thinking: 'nothing owns the export command yet',
    full_thinking: 'walking the request for names\nnothing owns the export command yet',
  },
};

type ElectronMock = Record<string, unknown>;

describe('laneSiblingTitle — the identity without the board caption', () => {
  it('cuts a planning digest label at its caption', () => {
    expect(laneSiblingTitle({ taskId: 'open-coverage-1', description: COVERAGE_LABEL })).toBe(
      'Coverage 1'
    );
    expect(
      laneSiblingTitle({
        taskId: 'review-2',
        description: 'Review 2 · this part of the request against the whole plan',
      })
    ).toBe('Review 2');
    expect(
      laneSiblingTitle({
        taskId: 'synthesis',
        description: 'Synthesis · wiring the slices into a task DAG',
      })
    ).toBe('Synthesis');
  });

  it('leaves every other lane title whole — the separator is not an identity marker there', () => {
    expect(laneSiblingTitle({ taskId: 'slice-api', description: 'Slice · api' })).toBe(
      'Slice · api'
    );
    expect(laneSiblingTitle({ taskId: 'store', description: 'Build the store' })).toBe(
      'Build the store'
    );
    expect(laneSiblingTitle({ taskId: 'store' })).toBe('store');
  });
});

describe('the fleet strip paints each fact once', () => {
  beforeEach(() => {
    window.matchMedia = ((q: string) => ({
      matches: q.includes('prefers-reduced-motion'),
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      onchange: null,
      dispatchEvent: () => false,
    })) as unknown as typeof window.matchMedia;
    const electron = (window as unknown as { electron: ElectronMock }).electron;
    electron.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-once',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: { 'slice-store': Date.now(), 'open-coverage-1': Date.now() },
      clarify: null,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    electron.fleetStatus = vi.fn(async () => ({}));
    electron.swarmSetPaused = vi.fn(async () => true);
    electron.swarmAddNote = vi.fn(async () => true);
    electron.revealInFinder = vi.fn(async () => undefined);
    electron.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('names the sibling lane by its identity and leaves the caption to the board row', async () => {
    const { findByTestId, getAllByText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const also = await findByTestId('fleet-node-also');
    expect(also.getAttribute('data-task')).toBe('open-coverage-1');
    expect(also.textContent).toContain('Coverage 1');
    expect(also.textContent).not.toContain('what the request names');
    // The full label survives exactly once: the board row, where it has the width.
    expect(getAllByText(COVERAGE_LABEL)).toHaveLength(1);
  });

  it('paints the unattributed-supervision caption once for all spans', async () => {
    const { findByTestId, queryAllByText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const line = await findByTestId('fleet-unattributed');
    expect(line.textContent).toContain('Judging · store');
    expect(line.textContent).toContain('Judging · api');
    expect(
      queryAllByText(/on idle nodes \(each verdict names its node when it lands\)/)
    ).toHaveLength(1);
    expect(queryAllByText(/on an idle node \(the verdict names it when it lands\)/)).toHaveLength(
      0
    );
  });

  it('opens the inspector on the sibling lane, not the primary, from the sibling row', async () => {
    const { findByTestId, findByRole, getByRole, getByLabelText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const also = await findByTestId('fleet-node-also');
    // The same anchors the per-tick instrument reads off the primary cell, computed the same way.
    expect(also.getAttribute('data-expandable')).toBe('true');
    expect(Number(also.getAttribute('data-gen-len'))).toBeGreaterThan(0);
    // Its own control, named by its lane — and NOT nested inside the primary cell's button.
    const control = getByRole('button', { name: /Open the full stream of Coverage 1 on gabee/ });
    expect(control).toBe(also);
    expect(also.parentElement?.closest('[role="button"]')).toBeNull();

    fireEvent.click(also);
    const dialog = await findByRole('dialog');
    expect(dialog.getAttribute('data-task')).toBe('open-coverage-1');
    fireEvent.click(getByLabelText('Close'));

    // The primary cell still opens ITS lane.
    fireEvent.click(getByRole('button', { name: /Open the full stream from gabee/ }));
    expect((await findByRole('dialog')).getAttribute('data-task')).toBe('slice-store');
    fireEvent.click(getByLabelText('Close'));

    // Keyboard: Enter on the focused sibling row.
    fireEvent.keyDown(also, { key: 'Enter' });
    expect((await findByRole('dialog')).getAttribute('data-task')).toBe('open-coverage-1');
  });
});
