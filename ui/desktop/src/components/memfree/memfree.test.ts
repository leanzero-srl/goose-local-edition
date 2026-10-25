import { describe, expect, it } from 'vitest';
import {
  GIB,
  afterLine,
  groupByCategory,
  memfreeEnabledFrom,
  minMbFrom,
  quitOffer,
  verdictLine,
  verdictOf,
} from './memfree';
import { GABEE, gb } from './memfree.fixtures';

describe('verdictOf (mirror of the engine rule)', () => {
  it('Gabee: nothing goose owns, apps not enough -> cannotFit, 5.7 GB short after everything', () => {
    const v = verdictOf(gb(21.2), 0, [{ bundleId: 'all', bytes: gb(15.5) }]);
    expect(v.kind).toBe('cannotFit');
    expect(v.remainingBytes / GIB).toBeCloseTo(5.7, 1);
  });

  it('goose own covers it -> fitsByGooseOwned, no apps', () => {
    expect(verdictOf(gb(4), gb(6), [{ bundleId: 'a', bytes: gb(10) }])).toEqual({
      kind: 'fitsByGooseOwned',
      remainingBytes: 0,
      appsToQuit: [],
    });
  });

  it('goose own + apps covers it -> fitsWithApps naming only the biggest apps it takes', () => {
    const v = verdictOf(gb(10), gb(3), [
      { bundleId: 'small', bytes: gb(1) },
      { bundleId: 'big', bytes: gb(5) },
      { bundleId: 'mid', bytes: gb(3) },
    ]);
    expect(v.kind).toBe('fitsWithApps');
    expect(v.appsToQuit).toEqual(['big', 'mid']);
  });

  it('exactly enough counts as fitting', () => {
    expect(verdictOf(gb(5), gb(5), []).kind).toBe('fitsByGooseOwned');
  });
});

describe('verdictLine', () => {
  it('Gabee fixture -> cannotFit with everything 15.5, short 5.7 and both alternatives', () => {
    const line = verdictLine(GABEE);
    expect(line?.kind).toBe('cannotFit');
    if (line?.kind !== 'cannotFit') return;
    expect(line.everythingGb).toBeCloseTo(15.5, 1);
    expect(line.shortGb).toBeCloseTo(5.7, 1);
    expect(line.alternatives.map((a) => a.kind)).toEqual(['smallerVariant', 'peerMac']);
  });

  it('prefers the engine verdict and names its apps by label', () => {
    const line = verdictLine({
      ...GABEE,
      gooseOwnedBytes: gb(8),
      shortBytes: gb(12),
      verdict: { kind: 'fitsWithApps', remainingBytes: 0, appsToQuit: ['com.google.Chrome'] },
    });
    expect(line).toEqual({
      kind: 'fitsWithApps',
      freeGb: (gb(8) + gb(7.2)) / GIB,
      apps: ['Google Chrome'],
    });
  });

  it('fitsByGooseOwned says what goose frees', () => {
    const line = verdictLine({ ...GABEE, gooseOwnedBytes: gb(22), shortBytes: gb(21.2) });
    expect(line?.kind).toBe('fitsByGooseOwned');
    if (line?.kind === 'fitsByGooseOwned') expect(line.freeGb).toBeCloseTo(22, 1);
  });

  it('nothing short -> no verdict line', () => {
    expect(verdictLine({ ...GABEE, shortBytes: 0 })).toBeNull();
    expect(verdictLine({ ...GABEE, shortBytes: null })).toBeNull();
  });

  it('drops alternative kinds it does not know', () => {
    const line = verdictLine({ ...GABEE, alternatives: [{ kind: 'futureThing' }] });
    expect(line?.kind === 'cannotFit' && line.alternatives).toEqual([]);
  });
});

describe('groupByCategory', () => {
  it('orders categories, sums them and sorts holders biggest first', () => {
    const blocks = groupByCategory(GABEE.groups);
    expect(blocks.map((b) => b.category)).toEqual(['userApp', 'system']);
    expect(blocks[0].bytes / GIB).toBeCloseTo(15.5, 1);
    expect(blocks[0].groups.map((g) => g.label)).toEqual([
      'Google Chrome',
      'Safari',
      'Microsoft Teams',
    ]);
  });
});

describe('quitOffer', () => {
  it('never offers quit for a system or goose holder', () => {
    expect(quitOffer(GABEE.groups[3], true)).toEqual({ show: false });
    expect(quitOffer({ ...GABEE.groups[0], category: 'gooseEngine' }, true)).toEqual({
      show: false,
    });
    expect(quitOffer({ ...GABEE.groups[0], quittable: false }, true)).toEqual({ show: false });
  });

  it('offers a user app, disabled where the Mac does not allow quitting', () => {
    expect(quitOffer(GABEE.groups[0], true)).toEqual({ show: true, disabled: false });
    expect(quitOffer(GABEE.groups[0], false)).toEqual({ show: true, disabled: true });
  });
});

describe('afterLine', () => {
  it('before -> after from the engine reading, and the new fit', () => {
    const a = afterLine({
      items: [],
      before: { budgetBytes: gb(9.4) },
      after: { budgetBytes: gb(16.6) },
      fit: { verdict: 'block', shortBytes: gb(14) },
    });
    expect(a.beforeGb).toBeCloseTo(9.4, 1);
    expect(a.afterGb).toBeCloseTo(16.6, 1);
    expect(a.freedGb).toBeCloseTo(7.2, 1);
    expect(a.fits).toBe(false);
    expect(a.shortGb).toBeCloseTo(14, 1);
  });

  it('a fit that now fits, and a budget that fell, never shows negative freed', () => {
    const a = afterLine({
      items: [],
      before: { budgetBytes: gb(9) },
      after: { budgetBytes: gb(8.9) },
      fit: { verdict: 'allow' },
    });
    expect(a.freedGb).toBe(0);
    expect(a.fits).toBe(true);
    expect(a.shortGb).toBeNull();
  });
});

describe('tunable readers', () => {
  it('enabled unless explicitly false', () => {
    expect(memfreeEnabledFrom(undefined)).toBe(true);
    expect(memfreeEnabledFrom(null)).toBe(true);
    expect(memfreeEnabledFrom(false)).toBe(false);
    expect(memfreeEnabledFrom(' False ')).toBe(false);
  });

  it('min MB defaults to 200 on garbage', () => {
    expect(minMbFrom(undefined)).toBe(200);
    expect(minMbFrom('abc')).toBe(200);
    expect(minMbFrom(-5)).toBe(200);
    expect(minMbFrom('350')).toBe(350);
  });
});
