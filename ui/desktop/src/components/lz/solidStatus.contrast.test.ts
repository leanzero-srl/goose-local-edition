import { createElement } from 'react';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { darkTokens, lightTokens } from '../../theme/theme-tokens';
import { Button as HostButton } from '../ui/button';
import { Button as StudioButton } from './Button';
import { contrast, resolvedPaint, studioToken } from './resolvedPaint';

const THEMES = ['light', 'dark'] as const;

function host(name: string, theme: (typeof THEMES)[number]): string {
  const value = (theme === 'dark' ? darkTokens : lightTokens)[name as keyof typeof lightTokens];
  expect(value, name).toMatch(/^#[0-9a-f]{6}$/i);
  return value.toLowerCase();
}

/**
 * Q-457 — every colour-modifier wash in the host became a SOLID token. These are the pairs the
 * replacements rest on, resolved through main.css per theme (the same cascade resolvedPaint uses),
 * so a token that drifts under the line fails here rather than on a live walk.
 */
describe('the solid replacements for the Q-457 washes hold their contrast in both themes', () => {
  it('status and accent FILLS carry their ink at 4.5:1 (badges, strips, the danger hover)', () => {
    const fills: Array<[string, string]> = [
      ['--color-lz-ok-solid', '#ffffff'],
      ['--color-lz-warn-solid', '#ffffff'],
      ['--color-lz-err-solid', '#ffffff'],
      ['--color-lz-accent', '--color-lz-accent-ink'],
      ['--color-lz-secondary', '--color-lz-secondary-ink'],
    ];
    for (const theme of THEMES) {
      for (const [fill, ink] of fills) {
        const i = ink.startsWith('#') ? ink : studioToken(ink, theme);
        expect(contrast(studioToken(fill, theme), i), `${theme} ${fill}`).toBeGreaterThanOrEqual(
          4.5
        );
      }
    }
  });

  it('a CALLOUT edge reads at 3:1 on every surface a callout sits on', () => {
    for (const theme of THEMES) {
      const grounds = [
        studioToken('--color-lz-surface', theme),
        studioToken('--color-lz-bg', theme),
        host('--color-background-primary', theme),
      ];
      for (const edge of [
        '--color-lz-ok',
        '--color-lz-warn',
        '--color-lz-err',
        '--color-lz-accent-line',
      ]) {
        for (const ground of grounds) {
          expect(
            contrast(studioToken(edge, theme), ground),
            `${theme} ${edge} on ${ground}`
          ).toBeGreaterThanOrEqual(3);
        }
      }
      // ...and the words inside it are the page ink.
      for (const ground of grounds) {
        expect(contrast(studioToken('--color-lz-ink', theme), ground)).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it('a selected card ring (accent line) reads at 3:1 on the host surfaces it sits on', () => {
    for (const theme of THEMES) {
      for (const ground of ['--color-background-primary', '--color-background-secondary']) {
        expect(
          contrast(studioToken('--color-lz-accent-line', theme), host(ground, theme)),
          `${theme} ${ground}`
        ).toBeGreaterThanOrEqual(3);
      }
    }
  });

  it('the find bar: quiet ink and the hover step on the inverse surface', () => {
    for (const theme of THEMES) {
      const inverse = host('--color-background-inverse', theme);
      expect(
        contrast(studioToken('--color-lz-inverse-ink-2', theme), inverse),
        `${theme} placeholder`
      ).toBeGreaterThanOrEqual(4.5);
      expect(
        contrast(
          studioToken('--color-lz-inverse-hover', theme),
          host('--color-text-inverse', theme)
        ),
        `${theme} hover`
      ).toBeGreaterThanOrEqual(4.5);
    }
  });

  it('search matches are solid fills that carry a fixed dark ink', () => {
    for (const theme of THEMES) {
      const ink = studioToken('--highlight-ink', theme);
      for (const fill of ['--highlight-color', '--highlight-current']) {
        expect(studioToken(fill, theme), fill).toMatch(/^#[0-9a-f]{6}$/);
        expect(contrast(studioToken(fill, theme), ink), `${theme} ${fill}`).toBeGreaterThanOrEqual(
          4.5
        );
      }
      expect(studioToken('--highlight-color', theme)).not.toBe(
        studioToken('--highlight-current', theme)
      );
    }
  });

  it('the sidebar scrollbar thumb is a solid grey that reads at 3:1 on the sidebar', () => {
    for (const theme of THEMES) {
      expect(
        contrast(
          host('--color-text-secondary', theme),
          host('--color-background-secondary', theme)
        ),
        theme
      ).toBeGreaterThanOrEqual(3);
    }
  });
});

/**
 * Q-475 — the destructive button is ONE solid token that carries white at 4.5:1 in both themes, at
 * rest and under the pointer. The 3.0.76 walk measured the confirm buttons (Delete Session, Remove,
 * Stop) at 2.8–3.3:1: ui/button filled with --color-background-danger (#f94b4b / #ff6b6b) and
 * hovered to #fa6161 / #ff5252, and lz/Button hovered to the bare err token (#ef4444 in dark).
 * Both primitives are RENDERED here and their compiled classes resolved, so a variant that drifts
 * back to a pastel fill fails on the class it actually paints.
 */
describe('the destructive fill is solid and readable in both primitives (Q-475)', () => {
  const primitives = [
    ['ui/button', () => createElement(HostButton, { variant: 'destructive' }, 'Delete Session')],
    ['lz/Button', () => createElement(StudioButton, { variant: 'destructive' }, 'Delete Session')],
  ] as const;

  for (const [name, make] of primitives) {
    it(`${name} destructive: white ink at 4.5:1 at rest and under hover, light and dark`, async () => {
      render(make());
      const button = screen.getByRole('button', { name: 'Delete Session' });
      const fills = new Set<string>();
      for (const theme of THEMES) {
        for (const hover of [false, true]) {
          const paint = await resolvedPaint(button, theme, { hover });
          expect(paint.missing, `${name} classes that compile to nothing`).toEqual([]);
          expect(paint.bg, `${name} ${theme} hover=${hover} fill`).toMatch(/^#[0-9a-f]{6}$/);
          expect(paint.text).toBe('#ffffff');
          expect(
            contrast(paint.bg, paint.text),
            `${name} ${theme} hover=${hover}: white on ${paint.bg}`
          ).toBeGreaterThanOrEqual(4.5);
          if (!hover) fills.add(paint.bg as string);
        }
      }
      expect([...fills], 'one destructive fill in both themes').toEqual([
        studioToken('--color-lz-err-solid', 'light'),
      ]);
    });
  }

  it('the destructive token and its hover step are solid hexes that carry white at 4.5:1', () => {
    for (const theme of THEMES) {
      for (const token of ['--color-lz-err-solid', '--color-lz-danger-hover']) {
        const fill = studioToken(token, theme);
        expect(fill, token).toMatch(/^#[0-9a-f]{6}$/);
        expect(contrast(fill, '#ffffff'), `${theme} ${token}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});
