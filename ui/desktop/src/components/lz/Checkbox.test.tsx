import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { Checkbox } from './Checkbox';
import { assertStudioClean } from './assertStudioClean';

function Controlled({ initial = false }: { initial?: boolean }) {
  const [on, setOn] = useState(initial);
  return (
    <Checkbox
      checked={on}
      onChange={setOn}
      label="Developer"
      description="Edit files and run shell commands"
    />
  );
}

describe('lz/Checkbox', () => {
  it('is a role=checkbox named by its label alone, described by its description — no native input', () => {
    const { container } = render(<Controlled />);
    expect(container.querySelector('input')).toBeNull();
    const box = screen.getByRole('checkbox', { name: 'Developer' });
    expect(box).toHaveAttribute('aria-checked', 'false');
    expect(box).toHaveAccessibleDescription('Edit files and run shell commands');
    expect(screen.getByLabelText('Developer')).toBe(box);
    assertStudioClean(container);
  });

  it('toggles on click and keyboard activation, and checked is the solid accent fill with a tick', () => {
    const { container } = render(<Controlled />);
    const box = screen.getByRole('checkbox', { name: 'Developer' });
    fireEvent.click(box);
    expect(box).toHaveAttribute('aria-checked', 'true');
    const mark = box.querySelector('span[aria-hidden]')!;
    expect(mark.className).toContain('bg-lz-accent');
    expect(mark.querySelector('svg')).not.toBeNull();
    fireEvent.click(box);
    expect(box).toHaveAttribute('aria-checked', 'false');
    expect(box.querySelector('span[aria-hidden] svg')).toBeNull();
    assertStudioClean(container);
  });

  it('the card variant rings the whole tile in the accent when checked; disabled cannot change', () => {
    const onChange = vi.fn();
    const { container, rerender } = render(
      <Checkbox checked variant="card" label="Memory" onChange={onChange} />
    );
    const tile = screen.getByRole('checkbox', { name: 'Memory' });
    expect(tile.className).toContain('ring-lz-accent');
    rerender(
      <Checkbox checked={false} variant="card" label="Memory" onChange={onChange} disabled />
    );
    expect(tile.className).not.toContain('ring-lz-accent');
    expect(tile).toBeDisabled();
    fireEvent.click(tile);
    expect(onChange).not.toHaveBeenCalled();
    assertStudioClean(container);
  });
});
