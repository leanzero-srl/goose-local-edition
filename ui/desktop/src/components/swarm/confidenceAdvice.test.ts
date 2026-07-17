import { describe, it, expect } from 'vitest';
import { agreementAdvice, holdingBackText } from './SwarmRunPanel';

/**
 * THE BUG THIS PINS, exactly as it rendered on Mihai's screen (run allon-mihai, 1.41.56, conf 88):
 *
 *   Agreement      ############################  88
 *                  "3 drafts agree: count spread 1, file-overlap 100% (role-normalized)"
 *   WHAT'S HOLDING IT BACK
 *   3 drafts agree: count spread 1, file-overlap 100% (role-normalized)      <- a compliment, as the problem
 *   WHAT WOULD RAISE IT
 *   The drafts disagree on how to structure the build. ...try the retarget option...
 *                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ flatly contradicts the line above, and pitches a
 *                                                     lever that was ALREADY ON.
 *
 * The advice was a single static string written for the LOW-agreement case and shown for every value of
 * agreement. A panel that contradicts the engine on screen is the UI equivalent of a false green: the
 * numbers were right and the prose told the user something the engine never said.
 */
describe('agreementAdvice', () => {
  it('NEVER claims the drafts disagree when they agree — the real case (agreement 88)', () => {
    const advice = agreementAdvice(88);
    expect(advice).not.toMatch(/drafts disagree/i);
    expect(advice).toMatch(/little headroom|nearly the same structure/i);
  });

  it('does not pitch retarget when there is no headroom for it to win', () => {
    // Recommending "turn on retarget" to someone at 88 wastes fleet minutes on a ~12-point ceiling.
    expect(agreementAdvice(88)).not.toMatch(/retarget/i);
    expect(agreementAdvice(95)).not.toMatch(/retarget/i);
  });

  it('still tells the truth when the drafts genuinely disagree', () => {
    expect(agreementAdvice(30)).toMatch(/drafts disagree/i);
    expect(agreementAdvice(30)).toMatch(/retarget/i);
    expect(agreementAdvice(59)).toMatch(/drafts disagree/i);
  });

  it('has a middle tier where re-drafting is honestly worth a try', () => {
    expect(agreementAdvice(70)).toMatch(/broadly agree/i);
    expect(agreementAdvice(70)).toMatch(/retarget/i);
    expect(agreementAdvice(70)).not.toMatch(/drafts disagree on how/i);
  });

  it('is monotone in tone across the boundaries — no tier is skipped', () => {
    const tiers = [0, 59, 60, 84, 85, 100].map(agreementAdvice);
    expect(new Set(tiers).size).toBe(3); // exactly three distinct messages, at the stated cuts
    expect(tiers[0]).toBe(tiers[1]); // 0 and 59 -> low
    expect(tiers[2]).toBe(tiers[3]); // 60 and 84 -> mid
    expect(tiers[4]).toBe(tiers[5]); // 85 and 100 -> high
  });
});

describe('holdingBackText', () => {
  it('frames a POSITIVE agreement reason as the cap it actually is', () => {
    // final = min(agreement, clarity). At 88 vs 100, agreement IS the ceiling — say so, then quote the
    // engine verbatim, instead of printing "3 drafts agree" under a "what's holding it back" header.
    const t = holdingBackText(88, true, '3 drafts agree: count spread 1, file-overlap 100%', true);
    expect(t).toMatch(/caps the score at 88/);
    expect(t).toContain('3 drafts agree: count spread 1, file-overlap 100%');
  });

  it('never invents a reason the engine did not give', () => {
    const t = holdingBackText(40, true, null, true);
    expect(t).toBe('The planning drafts disagree on how to structure the build.');
  });

  it('when spec-clarity binds, it talks about the SPEC, not the drafts', () => {
    expect(holdingBackText(90, false, 'ignored', true)).toMatch(/requirements are still ambiguous/i);
    expect(holdingBackText(90, false, 'ignored', false)).toMatch(/product itself isn't fully specified/i);
  });
});
