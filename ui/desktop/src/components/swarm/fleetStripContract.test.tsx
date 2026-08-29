import { render, cleanup } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel, { fleetExpandText, fleetThinkingLine } from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE FLEET CELL'S TWO CONTRACTS.
 *
 * 1. ONE definition of what a cell can expand. The rule was written out twice inline — once for the
 *    visible line, once for the expandable text — so the two could disagree about whether a cell has
 *    thinking at all.
 * 2. The cell's ANCHORS. The per-tick frontend instrument (tick_ui.mjs, out of this repo) reads the
 *    RENDERED lane text and the RENDERED clickability off these attributes. Without them it re-derived
 *    both from the IPC payload, and so reported a healthy render path while the renderer dropped every
 *    field it was counting — and it declared a row unclickable whenever the transcript was empty, which
 *    is backwards for a thinking-only model. A rename here breaks that instrument silently, which is what
 *    this test refuses to allow.
 */

const POOL = [
  {
    id: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model_id: 'gabee-qwen3.6-27b-fable-fusion-711',
    weight: 2,
  },
  {
    id: 'local-mihai-qwen3.6-27b-fable-fusi',
    model_id: 'mihai-qwen3.6-27b-fable-fusion-711',
    weight: 2,
  },
  {
    id: 'worksmacstudio-workhorse-qwen3.6-27b',
    model_id: 'workhorse-qwen3.6-27b-fable-fusion-711',
    weight: 2,
  },
];

const EVENTS = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`',
    pool: POOL,
    ts: '2026-08-28T09:00:00.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 3 },
  { event: 'phase', phase: 'research' },
];

// gabee: durable narration, nothing else — expandable on the narration alone.
// mihai: a rolling thinking window the digest never counted — NOT expandable, and the window must not
//        be shown as the live line either.
// workhorse: a counted thinking run — expandable on the thinking line.
const ACTIVITY = {
  'slice-store': {
    model: POOL[0].model_id,
    phase: 'working',
    reasoning: 'the store owns the ledger file',
    full_reasoning: 'the store owns the ledger file and nothing else writes it',
  },
  'slice-api': {
    model: POOL[1].model_id,
    phase: 'working',
    last_thinking: 'a window left behind by a call that already finished',
    thinking_chars: 0,
  },
  synthesis: {
    model: POOL[2].model_id,
    phase: 'working',
    last_thinking: 'wiring the dag',
    thinking_chars: 2400,
  },
};

type ElectronMock = Record<string, unknown>;

describe('fleetExpandText — one definition of what a cell can expand', () => {
  it('expands on the durable narration alone', () => {
    expect(fleetExpandText({ fullReasoning: 'weighing the ledger format' })).toContain('ledger');
  });

  it('shows the thinking window ONLY when the digest counted thinking', () => {
    expect(fleetThinkingLine({ lastThinking: 'stale window', thinkingChars: 0 })).toBe('');
    expect(fleetThinkingLine({ lastThinking: 'live run', thinkingChars: 900 })).toBe('💭 live run');
  });

  it('opens a THINKING-ONLY lane — the case the old detector called dead', () => {
    // `think > 2000 && tx === 0 && ft === 0 => NOT clickable` was the out-of-repo guess. That lane is the
    // one a reader most needs to open, and it opens.
    expect(fleetExpandText({ lastThinking: 'reasoning it out', thinkingChars: 9000 })).toContain(
      'reasoning it out'
    );
  });

  it('is empty for a whitespace-only window, and for no lane at all', () => {
    expect(fleetExpandText({ lastThinking: '   ', thinkingChars: 5000 })).toBe('');
    expect(fleetExpandText(undefined)).toBe('');
  });

  it('joins the narration and the thinking line when both are there', () => {
    const t = fleetExpandText({
      fullReasoning: 'narration',
      lastThinking: 'run',
      thinkingChars: 12,
    });
    expect(t).toBe('narration\n\n💭 run');
  });
});

describe('the fleet cell renders the anchors the frontend tick reads', () => {
  beforeEach(() => {
    // The live line is typewriter-smoothed, so mid-animation it holds a PREFIX of the text. Reduced
    // motion is the same component with the animation off — the assertion is then about the text, not
    // about how far the typewriter happened to get.
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
      runId: 'swarm-test',
      dir: '/tmp/build',
      events: EVENTS,
      activity: ACTIVITY,
      activityMtimes: {
        'slice-store': Date.now(),
        'slice-api': Date.now(),
        synthesis: Date.now(),
      },
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

  it('gives every node a joinable cell whose data-expandable IS the rendered clickability', async () => {
    const { findAllByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const cells = await findAllByTestId('fleet-node');
    const byDevice = new Map(cells.map((c) => [c.getAttribute('data-device'), c]));
    expect([...byDevice.keys()].sort()).toEqual(['gabee', 'mihai', 'workhorse']);

    const gabee = byDevice.get('gabee')!;
    // The join key an instrument needs to compare a cell against its OWN digest, never a neighbour's.
    expect(gabee.getAttribute('data-task')).toBe('slice-store');
    expect(gabee.getAttribute('data-expandable')).toBe('true');
    expect(Number(gabee.getAttribute('data-gen-len'))).toBeGreaterThan(0);
    // data-expandable is the SAME predicate the row's own affordances use.
    expect(gabee.querySelector('[role="button"]')).not.toBeNull();

    const workhorse = byDevice.get('workhorse')!;
    expect(workhorse.getAttribute('data-expandable')).toBe('true');
    expect(workhorse.textContent).toContain('wiring the dag');

    // The uncounted thinking window: no expandable text, so the row is dead — and it says so.
    const mihai = byDevice.get('mihai')!;
    expect(mihai.getAttribute('data-expandable')).toBe('false');
    expect(mihai.getAttribute('data-gen-len')).toBe('0');
    expect(mihai.querySelector('[role="button"]')).toBeNull();
    expect(mihai.textContent).not.toContain('a window left behind');
  });

  it('puts the rendered generation behind its own anchor, so realtime is read off the screen', async () => {
    const { findAllByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const cells = await findAllByTestId('fleet-node');
    const gabee = cells.find((c) => c.getAttribute('data-device') === 'gabee')!;
    const gen = gabee.querySelector('[data-testid="fleet-node-gen"]');
    expect(gen).not.toBeNull();
    expect(gen!.textContent).toContain('ledger');
  });
});
