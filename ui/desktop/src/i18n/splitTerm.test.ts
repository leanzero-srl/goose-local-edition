import { describe, expect, it } from 'vitest';
import en from './messages/en.json';

/**
 * Q-155: one thing, one name. The 3.0.52 live round read "Dist · 11.0 tok/s" in the tray, "The
 * distributed engine owns this Mac" on the Engine tab and "The split owns this Mac" under Run it —
 * three names for the model split across the Macs. Every catalog message now says "split"
 * ("Split across 2 Macs", "the split"); this refuses the old names coming back through a new
 * message. (The tray's English lives in utils/mlxTray.ts and is pinned by mlxTray.test.ts.)
 */
describe('the split has one name in every message a person reads', () => {
  it('no message calls it the distributed engine, "Distributed · …" or "Dist"', () => {
    const offenders = Object.entries(en as Record<string, { defaultMessage: string }>)
      .filter(([, { defaultMessage }]) =>
        /distributed (engine|inference|run|configuration)|Distributed (·|engine|runs)|\bDist\b/i.test(
          defaultMessage
        )
      )
      .map(([id, { defaultMessage }]) => `${id}: ${defaultMessage}`);
    expect(offenders).toEqual([]);
  });
});
