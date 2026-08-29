import { describe, expect, it } from 'vitest';
import {
  CARD_TAIL_CHARS,
  INLINE_TAIL_CHARS,
  MAX_REVEAL_BACKLOG_CHARS,
  fleetExpandText,
  isClippedTail,
  laneLiveLine,
  laneNarrative,
  revealStep,
  streamTailNote,
  taskGenReasoning,
} from './SwarmRunPanel';

// EVERY SURFACE THAT RENDERS A STREAM, AND THE ONE RULE THEY NOW SHARE.
//
// The engine writes each call's narration to append-only logs (`<task>.log`, `<task>.think.log`) and main.ts
// hands both to the panel. The digest's `reasoning` / `lastThinking` / `lastText` are ROLLING WINDOWS over
// those same streams, rewritten in place ~2.5x a second. A surface reading a window watches the text CLEAR
// AND REFILL rather than accumulate — "the output rolls" — and every one of these surfaces was fixed
// separately, so the rule existed in five diverged copies and three of them were still rolling.

const rolling = {
  reasoning: 'a couple of digest chunks',
  lastText: 'the rolling answer view',
  lastThinking: 'the tail of the reasoning',
  thinkingChars: 900,
  fullReasoning: 'the 24,000-char clip of the answer channel',
};

describe('the fleet strip reads the durable log, not the rolling window', () => {
  it('shows the durable transcript ahead of every digest view of the same stream', () => {
    const line = laneLiveLine({ ...rolling, fullTranscript: 'the durable append-only transcript' });
    expect(line).toBe('the durable append-only transcript');
  });

  it('shows the durable thinking log ahead of the 2,400-char window', () => {
    const line = laneLiveLine({
      lastThinking: 'the window',
      thinkingChars: 900,
      fullThinking: 'the durable reasoning log',
    });
    expect(line).toBe('💭 the durable reasoning log');
  });

  it('bounds what an inline cell holds, and keeps the NEWEST end', () => {
    const log = `${'x'.repeat(INLINE_TAIL_CHARS * 3)}the newest sentence of the run`;
    const line = laneLiveLine({ fullTranscript: log });
    expect(line.length).toBeLessThanOrEqual(INLINE_TAIL_CHARS);
    expect(line.endsWith('the newest sentence of the run')).toBe(true);
  });

  it('keeps the substance gate — a single-token fragment is never a live line', () => {
    // A busy node rendering as one meaningless letter was observed live; falling through is better.
    expect(laneLiveLine({ fullTranscript: '(g', recent: ['ran pytest -q'] })).toBe('ran pytest -q');
  });

  it('still falls back to the digest for a lane whose logs have not appeared', () => {
    expect(laneLiveLine(rolling)).toBe('a couple of digest chunks');
    expect(laneLiveLine({})).toBe('');
  });

  it('makes a THINKING-ONLY node expandable — that is the node you most need to open', () => {
    expect(
      fleetExpandText({ fullThinking: 'reasoning only, no answer yet', thinkingChars: 12 })
    ).toBe('💭 reasoning only, no answer yet');
    expect(fleetExpandText({})).toBe('');
  });

  it('does not show a stale window behind a lane that has counted no thinking', () => {
    expect(laneLiveLine({ lastThinking: 'left over from the last call', thinkingChars: 0 })).toBe(
      ''
    );
  });
});

describe('a row body prefers the durable transcript', () => {
  it('reads the log before the 24k clip, the digest and the rolling answer', () => {
    expect(laneNarrative({ ...rolling, fullTranscript: 'the log' })).toBe('the log');
    expect(laneNarrative(rolling)).toBe(rolling.fullReasoning);
    expect(laneNarrative({ lastText: 'only the rolling answer' })).toBe('only the rolling answer');
  });
});

describe('the live-generation card no longer shows the 24k clip', () => {
  it('prefers the durable thinking log over full_reasoning', () => {
    const text = taskGenReasoning({
      full_thinking: 'the durable think.log',
      full_reasoning: 'the 24,000-char tail clip',
      reasoning: 'a digest chunk',
      last_thinking: 'the window',
      last_text: 'the rolling answer',
    });
    expect(text).toBe('the durable think.log');
  });

  it('falls back to the durable transcript before any clipped digest field', () => {
    const text = taskGenReasoning({
      full_transcript: 'the durable task.log',
      full_reasoning: 'the 24,000-char tail clip',
    });
    expect(text).toBe('the durable task.log');
  });

  it('keeps the old chain for a digest with no durable log at all', () => {
    expect(taskGenReasoning({ last_text: 'only this' })).toBe('only this');
    expect(taskGenReasoning({})).toBe('');
  });

  it('bounds the card, keeping the newest end', () => {
    const log = `${'y'.repeat(CARD_TAIL_CHARS * 2)}the end of the reasoning`;
    const text = taskGenReasoning({ full_thinking: log });
    expect(text.length).toBe(CARD_TAIL_CHARS);
    expect(text.endsWith('the end of the reasoning')).toBe(true);
  });
});

describe('a pane says when it is only a tail', () => {
  it('uses the flag main.ts computed, which is the only place both numbers exist', () => {
    expect(isClippedTail('a short rendered tail', 400_000, true)).toBe(true);
    expect(isClippedTail('a short rendered tail', 400_000, false)).toBe(false);
  });

  it('never calls a complete NON-ASCII log a tail', () => {
    // THE BUG: the OUTPUT caption compared on-disk BYTES against a UTF-16 length. Three-byte characters
    // make a complete log's size exceed its own rendered length by 3x without a character being dropped.
    const cjk = '這是完整的推理日誌'.repeat(200);
    const bytes = new TextEncoder().encode(cjk).length;
    expect(bytes).toBeGreaterThan(cjk.length + 1024);
    expect(isClippedTail(cjk, bytes)).toBe(false);
    expect(streamTailNote(cjk, bytes)).toBe('');
  });

  it('says how much is on disk when the log really was clipped', () => {
    expect(streamTailNote('the last 400,000 chars', 812_000)).toBe(' · tail of 793KB');
  });

  it('stays silent when there is no durable log to measure', () => {
    expect(streamTailNote(undefined, 812_000)).toBe('');
    expect(streamTailNote('some text')).toBe('');
  });
});

describe('the typewriter reveals new text and never chases a backlog', () => {
  const step = (target: string, current: string) =>
    revealStep({ target, current, charsPerSec: 110, deltaSeconds: 0.1, reduceMotion: false });

  it('snaps when the target is more than a poll ahead, instead of typing from the start', () => {
    const tail = 'z'.repeat(INLINE_TAIL_CHARS);
    expect(step(tail, '')).toBe(tail);
  });

  it('still types the handful of characters a single poll adds', () => {
    const current = 'the model is writing';
    const target = `${current} the next few words`;
    expect(target.length - current.length).toBeLessThan(MAX_REVEAL_BACKLOG_CHARS);
    const next = step(target, current);
    expect(next.startsWith(current)).toBe(true);
    expect(next.length).toBeGreaterThan(current.length);
    expect(next.length).toBeLessThan(target.length);
  });
});
