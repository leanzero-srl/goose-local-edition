import { describe, expect, it } from 'vitest';
import { buildActivity, buildPhaseTodo } from './useSwarmRun';

/**
 * THE STALE-PAUSE DEFECT (Mihai, r4-relaunch, 2026-08-30): `clarify.pending` is a FILE test —
 * questions file present, answers file absent. The proxyless engine's timeout path never writes
 * answers, so the panel said "the build is paused, waiting for you" while the run was mid-REVIEW.
 * The event stream is the truth and `low_confidence_ask_timeout` is its sentence.
 */
const ASK = {
  event: 'low_confidence_ask',
  questions: [{ question: 'D1?' }, { question: 'D2?' }, { question: 'D3?' }],
};
const TIMEOUT = {
  event: 'low_confidence_ask_timeout',
  waited_secs: 5,
  questions_unanswered: 3,
  detail: 'no answers arrived',
};

describe('a timed-out ask resolves the clarify state instead of begging forever', () => {
  it('buildActivity surfaces proxy.timedOut with the window and count', () => {
    const { proxy } = buildActivity([ASK, TIMEOUT]);
    expect(proxy.timedOut).toEqual({ questions: 3, waitedSecs: 5 });
  });

  it('an ask with no timeout stays live', () => {
    const { proxy } = buildActivity([ASK]);
    expect(proxy.timedOut).toBeNull();
  });

  it('the feed carries the resolution so the record survives the card', () => {
    const { activity } = buildActivity([ASK, TIMEOUT]);
    expect(
      activity.some((r) => r.text.includes('Unanswered at the 5s window'))
    ).toBe(true);
  });

  it('the checklist row reads done-with-reason, never running, after the window', () => {
    const phases = buildPhaseTodo([ASK, TIMEOUT], {}, { clarifyPending: true });
    const row = phases
      .flatMap((p) => p.items)
      .find((i) => i.id === 'a-ask-legacy');
    expect(row?.state).toBe('done');
    expect(row?.detail).toContain('unanswered at the unattended window');
  });
});
