/**
 * THE LEANZERO MARK — a bold "L" with two geese lifting out of it.
 *
 * Owner brief (2026-09-02): "a big L encompassing them ... 2 or 3 geese flying ... black or maybe
 * blue the same colour as leanzero", then, on the three-bird draft: "2 BIGGER geese going out of
 * that L". The L is the leanzero.net monogram language (sharp corners, one weight); the pair is
 * the product (Goose Flock) leaving it. Drawn in `currentColor` so ONE mark serves every context:
 * white on the accent square, LeanZero blue (--color-action-solid) on a plain surface, black in a
 * menu-bar template.
 *
 * Geometry notes, so a later edit does not quietly break it — all four were paid for by a render
 * that looked wrong:
 *  - 64x64 design grid. The L has LETTER proportions (31 wide x 46 tall, 12 thick), not the
 *    half-canvas corner bracket of the first draft: a near-square bracket reads as a frame, and it
 *    left no room for geese this size. The short foot (ends at x=35) is what lets the trailing
 *    goose's wing dip past it instead of colliding with it.
 *  - The geese are EQUAL size (1.55) — a pair, not a perspective trick — and large enough to be
 *    the subject rather than decoration in the corner.
 *  - GOOSE is mirror-symmetric BY CONSTRUCTION (each wing is the reflection of the other through
 *    x=0). Two earlier freehand attempts read as a jet and a swoosh purely because they were not.
 *  - They fly at rotate(52), which puts the pair along their SHORT axis (length 13.4) rather than
 *    their wingspan (20.4), so the echelon separates: 8.4 units clear between lead tail and
 *    trailing nose. Everything stays inside x 4..57, y 7..60 — nothing touches the viewBox edge,
 *    which is what clipped the second draft's lead bird.
 *  - Smallest honest size is 20px. Call sites render it at 20+.
 */

/** One goose in flight seen from below: neck thrust forward, wings swept back, short tail. */
const GOOSE = [
  'M 0,-9.2',
  'C 1.4,-8.8 1.7,-7.0 1.6,-4.8',
  'C 3.6,-4.4 6.8,-2.2 10.2,1.8',
  'C 6.8,0.7 3.6,0.3 1.7,1.0',
  'C 1.5,2.4 0.9,3.4 0,4.2',
  'C -0.9,3.4 -1.5,2.4 -1.7,1.0',
  'C -3.6,0.3 -6.8,0.7 -10.2,1.8',
  'C -6.8,-2.2 -3.6,-4.4 -1.6,-4.8',
  'C -1.7,-7.0 -1.4,-8.8 0,-9.2',
  'Z',
].join(' ');

const goose = (x: number, y: number) => ({
  d: GOOSE,
  transform: `translate(${x} ${y}) rotate(52) scale(1.55)`,
});

export const LEANZERO_MARK_VIEWBOX = '0 0 64 64';

export const LEANZERO_MARK: { d: string; transform?: string }[] = [
  { d: 'M4,14 H16 V48 H35 V60 H4 Z' },
  goose(33, 40),
  goose(45, 20),
];
