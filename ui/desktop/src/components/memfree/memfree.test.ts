import { describe, expect, it } from 'vitest';
import {
  DEFAULT_NEVER_TOUCH,
  GIB,
  afterLine,
  effectiveNeverTouch,
  forceStopBundle,
  neverTouchExtrasFrom,
  neverTouchHit,
  neverTouchMatch,
  nuclearRequest,
  nuclearRows,
  unloadOffer,
  type MemfreeGroup,
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
      withGoose: true,
    });
  });

  it('goose holds 0 -> fitsWithApps names only the apps, never goose', () => {
    const line = verdictLine({
      ...GABEE,
      gooseOwnedBytes: 0,
      shortBytes: gb(4.3),
      verdict: { kind: 'fitsWithApps', remainingBytes: 0, appsToQuit: ['com.google.Chrome'] },
    });
    expect(line).toEqual({
      kind: 'fitsWithApps',
      freeGb: gb(7.2) / GIB,
      apps: ['Google Chrome'],
      withGoose: false,
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

const grp = (id: string, category: string, extra: Partial<MemfreeGroup> = {}): MemfreeGroup => ({
  id,
  label: id,
  category,
  bytes: gb(1),
  pids: [{ pid: 1, startedAt: 1 }],
  quittable: category === 'userApp',
  reclaimableByGoose: category !== 'userApp' && category !== 'job' && category !== 'system',
  bundleId: category === 'userApp' ? id.replace(/^app:/, '') : null,
  ...extra,
});

describe('never-touch list', () => {
  it('reads a list, a JSON string or a comma list; drops blanks, dupes and defaults', () => {
    expect(neverTouchExtrasFrom(['Foo', ' foo ', '', 'us.zoom.xos', 'Bar'])).toEqual([
      'Foo',
      'Bar',
    ]);
    expect(neverTouchExtrasFrom('["a","b"]')).toEqual(['a', 'b']);
    expect(neverTouchExtrasFrom('a, b ,')).toEqual(['a', 'b']);
    expect(neverTouchExtrasFrom(null)).toEqual([]);
    expect(neverTouchExtrasFrom('[broken')).toEqual([]);
  });

  it('effective = defaults first, then additions', () => {
    expect(effectiveNeverTouch(['Foo'])).toEqual([...DEFAULT_NEVER_TOUCH, 'Foo']);
  });

  it('matches bundle id, label or the name in the id, case-insensitively', () => {
    const teams = grp('app:com.microsoft.teams2', 'userApp', { label: 'Microsoft Teams' });
    expect(neverTouchHit(teams, DEFAULT_NEVER_TOUCH)).toBe('com.microsoft.teams2');
    expect(neverTouchHit(grp('ai:/Applications/LM Studio.app', 'aiServer'), ['lm studio'])).toBe(
      'lm studio'
    );
    expect(neverTouchHit(grp('app:com.apple.TextEdit', 'userApp'), DEFAULT_NEVER_TOUCH)).toBeNull();
  });

  it('C7: a natural name or a bundle prefix matches by contains, like the engine', () => {
    const teams = grp('app:com.microsoft.teams2', 'userApp', { label: 'Microsoft Teams' });
    expect(neverTouchHit(teams, ['Teams'])).toBe('Teams');
    expect(neverTouchHit(teams, ['com.microsoft'])).toBe('com.microsoft');
    expect(neverTouchHit(teams, ['  ', 'Slack'])).toBeNull();
  });
});

describe('never-touch match hint (review C7)', () => {
  const teams = grp('app:com.microsoft.teams2', 'userApp', { label: 'Microsoft Teams' });
  const installed = [
    { name: 'Final Cut Pro', bundleId: 'com.apple.FinalCut' },
    { name: 'Broken', bundleId: null },
  ];
  it('names the running holder first, then an installed app, by contains', () => {
    expect(neverTouchMatch('Teams', [teams], installed)).toBe('Microsoft Teams');
    expect(neverTouchMatch('final cut', [teams], installed)).toBe('Final Cut Pro');
    expect(neverTouchMatch('com.apple.finalcut', [], installed)).toBe('Final Cut Pro');
  });
  it('an entry that names nothing running or installed matches nothing', () => {
    expect(neverTouchMatch('Photoshop', [teams], installed)).toBeNull();
    expect(neverTouchMatch('   ', [teams], installed)).toBeNull();
  });
});

describe('unload offer', () => {
  it('only a server goose can free now, never a protected one; disabled when remote is off', () => {
    expect(unloadOffer(grp('mlx:97100', 'gooseEngine'), true)).toEqual({
      show: true,
      disabled: false,
    });
    expect(unloadOffer(grp('ai:lms', 'aiServer'), false)).toEqual({ show: true, disabled: true });
    expect(unloadOffer(grp('ai:lms', 'aiServer', { protectedWhy: 'launchd' }), true).show).toBe(
      false
    );
    expect(unloadOffer(grp('gooseEngine', 'gooseEngine'), true).show).toBe(false);
    expect(unloadOffer(grp('app:x', 'userApp'), true).show).toBe(false);
  });
});

describe('nuclear rows + request', () => {
  const groups = [
    grp('mlx:97100', 'gooseEngine', { usedBy: ['sshd-session'], nuclearDefault: false }),
    grp('ai:lms', 'aiServer', { nuclearDefault: true }),
    grp('job:mcp', 'job', { protectedWhy: 'MCP server' }),
    grp('app:com.microsoft.teams2', 'userApp', { nuclearDefault: true }),
    grp('app:com.apple.TextEdit', 'userApp', { nuclearDefault: true }),
    grp('job:rclone', 'job', { why: 'a job' }),
    grp('gooseEngine', 'gooseEngine'),
  ];
  const rows = nuclearRows(groups, DEFAULT_NEVER_TOUCH);
  const kind = (id: string) => rows.find((r) => r.group.id === id)?.kind;

  it('protected and never-touch are never offered; goose’s own is its own row', () => {
    expect(kind('job:mcp')).toBe('protected');
    expect(kind('app:com.microsoft.teams2')).toBe('protected');
    expect(kind('job:rclone')).toBe('left');
    expect(kind('gooseEngine')).toBeUndefined();
    const offered = rows.filter((r) => r.kind === 'offer');
    expect(offered.map((r) => [r.group.id, r.kind === 'offer' && r.ticked])).toEqual([
      ['mlx:97100', false],
      ['ai:lms', false],
      ['app:com.apple.TextEdit', true],
    ]);
  });

  it('C1/C2: a server row is never ticked, even when an older engine says nuclearDefault', () => {
    const server = rows.find((r) => r.group.id === 'ai:lms');
    expect(server?.kind === 'offer' && server.ticked).toBe(false);
  });

  it('sends only what is ticked; a ticked protected id is dropped', () => {
    const ticked = new Set([
      'ai:lms',
      'app:com.apple.TextEdit',
      'job:mcp',
      'app:com.microsoft.teams2',
    ]);
    expect(nuclearRequest(rows, ticked, false, ['x'])).toEqual({
      gooseOwned: false,
      quitApps: ['com.apple.TextEdit'],
      unloadServers: ['ai:lms'],
      seenUsedBy: { 'ai:lms': [] },
      forceStopApps: [],
      nuclear: true,
      neverTouch: ['x'],
    });
    const mlx = nuclearRequest(rows, new Set(['mlx:97100']), false, []);
    expect(mlx.seenUsedBy).toEqual({ 'mlx:97100': ['sshd-session'] });
    expect(nuclearRequest(rows, new Set(), true, []).quitApps).toEqual([]);
  });
});

describe('force stop offer (C4)', () => {
  const item = (extra: object) => ({
    id: 'app:com.apple.TextEdit',
    label: 'TextEdit',
    ok: false,
    message: 'm',
    ...extra,
  });
  it('only an app item the engine marked forceStoppable', () => {
    expect(forceStopBundle(item({ forceStoppable: true }))).toBe('com.apple.TextEdit');
    expect(forceStopBundle(item({}))).toBeNull();
    expect(forceStopBundle(item({ forceStoppable: true, id: 'ai:lms' }))).toBeNull();
  });
});
