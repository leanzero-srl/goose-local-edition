import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createIntl } from 'react-intl';
import { ChatState } from '../../types/chatState';
import { createUserMessage, type Message } from '../../types/message';
import {
  composerLoopSlot,
  isSwarmBuildChat,
  loopReplyLine,
  tickFinishingElsewhere,
  tickRunningHere,
} from './composerLoop';
import { parseLoopCommand, type LoopCommand } from './model';
import { LOOP_MESSAGES, NOW_MS, loopRecord, waitingRecord } from './railFixtures';

const intl = createIntl({ locale: 'en', messages: {} });

beforeEach(() => {
  vi.spyOn(Date.prototype, 'getTimezoneOffset').mockReturnValue(0);
});

const command = (line: string): LoopCommand => {
  const parsed = parseLoopCommand(line);
  if (!parsed) throw new Error(`${line} is not a /loop command`);
  return parsed;
};

describe('the composer and the chat loop', () => {
  it('refuses a loop on a swarm build chat exactly where goosed does', () => {
    expect(isSwarmBuildChat('swarm', 'swarm-build')).toBe(true);
    expect(isSwarmBuildChat('swarm', 'swarm-build:strategy:two-macs')).toBe(true);
    expect(isSwarmBuildChat('swarm', 'swarm')).toBe(false);
    expect(isSwarmBuildChat('swarm', 'node:studio-27b')).toBe(false);
    expect(isSwarmBuildChat('swarm', 'strategy:two-macs@build')).toBe(false);
    expect(isSwarmBuildChat('anthropic', 'swarm-build')).toBe(false);
    expect(isSwarmBuildChat('swarm', null)).toBe(false);
  });

  it('the slot is the Loop button, the status chip, or nothing while the loop is read', () => {
    const facts = { swarmBuild: false, endedSeen: false, nowMs: NOW_MS };
    expect(composerLoopSlot({ kind: 'loading' }, facts)).toEqual({ kind: 'none' });
    expect(composerLoopSlot({ kind: 'none' }, facts)).toEqual({
      kind: 'button',
      swarmBuild: false,
    });
    expect(composerLoopSlot({ kind: 'none' }, { ...facts, swarmBuild: true })).toEqual({
      kind: 'button',
      swarmBuild: true,
    });
    const unreadable = composerLoopSlot({ kind: 'unreadable', error: 'bad json' }, facts);
    expect(unreadable).toMatchObject({ kind: 'chip', view: { tone: 'err' } });

    const running = composerLoopSlot(
      { kind: 'loop', loop: loopRecord(), status: 'running' },
      facts
    );
    expect(running).toMatchObject({ kind: 'chip', view: { tone: 'ok' } });
    if (running.kind === 'chip') {
      expect(intl.formatMessage(running.view.label.message, running.view.label.values)).toBe(
        'Tick 5 running · 2m'
      );
    }
  });

  it('names the chip in the rail pill words for every status, and hides a seen ended loop', () => {
    const facts = { swarmBuild: false, endedSeen: false, nowMs: NOW_MS };
    const label = (status: Parameters<typeof composerLoopSlot>[0], seen = false) => {
      const slot = composerLoopSlot(status, { ...facts, endedSeen: seen });
      return slot.kind === 'chip'
        ? [slot.view.tone, intl.formatMessage(slot.view.label.message, slot.view.label.values)]
        : slot.kind;
    };
    const w = waitingRecord();
    expect(label({ kind: 'loop', loop: w, status: 'waiting' })).toEqual([
      'accent',
      'Next tick 22:51',
    ]);
    expect(label({ kind: 'loop', loop: w, status: 'paused' })).toEqual(['stopped', 'Loop paused']);
    expect(label({ kind: 'loop', loop: w, status: 'needs_you' })).toEqual([
      'warn',
      'Loop needs you',
    ]);
    expect(label({ kind: 'loop', loop: w, status: 'waiting_you' })).toEqual([
      'warn',
      'Loop waiting for you',
    ]);
    expect(label({ kind: 'loop', loop: w, status: 'ended' })).toEqual(['stopped', 'Loop ended']);
    expect(label({ kind: 'loop', loop: w, status: 'ended' }, true)).toBe('button');
    expect(label({ kind: 'loop', loop: w, status: 'elsewhere' })).toEqual([
      'secondary',
      'Looping in another window',
    ]);
  });

  it('knows the turn in flight is tick n from its marker, and a person’s own turn is not', () => {
    expect(tickRunningHere(LOOP_MESSAGES, ChatState.Streaming)).toBe(5);
    expect(tickRunningHere(LOOP_MESSAGES, ChatState.Thinking)).toBe(5);
    expect(tickRunningHere(LOOP_MESSAGES, ChatState.Idle)).toBeNull();
    expect(tickRunningHere(LOOP_MESSAGES, ChatState.LoadingConversation)).toBeNull();

    const steer: Message = {
      ...createUserMessage('also check svc- names'),
      metadata: { userVisible: true, agentVisible: true, steer: true },
    };
    expect(tickRunningHere([...LOOP_MESSAGES, steer], ChatState.Streaming)).toBe(5);

    const own = createUserMessage('what did tick 5 change?');
    expect(tickRunningHere([...LOOP_MESSAGES, own], ChatState.Streaming)).toBeNull();
  });

  it('says a tick finishes in the background only when this window never ran it', () => {
    const running = { kind: 'loop' as const, loop: loopRecord(), status: 'running' as const };
    expect(tickFinishingElsewhere(running, ChatState.Idle, null)).toBe(5);
    // Tick 5 just ended here and goosed's record has not caught up yet: not "in the background".
    expect(tickFinishingElsewhere(running, ChatState.Idle, 5)).toBeNull();
    expect(tickFinishingElsewhere(running, ChatState.Streaming, null)).toBeNull();
    expect(
      tickFinishingElsewhere(
        { kind: 'loop', loop: waitingRecord(), status: 'waiting' },
        ChatState.Idle,
        null
      )
    ).toBeNull();
    expect(tickFinishingElsewhere({ kind: 'none' }, ChatState.Idle, null)).toBeNull();
  });

  it('answers a /loop control in the words goosed’s /loop says (execute_commands.rs)', () => {
    const paused = waitingRecord({
      status: 'paused',
      statusReason: { kind: 'by_you', afterTick: 5 },
    });
    expect(
      loopReplyLine(
        intl,
        command('/loop pause'),
        { kind: 'control', result: { loop: paused } },
        NOW_MS
      )
    ).toEqual({ text: 'Loop paused after tick 5.', refused: false });
    expect(
      loopReplyLine(
        intl,
        command('/loop stop'),
        { kind: 'control', result: { loop: loopRecord({ status: 'ended', ticks: [] }) } },
        NOW_MS
      ).text
    ).toBe('Loop stopped before its first tick.');
    expect(
      loopReplyLine(
        intl,
        command('/loop resume'),
        { kind: 'control', result: { loop: waitingRecord() } },
        NOW_MS
      ).text
    ).toBe('Loop resumed — next tick 22:51.');
    expect(
      loopReplyLine(
        intl,
        command('/loop now'),
        {
          kind: 'control',
          result: {
            loop: waitingRecord({
              offer: { n: 6, messageId: 'looptick_x', offeredAt: '2026-09-27T22:43:00Z' },
            }),
          },
        },
        NOW_MS
      ).text
    ).toBe('Tick 6 starts now.');
    expect(
      loopReplyLine(
        intl,
        command('/loop pause'),
        {
          kind: 'control',
          result: {
            refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
          },
        },
        NOW_MS
      )
    ).toEqual({ text: 'The loop runner is not in this build', refused: true });
    expect(loopReplyLine(intl, command('/loop'), { kind: 'status', result: {} }, NOW_MS).text).toBe(
      'No loop in this chat. Use /loop <goal> or the Loop button.'
    );
    expect(
      loopReplyLine(
        intl,
        command('/loop'),
        { kind: 'status', result: { error: 'record unreadable' } },
        NOW_MS
      )
    ).toEqual({ text: 'record unreadable', refused: true });
    const status = loopReplyLine(
      intl,
      command('/loop'),
      { kind: 'status', result: { loop: waitingRecord() } },
      NOW_MS
    ).text;
    expect(status).toMatch(
      /^Loop: Make scripts\/generate_users\.js produce every problem class in notes\/kickoff\.md · Next tick 22:51 · in \d+m · tick 5$/
    );
  });
});
