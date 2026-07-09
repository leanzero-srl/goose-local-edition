import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import FanInCard, { type NodeLane } from './FanInCard';

const lanes: NodeLane[] = [
  { device: 'm4-max', action: 'edit auth.rs', status: 'done' },
  { device: 'm3-ultra', action: 'grep callsites', status: 'running' },
  { device: 'studio-2', action: 'cargo test', status: 'error' },
];

const STATUS_COLORS = ['#f5a623', '#2ecc71', '#ff3b30'];

describe('FanInCard', () => {
  it('renders inline node chips with solid formation hues, disjoint from status', () => {
    const { getAllByTestId } = render(<FanInCard dispatch="dispatch" lanes={lanes} />);
    const chips = getAllByTestId('node-chip') as HTMLElement[];
    expect(chips).toHaveLength(3);
    expect(chips[0].textContent).toBe('⬢A');
    expect(chips[1].textContent).toBe('⬢B');
    expect(chips[2].textContent).toBe('⬢C');

    // Each chip's color is a solid formation hue, and NONE equals a status color.
    for (const chip of chips) {
      const color = chip.style.color.replace(/\s/g, '');
      expect(color).not.toBe(''); // a solid color, not a faded/absent tint
      // jsdom keeps hex; normalize both sides just in case
      const asHex = color.startsWith('#') ? color.toLowerCase() : color;
      for (const status of STATUS_COLORS) {
        expect(asHex).not.toBe(status);
      }
    }
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
    // sharp radius (not a childish-rounded pill)
    expect(card.style.borderRadius).toBe('3px');
    // one SVG status icon per lane, each with an explicit (dark-mode-safe) color — never a bare glyph
    const statuses = getAllByTestId('node-status');
    expect(statuses).toHaveLength(3);
    for (const s of statuses) {
      expect(s.tagName.toLowerCase()).toBe('svg');
      expect(s.style.color.replace(/\s/g, '')).not.toBe('');
    }
    const text = container.textContent ?? '';
    expect(text).not.toContain('⏺'); // not Claude Code's glyph
    expect(text).toContain('fan-in');
  });
});
