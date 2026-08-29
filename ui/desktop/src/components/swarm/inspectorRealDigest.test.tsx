import { describe, expect, it } from 'vitest';
import { inspectorOutputText, inspectorThinkingText } from './SwarmRunPanel';
import real from './__fixtures__/realLaneDigest.json';

// THE ARCHIVED-TREE REPLAY, APPLIED TO THE UI.
//
// The other inspector tests use synthetic strings. This one uses REAL engine output captured from
// lane `open-coverage-1`, where the rolling window is 2,000 characters and the durable think.log is
// 6,014 — the exact proportions that made the pane look truncated and, when the two were joined,
// made it render everything twice.
describe('the inspector against real engine output', () => {
  const lane = {
    fullThinking: real.full_thinking,
    fullReasoning: real.full_reasoning ?? undefined,
    reasoning: real.reasoning ?? undefined,
    lastThinking: real.last_thinking ?? undefined,
    lastText: real.last_text ?? undefined,
    recent: real.recent ?? [],
  };

  it('shows the durable log, not the rolling window', () => {
    const out = inspectorThinkingText(lane);
    expect(out.length).toBeGreaterThan((real.last_thinking ?? '').length);
    expect(out).toBe(real.full_thinking.trim());
  });

  it('does not render the reasoning twice', () => {
    const out = inspectorThinkingText(lane);
    // The window is a SUFFIX of the log. If the two were joined, its opening line would appear twice.
    const windowHead = (real.last_thinking ?? '').slice(0, 120);
    expect(windowHead.length).toBeGreaterThan(50);
    const occurrences = out.split(windowHead).length - 1;
    expect(occurrences).toBe(1);
  });

  it('falls back correctly on a lane that produced no answer-channel text', () => {
    // This lane wrote no <task>.log at all -- a pure-reasoning coverage call. OUTPUT must not invent
    // content, and must not throw.
    const out = inspectorOutputText({ recent: real.recent ?? [], lastText: real.last_text ?? undefined });
    expect(typeof out).toBe('string');
    expect(out).not.toContain('undefined');
  });

  it('the fixture still describes the bug it was captured for', () => {
    // If someone regenerates this fixture from a run where the window happens to cover the whole log,
    // these tests silently stop proving anything.
    expect((real.last_thinking ?? '').length).toBeLessThan(real.full_thinking.length);
  });
});
