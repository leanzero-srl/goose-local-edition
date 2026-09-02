import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import FanInCard, { type NodeLane } from './FanInCard';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const lanes: NodeLane[] = [
  { device: 'm4-max', action: 'edit auth.rs', status: 'done' },
  { device: 'm3-ultra', action: 'grep callsites', status: 'running' },
  { device: 'studio-2', action: 'cargo test', status: 'error' },
];

describe('FanInCard', () => {
  it('renders an identity dot per lane off the shared ramp, one distinct solid hue each', () => {
    const { getAllByTestId } = render(<FanInCard dispatch="dispatch" lanes={lanes} />);
    const chips = getAllByTestId('node-chip') as HTMLElement[];
    expect(chips).toHaveLength(3);
    // The letter is for the reader, not the eye: a dot carries the identity, the aria-label the name.
    expect(chips.map((c) => c.getAttribute('aria-label'))).toEqual(['node A', 'node B', 'node C']);
    for (const c of chips) expect(c.textContent).toBe('');

    // Each chip takes its hue from the ONE shared ramp — the Studio node token utility, in slot order —
    // and no two lanes share one. Identity and status are told apart by their MARK — a filled chip versus
    // an outline SVG icon — which is what the next test pins, and which holds even where the ramp and
    // the triad share a hue. The hue is a class, never an inline colour.
    const hues = chips.map((chip) => chip.className.match(/\bbg-lz-node-([1-6])\b/)?.[1]);
    expect(hues).toEqual(['1', '2', '3']);
    for (const c of chips) expect(c.getAttribute('style')).toBeNull();
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
    // The Studio control radius (6px), as the token utility — never a pill, never an inline value this
    // file invents for itself.
    expect(card.className).toContain('rounded-lz-control');
    expect(card.getAttribute('style')).toBeNull();
    // one SVG status icon per lane, each in its status-triad token class (theme-aware, so dark-mode-safe)
    // — never a bare glyph, never an inline colour
    const statuses = getAllByTestId('node-status');
    expect(statuses).toHaveLength(3);
    expect(statuses.map((s) => s.tagName.toLowerCase())).toEqual(['svg', 'svg', 'svg']);
    expect(
      statuses.map((s) => (s.getAttribute('class') ?? '').match(/\btext-lz-(ok|warn|err)\b/)?.[1])
    ).toEqual(['ok', 'warn', 'err']);
    for (const s of statuses) expect(s.getAttribute('style')).toBeNull();
    const text = container.textContent ?? '';
    expect(text).not.toContain('⏺'); // not Claude Code's glyph
    // The footer repeated the lane count verbatim under a header that already gave it.
    expect(card.textContent).toContain('3 lanes');
  });

  it('emits only classes that compile, and nothing the Studio bans', async () => {
    const { container } = render(<FanInCard dispatch="dispatch" lanes={lanes} />);
    assertStudioClean(container);
    const missing = await missingUtilities(
      allClasses(container).filter((c) => !c.startsWith('lucide'))
    );
    expect(missing).toEqual([]);
  });
});
