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
    // RETIRED phases are not offered without evidence: CONTRACTS (P1-4) and REVIEW (2447d145c) are
    // deleted from the engine, so a run with no proof it ran them draws no chip — Synthesize sits
    // immediately before Build, and nothing reads "Review — skipped" over a run that could never have
    // reviewed. RESEARCH is LIVE again (the v2 fan) and is always offered. The deleted Plan stage must
    // not reappear either.
    const labels = within(phases)
      .getAllByRole('listitem')
      .map((li) => li.textContent?.trim());
    expect(labels.indexOf('Synthesize')).toBe(labels.indexOf('Build') - 1);
    expect(labels.indexOf('Research')).toBe(labels.indexOf('Synthesize') - 1);
    expect(within(phases).queryByText('Contracts')).not.toBeInTheDocument();
    expect(within(phases).queryByText(/Review/)).not.toBeInTheDocument();
    // ASK and SPLIT are conditional (VA-138): with no evidence the run asked nothing and split
    // nothing, so neither chip is drawn — and nothing back-fills a green check for an ask that
    // never happened.
    expect(within(phases).queryByText(/Ask/)).not.toBeInTheDocument();
    expect(within(phases).queryByText(/Split/)).not.toBeInTheDocument();
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

  // THE DEFECT 2447d145c LEFT BEHIND: the engine stopped emitting `phase: review`, and until 'review'
  // joined RETIRED_PHASES every new run's ribbon read "Review — skipped" from the moment Build lit —
  // a chip asserting the run had bypassed a stage the engine no longer has. The evidence here is
  // exactly what foldRunPhase observes on a post-2447d145c stream (open, ask, research, synthesis,
  // plan_loaded): no review key at all.
  it('never offers Review to a new run — no chip, no "skipped", once Build lights', () => {
    render(
      <FormationRibbon
        phase="build"
        evidence={{ open: true, ask: true, research: true, synthesize: true, build: true }}
        nodes={[{ device: 'gabee-qwen', working: true }]}
      />
    );
    const phases = screen.getByRole('list', { name: 'Run phases' });
    const items = within(phases).getAllByRole('listitem');
    expect(items.map((li) => li.textContent?.trim())).toEqual([
      'Open',
      'Ask',
      'Research',
      'Synthesize',
      'Build',
      'Integrate',
      'Repair',
      'Done',
    ]);
    expect(items.filter((li) => li.dataset.state === 'skipped')).toHaveLength(0);
    expect(within(phases).getByText('Build').closest('li')).toHaveAttribute('data-state', 'active');
  });

  // An ARCHIVED run proves it ran the retired stages, so it keeps their chips in engine order —
  // old run.jsonl files still carry the research/review/contracts phase events and must render as
  // history (retired means "absent is not skipped", never "hidden").
  it('keeps Research, Review and Contracts for a run whose evidence proves it ran them', () => {
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
    expect(labels.indexOf('Review')).toBe(labels.indexOf('Synthesize') + 1);
    expect(labels.indexOf('Contracts')).toBe(labels.indexOf('Build') - 1);
    expect(within(phases).getByText('Review').closest('li')).toHaveAttribute(
      'data-state',
      'complete'
    );
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
