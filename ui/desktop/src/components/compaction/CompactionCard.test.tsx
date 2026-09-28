import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import type { CompactionStatus } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';

const acp = vi.hoisted(() => ({
  compactionPreview: vi.fn(),
  compactionSteer: vi.fn(),
}));
vi.mock('../../acp/compaction', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/compaction')>()),
  ...acp,
}));

import { CompactionCard } from './CompactionCard';
import { compactingNow, compactionOf } from './compactionStatus';
import { onOpenContextRailRequest } from '../contextRail/contextRailRequest';
import { resetTurnReadForTests, usePublishTurnRead } from '../turnWorking/turnReadStore';
import { promptRead } from '../leanzero-swarm/engineFigures';
import type { MlxLiveRequest } from '../leanzero-swarm/mlxLiveStats';
import type { Message } from '../../types/message';

const SESSION = '20260928_19';

function status(over: Partial<CompactionStatus>): CompactionStatus {
  return {
    stage: 'reading',
    trigger: 'auto',
    tokensBefore: 142_100,
    parts: [],
    partsTotal: 3,
    elapsedMs: 5_000,
    ...over,
  };
}

/** The split reading the compaction request: 141.7K of its 142.1K from the prefix cache. */
function readingRequest(): MlxLiveRequest {
  return {
    id: 'compaction',
    status: 'running',
    phase: 'prefill',
    elapsedS: 12,
    promptTokens: 142_100,
    completionTokens: 0,
    maxTokens: null,
    tokensPerSecond: null,
    ttftS: null,
    cachedTokens: 141_700,
    prefilledTokens: 141_900,
    promptTps: 300,
    client: null,
    heldForRoom: null,
    stopped: null,
    stoppedAfterS: null,
    leaving: false,
  };
}

function Publish() {
  usePublishTurnRead(SESSION, promptRead(readingRequest()));
  return null;
}

function renderCard(s: CompactionStatus, { live = true, publish = false } = {}) {
  const onSend = vi.fn();
  render(
    <>
      {publish && <Publish />}
      <CompactionCard sessionId={SESSION} status={s} live={live} onSend={onSend} />
    </>,
    { wrapper: IntlTestWrapper }
  );
  return { onSend };
}

beforeEach(() => {
  acp.compactionPreview.mockResolvedValue({
    kept: [],
    writtenParts: [],
    alwaysHere: { ledgerTail: [] },
    steer: { note: 'use the 12-month cutoff', standing: false, pins: [], followAsWritten: false },
  });
  acp.compactionSteer.mockImplementation(async (_: string, steer: unknown) => steer);
});

afterEach(() => {
  resetTurnReadForTests();
  vi.clearAllMocks();
});

