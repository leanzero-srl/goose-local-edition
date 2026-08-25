import { render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import FormationRibbon from './FormationRibbon';

describe('FormationRibbon', () => {
  it('places the real fleet under the active engine phase with working and idle truth', () => {
    render(
      <FormationRibbon
        phase="build"
        nodes={[
          { device: 'gabee-qwen', working: true },
          { device: 'mihai-qwen', working: false },
        ]}
      />
    );

    const ribbon = screen.getByTestId('formation-ribbon');
    expect(ribbon).toHaveAttribute('data-active-phase', 'build');
    const phases = screen.getByRole('list', { name: 'Run phases' });
    expect(within(phases).getByText('Build').closest('li')).toHaveAttribute('data-state', 'active');
    expect(within(phases).getByText('Plan').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
    expect(within(phases).getByText('Verify').closest('li')).toHaveAttribute(
      'data-state',
      'upcoming'
    );

    const nodes = screen.getAllByTestId('formation-node');
    expect(nodes).toHaveLength(2);
    expect(nodes[0]).toHaveAttribute('data-node-state', 'working');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'idle');
    expect(screen.getByLabelText('Node A, gabee, working')).toBeInTheDocument();
    expect(screen.getByText('1 working · 1 idle')).toBeInTheDocument();
  });
});
