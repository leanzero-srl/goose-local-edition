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
    expect(within(phases).getByText('Synthesize').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
    expect(within(phases).getByText('Integrate').closest('li')).toHaveAttribute(
      'data-state',
      'upcoming'
    );
    // The deleted stages must not reappear as labels on a ribbon that no longer runs them.
    expect(within(phases).queryByText('Contracts')).not.toBeInTheDocument();
    expect(within(phases).queryByText('Plan')).not.toBeInTheDocument();

    const nodes = screen.getAllByTestId('formation-node');
    expect(nodes).toHaveLength(2);
    expect(nodes[0]).toHaveAttribute('data-node-state', 'working');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'idle');
    expect(screen.getByLabelText('Node A, gabee, working')).toBeInTheDocument();
    // The working/idle COUNT moved to the FLEET header, which is where the nodes are named. The ribbon
    // keeps the chips, which carry WHICH node is lit — the fact the count could not express.
    expect(screen.getByText('2 nodes')).toBeInTheDocument();
  });

  // A held run has no phase. The ribbon must show nothing active rather than the old regex fallback,
  // which asserted "Build active" over a fleet that was deliberately doing nothing.
  it('lights no step and places no node when the run has no phase', () => {
    render(<FormationRibbon phase={null} nodes={[{ device: 'gabee-qwen', working: false }]} />);
    expect(screen.getByTestId('formation-ribbon')).toHaveAttribute('data-active-phase', 'none');
    const phases = screen.getByRole('list', { name: 'Run phases' });
    expect(
      within(phases)
        .queryAllByRole('listitem')
        .filter((li) => li.dataset.state === 'active')
    ).toHaveLength(0);
    expect(screen.queryAllByTestId('formation-node')).toHaveLength(0);
  });

  it('does not claim conditional integration or repair completed without engine evidence', () => {
    render(
      <FormationRibbon
        phase="done"
        evidence={{
          open: true,
          research: true,
          synthesize: true,
          review: true,
          build: true,
          integrate: false,
          repair: false,
        }}
        nodes={[]}
      />
    );

    expect(screen.getByText('Integrate — skipped').closest('li')).toHaveAttribute(
      'data-state',
      'skipped'
    );
    expect(screen.getByText('Repair — skipped').closest('li')).toHaveAttribute(
      'data-state',
      'skipped'
    );
    expect(screen.getByText('Done').closest('li')).toHaveAttribute('data-state', 'active');
  });
});
