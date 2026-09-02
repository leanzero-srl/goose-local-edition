import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Button } from './button';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const VARIANTS = ['default', 'destructive', 'outline', 'secondary', 'ghost', 'link'] as const;

describe('ui/button (the app-wide base)', () => {
  it('disabled is a solid Studio state on every variant, never an opacity; focus is the accent outline', async () => {
    const { container } = render(
      <>
        {VARIANTS.map((v) => (
          <Button key={v} variant={v} disabled>
            {v}
          </Button>
        ))}
      </>
    );
    for (const v of VARIANTS) {
      const b = screen.getByText(v) as HTMLButtonElement;
      expect(b.disabled).toBe(true);
      for (const c of [
        'disabled:bg-lz-surface-2',
        'disabled:text-lz-ink-4',
        'disabled:border-lz-border',
        'disabled:cursor-not-allowed',
        'focus-visible:outline-ring',
      ]) {
        expect(b.className, v).toContain(c);
      }
      expect(b.className, v).not.toMatch(
        /opacity|ring-ring|\/20|\/40|\/50|\/60|\/80|\/90|shadow-xs/
      );
    }
    assertStudioClean(container);
    expect(
      await missingUtilities([
        'disabled:bg-lz-surface-2',
        'disabled:text-lz-ink-4',
        'disabled:border-lz-border',
        'disabled:cursor-not-allowed',
        'focus-visible:outline-2',
        'focus-visible:outline-offset-1',
        'focus-visible:outline-ring',
        'hover:bg-background-tertiary',
        'hover:bg-lz-inverse-hover',
        'hover:bg-lz-danger-hover',
      ])
    ).toEqual([]);
  }, 30_000);

  // The default and destructive hovers were `/90` alpha blends — a faded fill (DESIGN.md ban 2).
  // They are solid host-wide tokens now, one step toward the page in each theme.
  it('default and destructive hover on SOLID host-wide tokens, never a /90 alpha', () => {
    render(
      <>
        <Button variant="default">default</Button>
        <Button variant="destructive">destructive</Button>
      </>
    );
    expect(screen.getByText('default').className).toContain('hover:bg-lz-inverse-hover');
    expect(screen.getByText('destructive').className).toContain('hover:bg-lz-danger-hover');

    const css = readFileSync(resolve(__dirname, '../../styles/main.css'), 'utf8');
    for (const token of ['--color-lz-inverse-hover', '--color-lz-danger-hover']) {
      const values = [...css.matchAll(new RegExp(`${token}:\\s*(#[0-9a-f]{6});`, 'g'))].map(
        (m) => m[1]
      );
      // one light (:root) and one dark (.dark) value, both solid 6-digit hex, and they differ
      expect(values, token).toHaveLength(2);
      expect(new Set(values).size, token).toBe(2);
    }
    expect(css).not.toMatch(/background-(inverse|danger)\/90/);
  });
});
