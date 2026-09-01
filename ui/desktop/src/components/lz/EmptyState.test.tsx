import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { EmptyState } from './EmptyState';
import { TYPE } from './tokens';
import { assertStudioClean } from './assertStudioClean';

describe('lz/EmptyState', () => {
  it('is centered and branded: the icon sits in a solid accent block, the title is display type', () => {
    const { container, getByTestId, getByRole, getByText } = render(
      <EmptyState
        icon={<svg />}
        title="No runs yet"
        body="Start one from the Benchmark view."
        action={<button type="button">Start a run</button>}
      />
    );
    const root = getByTestId('lz-empty-state');
    expect(root.className).toContain('mx-auto');
    expect(root.className).toContain('text-center');
    const icon = getByTestId('lz-empty-state-icon');
    expect(icon.className).toContain('bg-lz-accent text-lz-accent-ink');
    expect(icon.className).toContain('rounded-lz-card');
    const title = getByRole('heading', { level: 2 });
    for (const c of TYPE.display.split(' ')) expect(title.className).toContain(c);
    expect(getByText('Start a run')).not.toBeNull();
    assertStudioClean(container);
  });

  it('renders without icon, body or action', () => {
    const { container, queryByTestId } = render(<EmptyState title="Nothing selected" />);
    expect(queryByTestId('lz-empty-state-icon')).toBeNull();
    expect(container.querySelector('p')).toBeNull();
    assertStudioClean(container);
  });
});
