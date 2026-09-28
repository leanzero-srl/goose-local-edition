import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import { TurnWorkingRow } from './TurnWorkingRow';
import { resetTurnReadForTests, usePublishTurnRead } from './turnReadStore';
import { turnProducedNothing } from './turnProducedNothing';
import { promptProgress, promptRead, type PromptRead } from '../leanzero-swarm/engineFigures';
import type { MlxLiveRequest, MlxLiveStats } from '../leanzero-swarm/mlxLiveStats';
import { resetNowForTests } from '../sessionActivity/ActivityPills';
import {
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from '../sessionActivity/sessionActivityStore';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import { PHASE_DOT, PHASE_FILL } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const NOW = Date.parse('2026-09-28T10:22:00Z');
const SESSION = '20260928_portugal';

/**
 * The critic's turn on 3.0.68 (21-live-045.png): the split read a 40.5K-token prompt, 1m 11s in,
 * at 322 tok/s, the card's bar a little past half.
 */
function reading(over: Partial<MlxLiveRequest> = {}): MlxLiveRequest {
  return {
    id: 'turn',
    status: 'running',
    phase: 'prefill',
    elapsedS: 71,
    promptTokens: 40_500,
    completionTokens: 0,
    maxTokens: null,
    tokensPerSecond: null,
    ttftS: null,
    cachedTokens: null,
    prefilledTokens: 22_862,
    promptTps: 322,
    client: null,
    heldForRoom: null,
    stopped: null,
    stoppedAfterS: null,
    leaving: false,
    ...over,
  };
}

function Publish({ read }: { read: PromptRead | null }) {
  usePublishTurnRead(SESSION, read);
  return null;
}

function renderRow(read: PromptRead | null) {
  return render(
    <IntlProvider locale="en" messages={{}}>
      <Publish read={read} />
      <TurnWorkingRow sessionId={SESSION} />
    </IntlProvider>
  );
}

describe('Q-301: the chat says its turn works while nothing is written yet', () => {
  beforeEach(() => {
    resetNowForTests(NOW);
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [], failed: [] });
  });
  afterEach(() => {
    resetTurnReadForTests();
    resetSessionActivityForTests();
  });

  it('the read is the card’s own figures: its bar, its rate, and the time left at that rate', () => {
    const lead = reading();
    const read = promptRead(lead)!;
    const stats = { requests: [lead] } as unknown as MlxLiveStats;
    // The card's bar and the row's are ONE rule.
    expect(read.progress).toEqual(promptProgress(stats, lead));
    expect(read).toEqual({
      tokens: 40_500,
      elapsedS: 71,
      progress: { done: 22_862, total: 40_500 },
      tps: 322,
      leftS: (40_500 - 22_862) / 322,
    });
    // Rapid-MLX's single engine reports neither progress nor a live rate: no bar, no time left.
    expect(promptRead(reading({ prefilledTokens: null, promptTps: null }))).toEqual({
      tokens: 40_500,
      elapsedS: 71,
      progress: null,
      tps: null,
      leftS: null,
    });
    // A request writing, or queued, is not being read.
    expect(promptRead(reading({ phase: 'generation' }))).toBeNull();
    expect(promptRead(reading({ status: 'waiting' }))).toBeNull();
  });

  it('reading: the tokens read of the total, the rate, the time left, and the bar', async () => {
    const { container } = renderRow(promptRead(reading()));
    const row = screen.getByTestId('turn-working-row');
    expect(row.dataset.stage).toBe('reading');
    expect(row.textContent).toContain('Reading your prompt');
    expect(screen.getByTestId('turn-working-figures').textContent).toBe(
      '22.9K of 40.5K tokens read · 322 tok/s · about 55s left at this rate'
    );
    const bar = screen.getByTestId('turn-working-progress');
    expect(bar.getAttribute('aria-valuenow')).toBe('56');
    const fill = bar.firstElementChild as HTMLElement;
    expect(fill.style.width).toBe('56%');
    expect(fill.className).toContain(PHASE_DOT.reading);
    for (const c of PHASE_FILL.reading.split(' ')) {
      expect(row.querySelector('span')!.className).toContain(c);
    }
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('the single engine: the size and how long it has been read — no bar, no time left', () => {
    renderRow(promptRead(reading({ prefilledTokens: null, promptTps: null })));
    expect(screen.getByTestId('turn-working-figures').textContent).toBe(
      '40.5K prompt tokens, reading for 1m 11s'
    );
    expect(screen.queryByTestId('turn-working-progress')).toBeNull();
  });

  it('no prompt being read: how long the turn has waited, from the running state', () => {
    seedSessionActivityForTests({
      running: [
        {
          sessionId: SESSION,
          sessionName: 'Portugal capital question',
          workingDir: '/Users/me',
          startedAt: '2026-09-28T10:20:00Z',
        },
      ],
    });
    renderRow(null);
    const row = screen.getByTestId('turn-working-row');
    expect(row.dataset.stage).toBe('waiting');
    expect(row.textContent).toBe('Waiting for the model’s first words · 2m');
  });

  it('the published read goes when the composer stops publishing it', () => {
    const { rerender } = renderRow(promptRead(reading()));
    expect(screen.getByTestId('turn-working-row').dataset.stage).toBe('reading');
    act(() =>
      rerender(
        <IntlProvider locale="en" messages={{}}>
          <Publish read={null} />
          <TurnWorkingRow sessionId={SESSION} />
        </IntlProvider>
      )
    );
    expect(screen.getByTestId('turn-working-row').dataset.stage).toBe('waiting');
  });

  it('the row lives only while the turn is in flight and nothing of its answer has arrived', () => {
    const user = { role: 'user', created: 0, content: [], metadata: {} } as unknown as Message;
    const assistant = { ...user, role: 'assistant' } as Message;
    expect(turnProducedNothing(ChatState.Thinking, [user])).toBe(true);
    expect(turnProducedNothing(ChatState.Streaming, [assistant, user])).toBe(true);
    expect(turnProducedNothing(ChatState.Streaming, [user, assistant])).toBe(false);
    expect(turnProducedNothing(ChatState.Idle, [user])).toBe(false);
    expect(turnProducedNothing(ChatState.WaitingForUserInput, [user])).toBe(false);
    expect(turnProducedNothing(ChatState.Thinking, [])).toBe(false);
  });
});
