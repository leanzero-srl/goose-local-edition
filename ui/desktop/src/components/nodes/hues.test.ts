import { describe, expect, it } from 'vitest';
import { ciede2000, ciede2000Lab, contrastRatio } from './colorMath';
import { KIND_HUE, ROLE_HUES, STATE_HUES, paletteFills } from './hues';

// Sharma, Wu & Dalal (2005), "The CIEDE2000 color-difference formula", Table 1: the published test
// pairs the formula is checked against (a subset covering the hue-rotation and mean-hue branches).
const SHARMA: [[number, number, number], [number, number, number], number][] = [
  [[50, 2.6772, -79.7751], [50, 0, -82.7485], 2.0425],
  [[50, 3.1571, -77.2803], [50, 0, -82.7485], 2.8615],
  [[50, 2.8361, -74.02], [50, 0, -82.7485], 3.4412],
  [[50, -1.3802, -84.2814], [50, 0, -82.7485], 1.0],
  [[50, 0, 0], [50, -1, 2], 2.3669],
  [[50, 2.49, -0.001], [50, -2.49, 0.0009], 7.1792],
  [[50, 2.5, 0], [73, 25, -18], 27.1492],
  [[50, 2.5, 0], [61, -5, 29], 22.8977],
  [[60.2574, -34.0099, 36.2677], [60.4626, -34.1751, 39.4387], 1.2644],
  [[22.7233, 20.0904, -46.694], [23.0331, 14.973, -42.5619], 2.0373],
  [[90.8027, -2.0831, 1.441], [91.1528, -1.6435, 0.0447], 1.4441],
  [[2.0776, 0.0795, -1.135], [0.9033, -0.0636, -0.5514], 0.9082],
];

const MIN_DISTANCE = 15;
const MIN_CONTRAST = 4.5;

describe('colour math', () => {
  it('CIEDE2000 reproduces the published test pairs', () => {
    for (const [a, b, want] of SHARMA) {
      expect(ciede2000Lab(a, b)).toBeCloseTo(want, 3);
    }
  });

  it('WCAG contrast of the two extremes is 21, of a colour with itself 1', () => {
    expect(contrastRatio('#000000', '#FFFFFF')).toBeCloseTo(21, 5);
    expect(contrastRatio('#15803D', '#15803D')).toBeCloseTo(1, 5);
  });

  it('refuses a colour that is not #rrggbb', () => {
    expect(() => ciede2000('red', '#000000')).toThrow(/not a #rrggbb/);
  });
});

describe('the node palette', () => {
  for (const theme of ['light', 'dark'] as const) {
    const fills = paletteFills(theme);

    it(`every ink reaches ${MIN_CONTRAST}:1 on its fill (${theme})`, () => {
      for (const { owner, fill } of fills) {
        expect(
          contrastRatio(fill.fill, fill.ink),
          `${owner} ${fill.ink} on ${fill.fill}`
        ).toBeGreaterThanOrEqual(MIN_CONTRAST);
      }
    });

    it(`every two fills that can share a card or a strategy row are ≥ ${MIN_DISTANCE} apart (${theme})`, () => {
      const close: string[] = [];
      for (let i = 0; i < fills.length; i++) {
        for (let j = i + 1; j < fills.length; j++) {
          const d = ciede2000(fills[i].fill.fill, fills[j].fill.fill);
          if (d < MIN_DISTANCE) close.push(`${fills[i].owner} ~ ${fills[j].owner}: ${d.toFixed(1)}`);
        }
      }
      expect(close).toEqual([]);
    });
  }

  it('each class string paints exactly the hexes it declares', () => {
    const hues = [KIND_HUE, ...Object.values(STATE_HUES), ...Object.values(ROLE_HUES)];
    for (const hue of hues) {
      expect(hue.className).toContain(`bg-[${hue.light.fill}]`);
      expect(hue.className).toContain(`text-[${hue.light.ink}]`);
      if (hue.dark.fill !== hue.light.fill) {
        expect(hue.className).toContain(`dark:bg-[${hue.dark.fill}]`);
        expect(hue.className).toContain(`dark:text-[${hue.dark.ink}]`);
      }
    }
  });

  it('the three families never share a hue', () => {
    const state = Object.values(STATE_HUES).map((h) => h.light.fill);
    const role = Object.values(ROLE_HUES).map((h) => h.light.fill);
    const all = [KIND_HUE.light.fill, ...state, ...role];
    expect(new Set(all).size).toBe(all.length);
  });
});
