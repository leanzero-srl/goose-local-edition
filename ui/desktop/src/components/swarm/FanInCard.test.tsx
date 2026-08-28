import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import FanInCard, { type NodeLane } from './FanInCard';
import { CHIP_RADIUS, FORMATION_RAMP } from './formationVisualState';

const lanes: NodeLane[] = [
  { device: 'm4-max', action: 'edit auth.rs', status: 'done' },
  { device: 'm3-ultra', action: 'grep callsites', status: 'running' },
  { device: 'studio-2', action: 'cargo test', status: 'error' },
];

describe('FanInCard', () => {
  it('renders inline node chips off the shared ramp, one distinct solid hue each', () => {
    const { getAllByTestId } = render(<FanInCard dispatch="dispatch" lanes={lanes} />);
    const chips = getAllByTestId('node-chip') as HTMLElement[];
    expect(chips).toHaveLength(3);
    expect(chips[0].textContent).toBe('⬢A');
    expect(chips[1].textContent).toBe('⬢B');
    expect(chips[2].textContent).toBe('⬢C');

    // Each chip takes its hue from the ONE shared ramp, in order, and no two lanes share one. This used to
    // assert hue-disjointness from the status triad instead — a check that went vacuous the moment the
    // colours became CSS tokens (a `var(...)` string never equals a hex literal, so it passed regardless).
    // Identity and status are told apart by their MARK — a filled chip versus an outline SVG icon — which
    // is what the next test pins, and which holds even where the ramp and the triad share a hue.
    const colors = chips.map((chip) => chip.style.color.replace(/\s/g, ''));
    for (const color of colors) expect(color).not.toBe(''); // solid, never a faded or absent tint
    expect(new Set(colors).size).toBe(colors.length);
    colors.forEach((color, i) => expect(color).toBe(FORMATION_RAMP[i].replace(/\s/g, '')));
  });

  it('is a sharp full-border card with no left-rail accent and SVG status icons colored per status', () => {
    const { getByTestId, getAllByTestId, container } = render(
      <FanInCard dispatch="dispatch" lanes={lanes} />
    );
    const card = getByTestId('fan-in-card');
    // full border, not a left rail
    expect(card.className).toContain('border ');
    expect(card.className).not.toMatch(/border-l\b/);
    expect(card.className).not.toMatch(/border-l-/);
    // The ONE panel/chip radius, from formationVisualState — restrained, never a pill, and never a value
    // this file invents for itself.
    expect(card.style.borderRadius).toBe(`${CHIP_RADIUS}px`);
    // one SVG status icon per lane, each with an explicit (dark-mode-safe) color — never a bare glyph
    const statuses = getAllByTestId('node-status');
    expect(statuses).toHaveLength(3);
    for (const s of statuses) {
      expect(s.tagName.toLowerCase()).toBe('svg');
      expect(s.style.color.replace(/\s/g, '')).not.toBe('');
    }
    const text = container.textContent ?? '';
    expect(text).not.toContain('⏺'); // not Claude Code's glyph
    // The footer repeated the lane count verbatim under a header that already gave it.
    expect(card.textContent).toContain('3 lanes');
  });
});
