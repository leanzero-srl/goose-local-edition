import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { contrast } from '../components/lz/resolvedPaint';
import { darkTokens, lightTokens } from './theme-tokens';

/**
 * Q-334 — the keyboard focus ring is visible in EVERY edition and theme. FOCUS, `ring-ring` and
 * `outline-ring` all draw `--color-ring-primary`; the standard (non-local) edition used to leave it
 * at #e3e6ea in light (a near-white hairline on white) and #525b68 in dark, and only
 * `.local-edition` repainted it. WCAG 2.2 SC 1.4.11: a focus indicator needs 3:1 against what it
 * sits on.
 */

const HOST_SURFACES = ['--color-background-primary', '--color-background-secondary'] as const;
/** The Studio surfaces the local edition paints (DESIGN.md: bg, surface, surface-2). */
const LZ_SURFACES = {
  light: ['#f8fafc', '#ffffff', '#f1f5f9'],
  dark: ['#0b1220', '#0f172a', '#1e293b'],
};

describe('the focus ring is a solid, visible colour in both editions (Q-334)', () => {
  for (const [theme, tokens] of [
    ['light', lightTokens],
    ['dark', darkTokens],
  ] as const) {
    it(`${theme}: 3:1 or better on every host and Studio surface`, () => {
      const ring = tokens['--color-ring-primary'];
      expect(ring).toMatch(/^#[0-9a-f]{6}$/i);
      for (const surface of [...HOST_SURFACES.map((k) => tokens[k]), ...LZ_SURFACES[theme]]) {
        expect(contrast(ring, surface), `${ring} on ${surface}`).toBeGreaterThanOrEqual(3);
      }
    });
  }

  it('no edition repaints the ring behind the token (one owner: theme-tokens.ts)', () => {
    const css = readFileSync(resolve(__dirname, '../styles/main.css'), 'utf8');
    expect(css).not.toMatch(/--color-ring(-primary)?\s*:\s*#[0-9a-f]{3,6}/i);
  });
});
