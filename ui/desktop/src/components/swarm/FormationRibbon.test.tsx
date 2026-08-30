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
    // RETIRED phases are not offered without evidence: RESEARCH and CONTRACTS are deleted from the
    // engine (P1-5/P1-4), so a run with no proof it ran them draws neither chip — Review sits
    // immediately before Build. The deleted Plan stage must not reappear either.
    const labels = within(phases)
      .getAllByRole('listitem')
      .map((li) => li.textContent?.trim());
    expect(labels.indexOf('Review')).toBe(labels.indexOf('Build') - 1);
    expect(within(phases).queryByText('Research')).not.toBeInTheDocument();
    expect(within(phases).queryByText('Contracts')).not.toBeInTheDocument();
    expect(within(phases).getByText('Ask').closest('li')).toHaveAttribute('data-state', 'complete');
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

  // FINDING 16 of the frontend truth review: pressing Pause used to null the phase, which returned
  // every chip to 'upcoming' — the ribbon un-completed a run four phases in until Resume. Held now
  // keeps the position and drops only the work claim: grey outline, no fill, ' — held'.
  it('held keeps completed history lit and renders the active chip as held, not working', () => {
    render(
      <FormationRibbon
        phase="build"
        held
        evidence={{ open: true, ask: true, synthesize: true, review: true, build: true }}
        nodes={[{ device: 'gabee-qwen', working: false }]}
      />
    );
    const phases = screen.getByRole('list', { name: 'Run phases' });
    expect(within(phases).getByText('Synthesize').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
    const buildChip = within(phases).getByText('Build — held').closest('li');
    expect(buildChip).toHaveAttribute('data-state', 'active');
    expect(buildChip).toHaveAttribute('data-held', 'true');
  });

  // An ARCHIVED run proves it ran the retired stages, so it keeps their chips in engine order —
  // old run.jsonl files still carry the research/contracts phase events and must render as history.
  it('keeps Research and Contracts for a run whose evidence proves it ran them', () => {
    render(
      <FormationRibbon
        phase="build"
        nodes={[{ device: 'gabee-qwen', working: true }]}
        evidence={{
          open: true,
          ask: true,
          research: true,
          synthesize: true,
          review: true,
          contracts: true,
        }}
      />
    );
    const phases = screen.getByRole('list', { name: 'Run phases' });
    const labels = within(phases)
      .getAllByRole('listitem')
      .map((li) => li.textContent?.trim());
    expect(labels.indexOf('Research')).toBe(labels.indexOf('Synthesize') - 1);
    expect(labels.indexOf('Contracts')).toBe(labels.indexOf('Build') - 1);
    expect(within(phases).getByText('Contracts').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
    expect(within(phases).getByText('Research').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
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
