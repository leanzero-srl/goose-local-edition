import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Button } from './Button';
import { assertStudioClean } from './assertStudioClean';

describe('lz/Button', () => {
  it('primary is the accent fill, secondary a neutral outline, ghost transparent — all type=button', () => {
    const { container, getByText } = render(
      <>
        <Button variant="primary">Run</Button>
        <Button>Cancel</Button>
        <Button variant="ghost" size="sm">
          More
        </Button>
      </>
    );
    const primary = getByText('Run');
    expect(primary.getAttribute('type')).toBe('button');
    expect(primary.className).toContain('bg-lz-accent');
    expect(primary.className).toContain('text-lz-accent-ink');
    expect(primary.className).toContain('hover:bg-lz-accent-hover');
    const secondary = getByText('Cancel');
    expect(secondary.className).toContain('border-lz-border-strong');
    expect(secondary.className).toContain('bg-lz-surface');
    const ghost = getByText('More');
    expect(ghost.className).toContain('bg-transparent');
    expect(ghost.className).toContain('h-7');
    for (const b of [primary, secondary, ghost]) {
      expect(b.className).toContain('rounded-lz-control');
      expect(b.className).toContain('focus-visible:outline-ring');
      expect(b.className).toContain('duration-120');
    }
    assertStudioClean(container);
  });

  it('disabled is a solid neutral, never an opacity, and swallows clicks', () => {
    const onClick = vi.fn();
    const { container, getByText } = render(
      <Button variant="primary" disabled onClick={onClick}>
        Run
      </Button>
    );
    const b = getByText('Run') as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    expect(b.className).toContain('disabled:bg-lz-surface-2');
    expect(b.className).toContain('disabled:text-lz-ink-3');
    expect(b.className).not.toMatch(/opacity/);
    fireEvent.click(b);
    expect(onClick).not.toHaveBeenCalled();
    assertStudioClean(container);
  });

  it('carries a leading icon slot and forwards native props', () => {
    const onClick = vi.fn();
    const { getByLabelText } = render(
      <Button aria-label="Stop the run" icon={<svg data-testid="ic" />} onClick={onClick}>
        Stop
      </Button>
    );
    const b = getByLabelText('Stop the run');
    expect(b.querySelector('svg')).not.toBeNull();
    fireEvent.click(b);
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
