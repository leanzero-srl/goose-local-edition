import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Input } from './input';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

describe('ui/input (the app-wide base)', () => {
  it('disabled is a solid Studio state — surface-2 fill, ink-4 text, hairline, not-allowed — never an opacity', async () => {
    const { container } = render(<Input disabled placeholder="Model" />);
    const input = screen.getByPlaceholderText('Model') as HTMLInputElement;
    expect(input.disabled).toBe(true);
    for (const c of [
      'disabled:bg-lz-surface-2',
      'disabled:text-lz-ink-4',
      'disabled:border-lz-border',
      'disabled:cursor-not-allowed',
    ]) {
      expect(input.className).toContain(c);
    }
    expect(input.className).not.toMatch(/opacity/);
    assertStudioClean(container);
    expect(
      await missingUtilities([
        'disabled:bg-lz-surface-2',
        'disabled:text-lz-ink-4',
        'disabled:border-lz-border',
        'disabled:cursor-not-allowed',
      ])
    ).toEqual([]);
  }, 30_000);
});
