import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { createIntl } from 'react-intl';
import type { FormingStatus } from '@aaif/goose-sdk';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import type { MlxLiveRequest } from '../leanzero-swarm/mlxLiveStats';
import type { ChatServedBy } from './chatServedBy';
import { pickTurnCue, writingProgress } from './turnStatus';
import { formingOf, turnLine } from './turnLine';
import LoadingGoose from '../LoadingGoose';
import { IntlTestWrapper } from '../../i18n/test-utils';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });
const MACS = ['Mihai Macbook', "Work's Mac Studio"];

/**
 * Round live-1 (3.0.52, 23:13): the 27B split writing session 20260926_19's answer — /v1/status
 * said 24,228 tokens, 2,355 s since arrival, 11 tok/s; the chat had received 41 tool calls, the
 * latest to ledger__ledger_append, 18.5k chars of arguments and 120 chars of text beside them.
 */
const WRITING: MlxLiveRequest = {
  id: 'chatcmpl-live1',
  status: 'running',
  phase: 'generation',
  elapsedS: 2355,
  promptTokens: 39996,
  completionTokens: 24228,
  maxTokens: 222148,
  tokensPerSecond: 11.02,
  ttftS: 133,
  cachedTokens: 0,
  prefilledTokens: null,
  promptTps: null,
};

const served = (over: Partial<ChatServedBy> = {}): ChatServedBy => ({
  engine: 'split',
  model: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  where: MACS,
  peerNodeId: null,
  foreign: false,
  contextWindow: 262144,
  phase: 'writing',
  activity: 'generating',
  work: 'thisChat',
  busyWithOthers: null,
  busyIn: null,
  turnRequest: WRITING,
  readTps: null,
  readiness: { kind: 'ready' },
  ...over,
});

const TEXT =
  'Now I have the decisions. Let me append each one to the ledger, then write the notes.';
const FORMING: FormingStatus = {
  calls: [
    ...Array.from({ length: 40 }, () => ({
      name: 'developer__text_editor',
      title: 'developer: text editor',
      argumentChars: 400,
    })),
    { name: 'ledger__ledger_append', title: 'ledger: ledger append', argumentChars: 2500 },
  ],
  argumentChars: 18500,
  reasoningChars: 0,
  text: TEXT,
};
const SERVER_LINE =
  'goose is writing 41 tool calls, the latest to ledger: ledger append — 18.5k chars of arguments, 85 chars of text beside the calls';

const progressMessage = (forming: FormingStatus | undefined): Message => ({
  id: 'acp_status_s_1',
  role: 'assistant',
  created: 0,
  content: [
    {
      type: 'systemNotification',
      notificationType: 'thinkingMessage',
      msg: SERVER_LINE,
      ...(forming ? { data: forming } : {}),
    },
  ],
  metadata: { userVisible: true, agentVisible: false },
});

describe('Q-151: the status line leads with the engine’s own time, tokens and rate', () => {
  it('the writing cue is the live request: elapsed since arrival, tokens generated, decode rate', () => {
    expect(writingProgress(WRITING)).toEqual({ elapsedS: 2355, tokens: 24228, tps: 11.02 });
    const cue = pickTurnCue({ served: served(), inFlight: true, lostTo: null, silent: false });
    expect(cue).toMatchObject({ kind: 'writing', tokens: 24228, elapsedS: 2355 });
  });

  it('no rate before two tokens (a rate needs an interval) and no cue while the request waits', () => {
    expect(writingProgress({ ...WRITING, completionTokens: 1, tokensPerSecond: 1048576 })).toEqual({
      elapsedS: 2355,
      tokens: 1,
      tps: null,
    });
    expect(writingProgress({ ...WRITING, status: 'waiting' })).toBeNull();
    expect(writingProgress({ ...WRITING, phase: 'prefill' })).toBeNull();
  });

  it('the round’s words: time, tokens and rate first, then the calls by the chat’s tool names', () => {
    const cue = pickTurnCue({ served: served(), inFlight: true, lostTo: null, silent: false });
    const line = turnLine(intl, ChatState.Streaming, false, cue, progressMessage(FORMING));
    expect(line.message).toBe(
      'Writing for 39m 15s · 24k tokens · 11.0 tok/s — 41 tool calls, the latest to ledger: ledger append'
    );
    expect(line.message).not.toContain('ledger__ledger_append');
    expect(line.message).not.toContain('chars');
    expect(line.forming).toBe(FORMING);
  });

  it('with no engine read (a cloud provider), goose’s own line — and the list stays reachable', () => {
    const line = turnLine(intl, ChatState.Streaming, false, null, progressMessage(FORMING));
    expect(line.message).toBe(SERVER_LINE);
    expect(line.forming).toBe(FORMING);
  });

  it('a held swarm outranks the cue; a status with no forming data lists nothing', () => {
    const cue = pickTurnCue({ served: served(), inFlight: true, lostTo: null, silent: false });
    expect(turnLine(intl, ChatState.Streaming, true, cue, progressMessage(FORMING))).toEqual({
      message: 'swarm paused — nothing is running until you resume',
      forming: null,
    });
    expect(formingOf(progressMessage(undefined))).toBeNull();
    expect(
      turnLine(intl, ChatState.Streaming, false, cue, progressMessage(undefined)).message
    ).toBe('Writing for 39m 15s · 24k tokens · 11.0 tok/s');
  });
});

describe('Q-158: opening a chat says one neutral word until the first read', () => {
  it('"checking this chat…" — whatever the last message or the engine held', () => {
    const cue = pickTurnCue({ served: served(), inFlight: true, lostTo: null, silent: false });
    expect(
      turnLine(intl, ChatState.LoadingConversation, false, cue, progressMessage(FORMING))
    ).toEqual({ message: 'checking this chat…', forming: null });
  });
});

describe('Q-151: the forming calls and the text beside them are one click away', () => {
  it('the disclosure lists every call with its size, and the text the chat does not place', () => {
    render(
      <IntlTestWrapper>
        <LoadingGoose chatState={ChatState.Streaming} message="Writing" forming={FORMING} />
      </IntlTestWrapper>
    );
    expect(screen.queryByTestId('loading-indicator-forming')).toBeNull();
    const toggle = screen.getByTestId('loading-indicator-forming-toggle');
    expect(toggle).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute('aria-expanded', 'true');

    const calls = screen.getByTestId('loading-indicator-forming-calls').querySelectorAll('li');
    expect(calls).toHaveLength(41);
    expect(calls[40]).toHaveTextContent('41.ledger: ledger append2,500 chars');
    expect(calls[40]).toHaveAttribute('title', 'ledger__ledger_append');
    expect(screen.getByTestId('loading-indicator-forming')).toHaveTextContent(
      '41 tool calls forming'
    );
    expect(screen.getByTestId('loading-indicator-forming')).toHaveTextContent(
      'Received so far: 18,500 chars of arguments'
    );
    expect(screen.getByTestId('loading-indicator-forming-text')).toHaveTextContent(TEXT);
  });

  it('no forming data, no toggle', () => {
    render(
      <IntlTestWrapper>
        <LoadingGoose chatState={ChatState.Streaming} message="Writing" />
      </IntlTestWrapper>
    );
    expect(screen.queryByTestId('loading-indicator-forming-toggle')).toBeNull();
  });
});
