import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { StatusDot } from './StatusDot';
import { assertStudioClean } from './assertStudioClean';

describe('lz/StatusDot', () => {
  it('is an 8px solid dot whose meaning is in the label', () => {
    const { container, getByRole } = render(<StatusDot tone="ok" label="healthy" />);
    const dot = getByRole('img');
    expect(dot.getAttribute('aria-label')).toBe('healthy');
    expect(dot.className).toContain('size-2');
    expect(dot.className).toContain('rounded-lz-pill');
    expect(dot.className).toContain('bg-lz-ok');
    expect(dot.className).not.toContain('animate-');
    assertStudioClean(container);
  });

  it('takes a node hue, and live pulses by SCALE on the motion token', () => {
    const { container, getByLabelText } = render(
      <>
        <StatusDot node={5} label="node E" />
        <StatusDot tone="accent" live label="streaming" size={10} />
      </>
    );
    expect(getByLabelText('node E').className).toContain('bg-lz-node-5');
    const live = getByLabelText('streaming');
    expect(live.className).toContain('animate-lz-live');
    expect(live.className).toContain('size-2.5');
    expect(live.getAttribute('data-live')).toBe('true');
    assertStudioClean(container);
  });
});
