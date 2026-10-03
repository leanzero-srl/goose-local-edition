import { render, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { PartChips } from './ScoringDetail';
import { partLeaf, summarizeParts } from './partSummary';
import replay from './m-committed-event-replay.fixture.json';
import solar from './sb72-solar-noserve.fixture.json';

// The REAL check from the DeepSeek Pro sb-7.2 run (0.699): 138 KB of nested parts that rendered
// "corroboration [object Object]" on the run card.
const parts = replay.check.parts as Record<string, unknown>;

describe('a check whose parts nest objects', () => {
  it('reads as groups of key: value chips, never "[object Object]"', () => {
    const { container } = render(<PartChips parts={parts} />);
    const text = container.textContent ?? '';
    expect(text).not.toContain('[object Object]');
    // Bounded: the 138 KB of frames and pixel examples are counted, not printed.
    expect(text.length).toBeLessThan(2000);

    const groups = within(container).getAllByTestId('part-group');
    expect(groups.map((g) => g.dataset.part)).toEqual([
      'corroboration',
      'live',
      'replay',
      'semantics',
      'clockEvidence',
      'motionLegs',
    ]);
    const [corroboration, live, , semantics, clock, legs] = groups.map((g) => g.textContent ?? '');
    expect(corroboration).toContain('requestStatus 200');
    expect(corroboration).toContain('before id pay_00514 · version 1');
    expect(corroboration).toContain('wire id pay_00514 · amount_minor 8579000 · currency …');
    expect(live).toContain('frames 11 items');
    expect(live).toContain('excludedMotionFrames none');
    expect(live).toContain('visible failed');
    expect(semantics).toContain('restart failed');
    expect(semantics).toContain('exit passed');
    expect(clock).toContain('chosen draw');
    expect(clock).toContain('scores draw 0 · raf 0');
    expect(legs).toContain('committed live update');
    expect(legs).toContain('newer/stale versions and exit');
  });

  it('takes the verdict tone from each group’s own ok', () => {
    const views = summarizeParts(parts);
    const tone = (key: string) => {
      const v = views.find((x) => x.key === key);
      return v?.kind === 'group' ? v.ok : undefined;
    };
    expect(tone('corroboration')).toBe(true);
    expect(tone('live')).toBe(false);
    expect(tone('replay')).toBe(false);
    expect(tone('semantics')).toBe(false);
    expect(tone('clockEvidence')).toBeNull();
    const legs = views.find((x) => x.key === 'motionLegs');
    expect(legs?.kind === 'group' && legs.items.every((i) => i.kind === 'flag' && !i.ok)).toBe(true);
  });

  it('counts long lists and states empty ones', () => {
    expect(partLeaf('ids', Array.from({ length: 120 }, (_, i) => `pay_${i}`))).toEqual({
      key: 'ids',
      kind: 'text',
      text: '120 items',
    });
    expect(partLeaf('jpy_kwd', [1.0, 1.0])).toEqual({ key: 'jpy_kwd', kind: 'text', text: '1, 1' });
    expect(partLeaf('invented', [])).toEqual({ key: 'invented', kind: 'text', text: 'none' });
    expect(
      partLeaf('cases', [
        { path: '/a', ok: true },
        { path: '/b', ok: false },
      ])
    ).toEqual({ key: 'cases', kind: 'text', text: '1/2 ok', tone: 'err' });
  });

  it('renders every part of the real solar verdict without an object leaking through', () => {
    for (const row of solar.checks as { parts?: Record<string, unknown> }[]) {
      if (!row.parts) continue;
      const { container, unmount } = render(<PartChips parts={row.parts} />);
      expect(container.textContent ?? '').not.toContain('[object Object]');
      unmount();
    }
  });
});
