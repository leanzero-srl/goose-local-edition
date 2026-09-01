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
      expect(b.className, v).not.toMatch(/opacity|ring-ring|\/20|\/40|\/50|\/60|\/80|shadow-xs/);
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
      ])
    ).toEqual([]);
  }, 30_000);
});
