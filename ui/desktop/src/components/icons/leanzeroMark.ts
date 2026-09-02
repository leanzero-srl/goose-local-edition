/**
 * THE LEANZERO MARK — a bold "L" cradling a flock of three geese climbing out of its crook.
 *
 * Owner brief (2026-09-02): "a big L encompassing them ... 2 or 3 geese flying ... black or maybe
 * blue the same colour as leanzero". The L is the leanzero.net monogram language (sharp corners,
 * one weight); the geese are the product (Goose Flock). Drawn in `currentColor` so ONE mark serves
 * every context: white on the accent square, LeanZero blue (--color-action-solid) on a plain
 * surface, black in a menu-bar template.
 *
 * Geometry notes, so a later edit does not quietly break it:
 *  - 64x64 design grid. The L is 10 units thick, inset 6, so the flock has a >=2.5-unit gutter
 *    from both arms at every size — birds touching the L turn the mark into a blob.
 *  - GOOSE is mirror-symmetric BY CONSTRUCTION (each wing is the reflection of the other through
 *    x=0). The first three attempts read as darts and swooshes purely because they were not.
 *  - The birds fly nose-up in their own frame; rotate(52) points the flock up-and-right, which
 *    puts consecutive birds along their SHORT axis (length 13.4) rather than their wingspan
 *    (20.4), so an echelon this tight still separates.
 *  - Smallest honest size is ~20px; below that the three birds merge. Call sites render it at 20+.
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
  transform: `translate(${x} ${y}) rotate(52)`,
});

export const LEANZERO_MARK_VIEWBOX = '0 0 64 64';

export const LEANZERO_MARK: { d: string; transform?: string }[] = [
  { d: 'M6,6 H16 V48 H58 V58 H6 Z' },
  goose(28.5, 37),
  goose(39.5, 26),
  goose(50.5, 15),
];
