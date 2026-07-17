import { describe, it, expect } from 'vitest';

/**
 * The banner's decision, extracted as the pure rule it always should have been.
 *
 * THE BUG THIS PINS, exactly as Mihai hit it: a run finished 7/7 tasks with 0 failed, and the engine's own
 * events said complete_result{passed:true, verified:true}, review{ran:true, findings:[]},
 * run_overview{generated:false, run_command:"python3 -m ledger --help", run_command_verified:TRUE}.
 * The panel showed a RED "This build did not reach a runnable, verified state — no summary was generated."
 *
 * The engine said the app RUNS. The UI called the build failed because the SUMMARIZER stayed quiet.
 * `generated` only ever meant "the model wrote the prose". A false red is the same sin as a false green,
 * and this panel exists to stop exactly that: only a deterministic engine event may create or kill a verdict.
 */
type Banner = 'none' | 'not-verified' | 'no-summary';

export function overviewBanner(verified: boolean, generated: boolean): Banner {
  if (!verified) return 'not-verified';
  if (!generated) return 'no-summary';
  return 'none';
}

describe('overviewBanner', () => {
  it('NEVER calls a verified build un-runnable just because the summary is missing — the real case', () => {
    // 7/7 done, 0 failed, passed+verified, run_command_verified — only the prose was missing.
    expect(overviewBanner(true, false)).toBe('no-summary');
    expect(overviewBanner(true, false)).not.toBe('not-verified');
  });

  it('still warns when goose genuinely never ran the app', () => {
    expect(overviewBanner(false, true)).toBe('not-verified');
    expect(overviewBanner(false, false)).toBe('not-verified');
  });

  it('says nothing when the app is verified and summarised', () => {
    expect(overviewBanner(true, true)).toBe('none');
  });

  it('the verify check is asked FIRST — a missing summary can never outrank it', () => {
    // The old code branched on `generated` first, so `generated:false` produced a red banner regardless of
    // whether the engine had verified the app. Ordering is the whole fix.
    const cases: Array<[boolean, boolean, Banner]> = [
      [true, false, 'no-summary'], // verified wins
      [false, false, 'not-verified'], // unverified wins
    ];
    for (const [verified, generated, want] of cases) {
      expect(overviewBanner(verified, generated)).toBe(want);
    }
  });
});
