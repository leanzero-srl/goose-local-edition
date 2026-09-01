import { createRef } from 'react';
import { Slot } from '@radix-ui/react-slot';
import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Button } from './Button';
import { allClasses, assertStudioClean } from './assertStudioClean';
import { missingUtilities } from './compileStudioCss';

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

  it('forwards its ref to the DOM button — directly and through a Radix Slot (asChild)', () => {
    const direct = createRef<HTMLButtonElement>();
    const slotted = createRef<HTMLButtonElement>();
    render(
      <>
        <Button ref={direct}>Run</Button>
        <Slot ref={slotted}>
          <Button>Slotted</Button>
        </Slot>
      </>
    );
    expect(direct.current).toBeInstanceOf(HTMLButtonElement);
    expect(direct.current?.textContent).toBe('Run');
    expect(slotted.current).toBeInstanceOf(HTMLButtonElement);
    expect(slotted.current?.textContent).toBe('Slotted');
    expect(Button.displayName).toBe('Button');
  });

  it('iconOnly is a square control (28 / 32) with its own class set and no text padding', async () => {
    const { container, getByLabelText } = render(
      <>
        <Button variant="ghost" size="sm" iconOnly aria-label="Close" icon={<svg />} />
        <Button iconOnly aria-label="Refresh" icon={<svg />} />
      </>
    );
    const sm = getByLabelText('Close');
    expect(sm.getAttribute('data-icon-only')).toBe('true');
    expect(sm.className).toContain('size-7');
    expect(sm.className).not.toMatch(/\bpx-|\bh-7\b/);
    const md = getByLabelText('Refresh');
    expect(md.className).toContain('size-8');
    expect(md.className).not.toMatch(/\bpx-|\bh-8\b/);
    assertStudioClean(container);
    expect(await missingUtilities(allClasses(container))).toEqual([]);
  }, 30_000);
});
