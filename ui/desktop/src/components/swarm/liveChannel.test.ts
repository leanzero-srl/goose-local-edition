import { beforeEach, describe, expect, it } from 'vitest';
import { foldEvents, resetFoldCache } from './useSwarmRun';
import { laneLiveLine } from './SwarmRunPanel';

/**
 * THE LIVE LINE FOLLOWS THE CHANNEL THAT ADVANCED LAST.
 *
 * Measured on r1 (gabee, 19:52, flagged by two consecutive 10-minute ticks): the fleet cell for
 * `review-build-app-meridian-…` showed round 1's final answer — data-gen-len 2676, unchanged for twenty
 * minutes — while the lane's digest had a NEW call reasoning behind it, thinking_chars past 24,000 and
 * climbing. REVIEW reuses the lane key every round, `<task>.log` is append-only so the old answer never
 * leaves, and a live line that prefers the transcript whenever it is non-empty shows that answer for as
 * long as the new call thinks. That is "the panel does not show what is being generated", verbatim.
 */

const RUN_STARTED = { event: 'run_started', ts: '2026-08-29T19:00:00Z', pool: [] };
const KEY = 'review-build-app-meridian-1';

/** Round 1's answer as the tick read it: 2,676 chars whose last line is ANSWER-1. */
const ANSWER_1 = `${'x'.repeat(2676 - 'ANSWER-1'.length - 1)}\nANSWER-1`;
const THOUGHT_A = `${'reading the plan against the request\n'.repeat(2)}first thought`;
const THOUGHT_B = `${'y'.repeat(24000)}\nMaybe frontend-serving is imported`;

const POLL_A = { full_transcript: ANSWER_1, thinking_chars: 100, full_thinking: THOUGHT_A };
const POLL_B = { full_transcript: ANSWER_1, thinking_chars: 24854, full_thinking: THOUGHT_B };

const poll = (digest: Record<string, unknown>) => {
  const lane = foldEvents([RUN_STARTED] as never, { [KEY]: digest } as never).planningLanes.find(
    (l) => l.taskId === KEY
  );
  expect(lane, 'the review lane must exist').toBeTruthy();
  return lane!;
};

describe('the live line follows the channel that grew in the latest poll', () => {
  beforeEach(() => resetFoldCache());

  it('shows the NEW call’s thinking, not the previous call’s answer, when only the thinking grew', () => {
    expect(ANSWER_1).toHaveLength(2676);
    poll(POLL_A);
    const lane = poll(POLL_B);
    const line = laneLiveLine(lane);
    expect(line).toBe('💭 Maybe frontend-serving is imported');
    expect(line).not.toContain('ANSWER-1');
  });

  it('returns to the answer the moment the transcript grows while the thinking does not', () => {
    poll(POLL_A);
    expect(laneLiveLine(poll(POLL_B))).toBe('💭 Maybe frontend-serving is imported');
    const lane = poll({ ...POLL_B, full_transcript: `${ANSWER_1}\nANSWER-2` });
    expect(laneLiveLine(lane)).toBe('ANSWER-2');
  });

  it('a done lane shows its answer, whichever channel moved last', () => {
    poll(POLL_A);
    const lane = poll({ ...POLL_B, phase: 'done' });
    expect(lane.status).toBe('done');
    expect(laneLiveLine(lane)).toBe('ANSWER-1');
  });

  it('on first sight prefers the answer when there is one, else the thinking', () => {
    expect(laneLiveLine(poll(POLL_A))).toBe('ANSWER-1');
    resetFoldCache();
    expect(laneLiveLine(poll({ thinking_chars: 100, full_thinking: THOUGHT_A }))).toBe(
      '💭 first thought'
    );
  });

  it('keeps the previous channel when nothing moved', () => {
    poll(POLL_A);
    poll(POLL_B);
    expect(laneLiveLine(poll(POLL_B))).toBe('💭 Maybe frontend-serving is imported');
  });

  it('follows the counter alone when the durable log has not caught up — it is the only signal moving', () => {
    poll({ full_transcript: ANSWER_1, thinking_chars: 100, last_thinking: 'first thought' });
    const lane = poll({
      full_transcript: ANSWER_1,
      thinking_chars: 24854,
      last_thinking: 'Maybe frontend-serving is imported',
    });
    expect(laneLiveLine(lane)).toBe('💭 Maybe frontend-serving is imported');
  });
});
