import { describe, it, expect } from 'vitest';
import { confVerdict } from './SwarmRunPanel';

// The bug this pins, exactly as Mihai caught it on screen: the gauge read
//   "73  —  Strong — ready to build"
// while ask_floor was 80. That plan had burned all 4 retarget rounds, hit proceed_at_cap, and ASKED 3
// questions. The verdict contradicted the number printed next to it. A verdict must come from the
// engine's own threshold, never a band the UI invented.
describe('confVerdict', () => {
  it('never calls a below-floor plan ready — the exact 73-vs-80 case', () => {
    const v = confVerdict(73, 80);
    expect(v).toContain('Below your bar of 80');
    expect(v.toLowerCase()).not.toContain('ready to build');
    expect(v.toLowerCase()).not.toContain('strong');
  });

  it('calls it ready only at or above the floor', () => {
    expect(confVerdict(80, 80)).toContain('ready to build');
    expect(confVerdict(87, 80)).toContain('ready to build');
    expect(confVerdict(79, 80)).toContain('Below your bar');
  });

  it('a high number under a HIGH floor is still not ready', () => {
    // The old band said >=70 is "Strong — ready to build" regardless. With a floor of 95, 90 is not ready.
    expect(confVerdict(90, 95)).toContain('Below your bar of 95');
  });

  it('falls back to a band ONLY when no floor is set (that run never asks, so there is no bar)', () => {
    expect(confVerdict(73, null)).toBe('Strong');
    expect(confVerdict(50, null)).toContain('Mixed');
    expect(confVerdict(20, null)).toContain('Low');
    // and it must not invent a bar that does not exist
    expect(confVerdict(73, null).toLowerCase()).not.toContain('bar');
  });
});
