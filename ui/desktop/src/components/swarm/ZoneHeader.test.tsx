import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ZoneHeader, ZONE_LABEL_CLASS } from './ZoneHeader';
import { EYEBROW_CLASS } from './formationVisualState';
import { assertStudioClean } from '../lz/assertStudioClean';

// A zone is NOT a node: its header is typography, never a hue. The old header painted a coloured
// square and coloured the name per zone (the benchmark read node-5 pink); this pins the doctrine.
describe('ZoneHeader — typography, not hue', () => {
  it('renders the label on the Studio zone step with no colour mark and no inline colour', () => {
    const { container } = render(
      <ZoneHeader
        label="Fleet"
        explain="what each node is doing right now"
        count={3}
        right={<span>3 working</span>}
      />
    );
    const label = screen.getByText('Fleet');
    // The zone register is the SectionHeader's h2 — the Studio `zone` type step, ink-2.
    const heading = label.closest('h2');
    expect(heading).not.toBeNull();
    expect(heading!.className).toContain('text-lz-zone');
    expect(heading!.className).toContain('uppercase');
    expect(heading!.className).toContain('text-lz-ink-2');
    expect(label.getAttribute('style')).toBeNull();
    // No decorative mark before the name: the first child of the heading IS the label.
    expect(heading!.firstElementChild).toBe(label);
    // The explainer is meta ink-3 in normal case — never a second uppercase register.
    const explain = screen.getByText(/what each node is doing right now/);
    expect(explain.className).toContain('normal-case');
    expect(explain.className).toContain('text-lz-ink-3');
    // The count pill is the SectionHeader's own.
    expect(screen.getByTestId('lz-section-count').textContent).toBe('3');
    expect(screen.getByText('3 working')).toBeInTheDocument();
    assertStudioClean(container);
  });

  it('the zone-header scale is the one uppercase register, shared with the ribbon eyebrow', () => {
    expect(ZONE_LABEL_CLASS).toBe('text-lz-zone uppercase');
    expect(EYEBROW_CLASS).toBe(ZONE_LABEL_CLASS);
  });

  it('a collapsible zone is one button that says whether it is open', () => {
    const { container } = render(
      <ZoneHeader label="Event log" collapsed onToggle={() => {}} right={<span>12 events</span>} />
    );
    const button = screen.getByRole('button');
    expect(button).toHaveAttribute('aria-expanded', 'false');
    expect(button.textContent).toContain('Event log');
    expect(button.textContent).toContain('12 events');
    assertStudioClean(container);
  });
});
