import { describe, expect, it } from 'vitest';
import { inspectorThinkingText, laneThinkingRun, liveThinkingWindow } from './SwarmRunPanel';

// This pane has been wrong twice and each time it looked like the OTHER failure, so both are pinned here.
describe('the node inspector shows the whole reasoning, exactly once', () => {
  it('never concatenates the durable log with the rolling window', () => {
    // The window is a SUFFIX of the log by construction: same stream, two views.
    const log = 'Let me analyze my slice: ledgerd-core.\n\nMy responsibilities:\n1. CLI parsing\n';
    const window = 'me analyze my slice: ledgerd-core.\n\nMy responsibilities:\n1. CLI parsing\n';
    const out = inspectorThinkingText({ fullThinking: log, lastThinking: window });
    expect(out).toBe(log.trim());
    expect(out.split('My responsibilities').length - 1).toBe(1);
  });

  it('falls back to the rolling window only when no durable log exists AND the counter is live', () => {
    expect(inspectorThinkingText({ lastThinking: 'only the window', thinkingChars: 640 })).toBe(
      'only the window'
    );
    expect(inspectorThinkingText({})).toBe('');
  });

  // VA-026: the window outlives its call. The join carries the previous poll's `lastThinking` when a
  // new digest omits the key, and a reused lane key (REVIEW round 2, a judge re-stream) seeds its digest
  // with thinking_chars 0 and no think.log — so a DEAD call's reasoning filled the pane under the new
  // call's title. laneThinkingRun already refused this; this pane was the ungated second copy.
  it('refuses a stale window when the counter says this call has no reasoning yet', () => {
    expect(
      inspectorThinkingText({ lastThinking: 'the previous call, still in the carry', thinkingChars: 0 })
    ).toBe('');
    expect(inspectorThinkingText({ lastThinking: 'no counter at all' })).toBe('');
    // Both readers of the window are the SAME predicate — the rule cannot fork again.
    const stale = { lastThinking: 'stale', thinkingChars: 0 };
    expect(liveThinkingWindow(stale)).toBe('');
    expect(laneThinkingRun(stale)).toBe('');
    expect(inspectorThinkingText(stale)).toBe('');
    const live = { lastThinking: 'live', thinkingChars: 4 };
    expect(liveThinkingWindow(live)).toBe('live');
    expect(laneThinkingRun(live)).toBe('live');
    expect(inspectorThinkingText(live)).toBe('live');
  });

  it('never shows the ANSWER channel under the Thinking title (agenda item V, UI half)', () => {
    // The chain read `fullReasoning` (the answer window) and `reasoning` (the answer summary) AHEAD of
    // the thinking window, so a lane with no think.log — or a model with no thinking stream — rendered
    // its ANSWER as its thinking, while Work showed the same words from the durable log.
    expect(
      inspectorThinkingText({
        answerWindow: 'the answer window',
        reasoning: 'answer chunks',
        lastThinking: 'the thinking window',
        thinkingChars: 19,
      })
    ).toBe('the thinking window');
    expect(
      inspectorThinkingText({ answerWindow: 'the answer window', reasoning: 'answer chunks' })
    ).toBe('');
  });

  it('prefers the durable log over every shorter view of the same stream', () => {
    const out = inspectorThinkingText({
      fullThinking: 'the whole 55,000 character transcript',
      answerWindow: 'a shorter answer-channel view',
      reasoning: 'shorter still',
      lastThinking: 'the 2,400-char tail',
    });
    expect(out).toBe('the whole 55,000 character transcript');
  });
});
