import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';

const acp = vi.hoisted(() => ({ compactionPreview: vi.fn(), compactionSteer: vi.fn() }));
vi.mock('../../acp/compaction', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/compaction')>()),
  ...acp,
}));

import SessionRail from '../session-rail/SessionRail';
import { requestOpenContextRail } from './contextRailRequest';

/** A preview shaped like E2E #3p's: the person's words, the files, a failed call, the ledger. */
const preview = {
  kept: [
    {
      id: 'asked',
      items: ['inactive = no login in 24 months', 'a project lead is never skipped'],
      tokens: 40,
    },
    {
      id: 'files',
      items: ['/w/plan/identity-plan.js — created, +347 −164 lines, 6 writes'],
      tokens: 30,
    },
    { id: 'failed', items: [], tokens: 0, leftOut: 2 },
    { id: 'notes', items: [], tokens: 0 },
    { id: 'ledger', items: [], error: 'Permission denied (os error 13)' },
  ],
  writtenParts: [
    { heading: 'Where we are', ask: 'What is done and in progress.' },
    { heading: 'Next step', ask: 'The one next step.' },
    { heading: 'Decisions and reasons', ask: 'Each decision and why.' },
  ],
  alwaysHere: {
    scratchpad: '- [x] plan the users',
    ledgerTail: ['2026-09-28 12:20 [fact] 10 pass'],
  },
  steer: { note: 'keep the lead rule', standing: false, pins: ['seed 20260928'] },
  last: {
    at: '2026-09-28T09:32:46Z',
    trigger: 'auto',
    tokensBefore: 142_100,
    tokensAfter: 6_300,
    elapsedMs: 238_000,
  },
  lastKept: '## Where we are\nplanning\n\n<kept-by-goose>…</kept-by-goose>',
  keptBudgetTokens: 11_136,
};

function renderRail(onSend = vi.fn()) {
  render(
    <SessionRail
      sessionId="s1"
      messages={[]}
      loop={{ kind: 'none' }}
      control={async () => ({ kind: 'failed', error: 'none' })}
      onPillsHeight={() => undefined}
      onSend={onSend}
    />,
    { wrapper: IntlTestWrapper }
  );
  return onSend;
}

beforeEach(() => {
  acp.compactionPreview.mockResolvedValue(preview);
  acp.compactionSteer.mockImplementation(async (_: string, steer: unknown) => steer);
});

afterEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
});

describe('Q-357: the rail’s Context tab', () => {
  it('opens on request, even with no loop and no changes, and shows what a compaction keeps', async () => {
    renderRail();
    expect(screen.queryByTestId('session-rail-panel')).toBeNull();
    act(() => {
      expect(requestOpenContextRail('s1')).toBe(true);
    });
    const panel = await screen.findByTestId('context-rail-panel');
    expect(screen.getByTestId('rail-tab-context')).toHaveAttribute('aria-selected', 'true');

    const asked = within(panel).getByTestId('context-pillar-asked');
    expect(asked.textContent).toContain('2 items');
    fireEvent.click(within(asked).getByRole('button', { name: /What you asked/ }));
    expect(within(asked).getByText('inactive = no login in 24 months')).toBeVisible();

    expect(within(panel).getByTestId('context-pillar-failed').textContent).toContain(
      '2 older ones are not kept here to fit'
    );
    expect(within(panel).getByTestId('context-pillar-ledger').textContent).toContain(
      'Could not be read: Permission denied (os error 13)'
    );
    expect(
      within(panel).getByText('Kept within 11.1K tokens — a sixteenth of the window')
    ).toBeTruthy();
    expect(within(panel).getByTestId('context-rail-last').textContent).toContain(
      '142.1K → 6.3K tokens · 3m 58s'
    );
    expect(within(panel).getByText('- [x] plan the users')).toBeTruthy();
    expect(within(panel).getByDisplayValue('keep the lead rule')).toBeTruthy();
    expect(within(panel).getByText('seed 20260928')).toBeTruthy();
  });

  it('pins and unpins through goosed, keeping the note', async () => {
    renderRail();
    act(() => {
      requestOpenContextRail('s1');
    });
    await screen.findByTestId('context-rail-panel');
    await screen.findByDisplayValue('keep the lead rule');
    fireEvent.change(screen.getByTestId('context-rail-pin-input'), {
      target: { value: 'svc-edi has no user row' },
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId('context-rail-add-pin'));
    });
    expect(acp.compactionSteer).toHaveBeenLastCalledWith(
      's1',
      expect.objectContaining({
        note: 'keep the lead rule',
        pins: ['seed 20260928', 'svc-edi has no user row'],
      })
    );
  });

  it('Compact now saves an edited note first, then compacts in the chat', async () => {
    const onSend = renderRail();
    act(() => {
      requestOpenContextRail('s1');
    });
    const note = await screen.findByDisplayValue('keep the lead rule');
    fireEvent.change(note, { target: { value: 'keep the 24-month rule' } });
    await act(async () => {
      fireEvent.click(screen.getByTestId('context-rail-compact'));
    });
    expect(acp.compactionSteer).toHaveBeenCalledWith(
      's1',
      expect.objectContaining({ note: 'keep the 24-month rule' })
    );
    expect(onSend).toHaveBeenCalledWith('/compact');
  });

  it('a preview that fails is said, never an empty tab', async () => {
    acp.compactionPreview.mockRejectedValue(new Error('goosed is not running'));
    renderRail();
    act(() => {
      requestOpenContextRail('s1');
    });
    expect((await screen.findByTestId('context-rail-failed')).textContent).toBe(
      'Couldn’t read what a compaction would keep: goosed is not running'
    );
  });
});
