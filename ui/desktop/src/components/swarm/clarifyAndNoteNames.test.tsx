import { render, cleanup, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * EVERY INPUT HAS A NAME THAT SURVIVES TYPING, AND EVERY DISABLED BUTTON SAYS WHY.
 *
 * Measured on the live app over CDP (2026-08-29): the clarify prompt's answer boxes were named only by
 * their placeholder "your answer…" — the weakest accessible name there is, and one that vanishes on the
 * first keystroke, so three questions had three boxes named nothing. The guidance textarea had the same
 * defect, and both Send buttons (the clarify one and the mid-build note one) were disabled with no
 * `title` and no `aria-describedby` — a dead control with no reason on it.
 */

const POOL = [{ id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 }];

const EVENTS = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`\n\nA small operations tool.',
    pool: POOL,
    ts: '2026-08-17T13:54:13.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 1 },
  { event: 'phase', phase: 'open' },
];

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const describedBy = (el: HTMLElement): string => {
  const id = el.getAttribute('aria-describedby');
  expect(id).toBeTruthy();
  return document.getElementById(id!)?.textContent ?? '';
};

function mockRun(clarify: unknown) {
  const e = electron();
  e.readSwarmRun = vi.fn(async () => ({
    runId: 'swarm-names',
    dir: '/tmp/build',
    events: EVENTS,
    activity: {},
    activityMtimes: {},
    clarify,
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
}

describe('the clarify prompt — each answer box is named by its question', () => {
  beforeEach(() =>
    mockRun({
      pending: true,
      questions: [
        {
          question: 'Which storage backend should the ledger use?',
          options: ['sqlite', 'postgres'],
        },
        { question: 'Ship a CLI alongside the web UI?', options: [] },
      ],
      answerPath: '/tmp/build/.swarm/answers.json',
    })
  );
  afterEach(() => cleanup());

  it('names every answer input with its question, not its placeholder', async () => {
    const { findByRole, getByRole } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const first = await findByRole('textbox', {
      name: /Which storage backend should the ledger use/,
    });
    const second = getByRole('textbox', { name: /Ship a CLI alongside the web UI/ });
    expect(first.tagName).toBe('INPUT');
    expect(second.tagName).toBe('INPUT');
    // The name must not be the placeholder: it has to still be there after the user types.
    fireEvent.change(second, { target: { value: 'yes' } });
    expect(getByRole('textbox', { name: /Ship a CLI alongside the web UI/ })).toBe(second);
  });

  it('labels the guidance textarea visibly', async () => {
    const { findByRole } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const guidance = await findByRole('textbox', { name: /Anything else\? \(optional\)/ });
    expect(guidance.tagName).toBe('TEXTAREA');
  });

  it('says why "Send answers & build" is disabled, and drops the reason once it can send', async () => {
    const { findByRole } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const send = await findByRole('button', { name: /Send answers/ });
    expect(send).toBeDisabled();
    expect(send.getAttribute('title')).toMatch(/Type an answer/);
    expect(describedBy(send)).toMatch(/Type an answer to at least one question/);

    const box = await findByRole('textbox', { name: /Ship a CLI alongside the web UI/ });
    fireEvent.change(box, { target: { value: 'yes, a thin one' } });
    expect(send).not.toBeDisabled();
    expect(send.getAttribute('title')).toMatch(/Send these answers/);
    expect(describedBy(send)).not.toMatch(/Type an answer/);
  });
});

describe('the mid-build note box — a disabled Send says why', () => {
  beforeEach(() => mockRun(null));
  afterEach(() => cleanup());

  it('carries "Type a note to send" as title and description until there is a note', async () => {
    const { findByRole, getByLabelText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const send = await findByRole('button', { name: 'Send' });
    expect(send).toBeDisabled();
    expect(send.getAttribute('title')).toBe('Type a note to send');
    expect(describedBy(send)).toBe('Type a note to send');

    fireEvent.change(getByLabelText('Add a note to this build'), {
      target: { value: 'use SQLite' },
    });
    expect(send).not.toBeDisabled();
    expect(send.getAttribute('title')).toMatch(/Send this note/);
  });
});
