import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Segmented } from './Segmented';
import { SURFACE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

const options = [
  { value: 'all', label: 'All' },
  { value: 'live', label: 'Live' },
  { value: 'gone', label: 'Gone', disabled: true },
  { value: 'done', label: 'Done' },
] as const;

describe('lz/Segmented', () => {
  it('is a radiogroup whose active segment is the accent fill with white ink', () => {
    const { container, getAllByRole, getByRole } = render(
      <Segmented aria-label="Lane filter" options={options} value="live" onChange={() => {}} />
    );
    expect(getByRole('radiogroup').getAttribute('aria-label')).toBe('Lane filter');
    const radios = getAllByRole('radio');
    expect(radios).toHaveLength(4);
    const active = radios[1];
    expect(active.getAttribute('aria-checked')).toBe('true');
    for (const c of SURFACE.selected.split(' ')) expect(active.className).toContain(c);
    expect(radios[0].className).not.toContain('bg-lz-accent');
    expect(radios[0].className).toContain('hover:bg-lz-surface-2');
    // roving tabindex: only the active segment is in the tab order
    expect(radios.map((r) => r.getAttribute('tabindex'))).toEqual(['-1', '0', '-1', '-1']);
    expect((radios[2] as HTMLButtonElement).disabled).toBe(true);
    assertStudioClean(container);
  });

  it('clicks select, arrows move and skip disabled segments, Home/End jump', () => {
    const onChange = vi.fn();
    const { getByRole, getAllByRole } = render(
      <Segmented aria-label="f" options={options} value="live" onChange={onChange} />
    );
    fireEvent.click(getAllByRole('radio')[0]);
    expect(onChange).toHaveBeenLastCalledWith('all');
    const group = getByRole('radiogroup');
    fireEvent.keyDown(group, { key: 'ArrowRight' });
    expect(onChange).toHaveBeenLastCalledWith('done'); // skipped the disabled "gone"
    fireEvent.keyDown(group, { key: 'ArrowLeft' });
    expect(onChange).toHaveBeenLastCalledWith('all');
    fireEvent.keyDown(group, { key: 'End' });
    expect(onChange).toHaveBeenLastCalledWith('done');
    fireEvent.keyDown(group, { key: 'Home' });
    expect(onChange).toHaveBeenLastCalledWith('all');
    expect(onChange).toHaveBeenCalledTimes(5);
  });
});
