import { describe, expect, it } from 'vitest';
import { ago, fmtElapsed, fmtRangeMin } from './SwarmRunPanel';

// The run header read "1184h 27m ~11845–47378 min left" on a 49-day-old board. The NUMBERS are the
// truth layer and stay exactly what they were; only the unit a person reads them in changes.
describe('humane time in the run header', () => {
  it('fmtElapsed: seconds, minutes, hours, then days once past two days', () => {
    expect(fmtElapsed(0.5)).toBe('30s');
    expect(fmtElapsed(5.5)).toBe('5m 30s');
    expect(fmtElapsed(90)).toBe('1h 30m');
    expect(fmtElapsed(36 * 60)).toBe('36h 0m');
    // The screenshot's "1184h 27m" is 71,067 minutes: forty-nine days and eight hours.
    expect(fmtElapsed(1184 * 60 + 27)).toBe('49d 8h');
  });

  it('fmtRangeMin: minutes under two hours, hours under two days, days beyond — the range the brief named', () => {
    expect(fmtRangeMin(11845, 47378)).toBe('~8–33 d');
    expect(fmtRangeMin(90, 600)).toBe('~2–10 h');
    expect(fmtRangeMin(3, 12)).toBe('~3–12 min');
    // A range that rounds to one figure states one figure rather than "~2–2 d".
    expect(fmtRangeMin(2800, 2900)).toBe('~2 d');
    // The floor never rounds to zero.
    expect(fmtRangeMin(20, 2 * 1440)).toBe('~1–2 d');
  });

  it('ago: minutes, hours, then days once past two days', () => {
    const now = Date.now();
    expect(ago(now - 30_000)).toBe('30s ago');
    expect(ago(now - 5 * 60_000)).toBe('5m ago');
    expect(ago(now - 10 * 3_600_000)).toBe('10h ago');
    expect(ago(now - 1127 * 3_600_000)).toBe('47d ago');
    expect(ago(null)).toBe('');
  });
});
