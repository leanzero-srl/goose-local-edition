import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Chip } from './Chip';
import { assertStudioClean } from './assertStudioClean';

describe('lz/Chip', () => {
  it('is QUIET by default: a 1px outline, ink-3 text, no fill, no uppercase', () => {
    const { container, getByText } = render(<Chip>4-bit</Chip>);
    const chip = getByText('4-bit');
    expect(chip.className).toContain('border-lz-border-strong');
    expect(chip.className).toContain('text-lz-ink-3');
    expect(chip.className).not.toMatch(/(^|\s)bg-/);
    expect(chip.className).not.toMatch(/uppercase|tracking-/);
    expect(chip.className).toContain('tnum');
    assertStudioClean(container);
  });

  it('a tone is the status fill carrying its ink', () => {
    const { container, getByText } = render(
      <>
        <Chip tone="ok">done</Chip>
        <Chip tone="err">failed</Chip>
        <Chip tone="accent">selected</Chip>
        <Chip tone="secondary">thinking</Chip>
      </>
    );
    expect(getByText('done').className).toContain('bg-lz-ok-solid text-white');
    expect(getByText('failed').className).toContain('bg-lz-err-solid text-white');
    expect(getByText('selected').className).toContain('bg-lz-accent text-lz-accent-ink');
    expect(getByText('thinking').className).toContain('bg-lz-secondary text-lz-secondary-ink');
    expect(getByText('done').getAttribute('data-tone')).toBe('ok');
    assertStudioClean(container);
  });

  it('a node is the identity hue with the ink measured for it', () => {
    const { container, getByText } = render(<Chip node={3}>m3-ultra</Chip>);
    const chip = getByText('m3-ultra');
    expect(chip.className).toContain('bg-lz-node-3 text-lz-node-3-ink');
    expect(chip.getAttribute('data-node')).toBe('3');
    assertStudioClean(container);
  });
});
