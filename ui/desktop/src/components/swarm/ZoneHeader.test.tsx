import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ZoneHeader, ZONE_HUES, ZONE_LABEL_CLASS } from './ZoneHeader';
import { EYEBROW_CLASS } from './formationVisualState';

// A zone is NOT a node: its header is typography, never a hue. The old header painted a coloured
// square and coloured the name per zone (the benchmark read node-5 pink); this pins the doctrine.
describe('ZoneHeader — typography, not hue', () => {
  it('renders the label on the zone-header scale with no colour mark and no inline colour', () => {
    render(<ZoneHeader label="Fleet" explain="what each node is doing right now" right={<span>3 nodes</span>} />);
    const label = screen.getByText('Fleet');
    expect(label.className).toContain(ZONE_LABEL_CLASS);
    expect(label.className).toContain('text-text-primary');
    expect(label.getAttribute('style')).toBeNull();
    // No decorative mark before the name: the first child of the header IS the label.
    expect(label.parentElement?.firstElementChild).toBe(label);
    expect(screen.getByText(/what each node is doing right now/)).toHaveClass('text-text-secondary');
    expect(screen.getByText('3 nodes')).toBeInTheDocument();
  });

  it('the zone-header scale is the one uppercase register, shared with the ribbon eyebrow', () => {
    expect(ZONE_LABEL_CLASS).toBe('text-[11px] font-semibold uppercase tracking-[0.08em]');
    expect(EYEBROW_CLASS).toBe(ZONE_LABEL_CLASS);
  });

  it('no zone colour reads from the node ramp', () => {
    for (const hue of Object.values(ZONE_HUES)) expect(hue).not.toContain('--color-node-');
  });
});
