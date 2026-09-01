import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Toolbar } from './Toolbar';
import { assertStudioClean } from './assertStudioClean';

describe('lz/Toolbar', () => {
  it('is one 36px row: a plain text search (never a native search field or select), filters, actions', () => {
    const onChange = vi.fn();
    const { container, getByRole, getByLabelText, queryByLabelText, getByText } = render(
      <Toolbar
        aria-label="Models"
        search={{
          value: '',
          onChange,
          placeholder: 'Search models',
          'aria-label': 'Search models',
        }}
        filters={<span>filters</span>}
        actions={<button type="button">Add</button>}
      />
    );
    const bar = getByRole('toolbar');
    expect(bar.className).toContain('h-lz-row');
    const input = getByLabelText('Search models') as HTMLInputElement;
    expect(input.type).toBe('text');
    expect(input.className).toContain('border-lz-border-strong');
    expect(input.className).toContain('focus-visible:outline-ring');
    expect(queryByLabelText('Clear search')).toBeNull();
    fireEvent.change(input, { target: { value: 'qwen' } });
    expect(onChange).toHaveBeenLastCalledWith('qwen');
    expect(getByText('Add').parentElement?.className).toContain('ml-auto');
    assertStudioClean(container);
  });

  it('shows its own clear button once there is text, and it clears', () => {
    const onChange = vi.fn();
    const { container, getByLabelText } = render(
      <Toolbar search={{ value: 'qwen', onChange, 'aria-label': 'Search models' }} />
    );
    fireEvent.click(getByLabelText('Clear search'));
    expect(onChange).toHaveBeenLastCalledWith('');
    assertStudioClean(container);
  });
});