describe('Q-357: the compaction card', () => {
  it('while reading, shows the split the prefix cache made of the conversation', () => {
    renderCard(status({ stage: 'reading' }), { publish: true });
    const card = screen.getByTestId('compaction-card');
    expect(card).toHaveAttribute('data-stage', 'reading');
    expect(within(card).getByText('Reading the conversation')).toBeInTheDocument();
    expect(screen.getByTestId('compaction-card-figures').textContent).toContain(
      '141.7K from cache'
    );
    expect(screen.getByTestId('compaction-card-read')).toBeInTheDocument();
  });

  it('while reading with no engine read, says the size goose measured and nothing more', () => {
    renderCard(status({ stage: 'reading' }), { live: false });
    expect(screen.getByTestId('compaction-card-figures').textContent).toBe('142.1K tokens · 5s');
    expect(screen.queryByTestId('compaction-card-read')).toBeNull();
  });

  it('while writing, counts tokens, rate and parts', () => {
    renderCard(
      status({
        stage: 'writing',
        writtenTokens: 1_200,
        writingMs: 115_000,
        parts: ['Where we are', 'Next step'],
        elapsedMs: 180_000,
      })
    );
    expect(screen.getByTestId('compaction-card-figures').textContent).toBe(
      '1.2K tokens written · 10.4 tok/s · part 2 of 3 · 3m 0s'
    );
    const bar = screen.getByTestId('compaction-card-parts');
    expect(bar).toHaveAttribute('aria-valuenow', '2');
    expect(bar.querySelectorAll('[data-done="true"]')).toHaveLength(2);
    expect(bar.querySelectorAll('[data-done="false"]')).toHaveLength(1);
  });

  it('when done, says before → after and opens what was kept', () => {
    const opened: string[] = [];
    const off = onOpenContextRailRequest((id) => {
      opened.push(id);
      return true;
    });
    renderCard(
      status({
        stage: 'done',
        tokensAfter: 6_300,
        elapsedMs: 238_000,
        noteVerdict: 'concern',
        said: 'the chat says 24 months',
      })
    );
    expect(screen.getByTestId('compaction-card-figures').textContent).toContain(
      '142.1K → 6.3K tokens'
    );
    expect(screen.getByTestId('compaction-card-verdict').textContent).toBe(
      'Compacted following your note. goose noted: “the chat says 24 months”'
    );
    fireEvent.click(screen.getByTestId('compaction-card-kept'));
    expect(opened).toEqual([SESSION]);
    off();
  });

  it('a summary that did not say how it read the note is said plainly', () => {
    renderCard(status({ stage: 'done', tokensAfter: 6_300, noteVerdict: 'missing' }));
    expect(screen.getByTestId('compaction-card-verdict').textContent).toBe(
      'goose didn’t say whether your note was clear — it was followed as written.'
    );
  });

  it('a question waits for the person; Compact as written follows the note without asking', async () => {
    const { onSend } = renderCard(
      status({
        stage: 'question',
        trigger: 'manual',
        said: 'The chat says 24 months; change it to 12?',
        note: 'use the 12-month cutoff',
      })
    );
    expect(screen.getByTestId('compaction-card-question').textContent).toBe(
      '“The chat says 24 months; change it to 12?”'
    );
    await act(async () => {
      fireEvent.click(screen.getByTestId('compaction-card-as-written'));
    });
    expect(acp.compactionSteer).toHaveBeenCalledWith(
      SESSION,
      expect.objectContaining({ note: 'use the 12-month cutoff', followAsWritten: true })
    );
    expect(onSend).toHaveBeenCalledWith('/compact');
  });

  it('Cancel leaves the conversation as it is', () => {
    const { onSend } = renderCard(status({ stage: 'question', said: '12 or 24?' }));
    fireEvent.click(screen.getByTestId('compaction-card-cancel'));
    expect(screen.getByTestId('compaction-card')).toHaveAttribute('data-stage', 'cancelled');
    expect(onSend).not.toHaveBeenCalled();
  });

  it('a failure says the conversation is unchanged and offers to try again', () => {
    const { onSend } = renderCard(status({ stage: 'failed', error: 'the engine went away' }));
    expect(screen.getByTestId('compaction-card-error').textContent).toBe(
      'the engine went away. Your conversation is unchanged.'
    );
    fireEvent.click(screen.getByTestId('compaction-card-retry'));
    expect(onSend).toHaveBeenCalledWith('/compact');
  });
});

describe('compactionOf / compactingNow', () => {
  const card = (stage: string): Message => ({
    id: 'acp_status_x',
    role: 'assistant',
    created: 0,
    content: [
      {
        type: 'systemNotification',
        notificationType: stage === 'done' ? 'inlineMessage' : 'thinkingMessage',
        msg: '',
        data: { kind: 'compaction', ...status({ stage: stage as CompactionStatus['stage'] }) },
      },
    ],
    metadata: { userVisible: true, agentVisible: false },
  });

  it('reads the status a card carries and whether the chat is compacting now', () => {
    expect(compactionOf(card('writing'))?.stage).toBe('writing');
    expect(compactingNow([card('writing')])).toBe(true);
    expect(compactingNow([card('writing'), card('done')])).toBe(false);
    const forming: Message = { ...card('writing'), content: [] };
    expect(compactionOf(forming)).toBeUndefined();
  });
});
