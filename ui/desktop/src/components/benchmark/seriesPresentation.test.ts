import { describe, expect, it } from 'vitest';
import { acceptsSeriesUpdate, groupBenchmarkHistory } from './seriesPresentation';
import { seriesFixture, completedSeriesFixture, sessionsFixture } from './series.fixture';

describe('series lifecycle identity', () => {
  it('rejects old first-build and untagged events after the second build starts', () => {
    const first = seriesFixture();
    const second = { ...first, currentBuild: 2 as const };
    expect(acceptsSeriesUpdate(second, first)).toBe(false);
    expect(acceptsSeriesUpdate(second, undefined)).toBe(false);
    expect(acceptsSeriesUpdate(second, { ...first, id: 'other' })).toBe(false);
    expect(acceptsSeriesUpdate(first, second)).toBe(true);
  });
  it('cannot resurrect a finished series from a delayed running snapshot', () => {
    const completed = completedSeriesFixture();
    expect(acceptsSeriesUpdate(completed, { ...completed, running: true })).toBe(false);
    expect(acceptsSeriesUpdate(completed, completed)).toBe(true);
    expect(acceptsSeriesUpdate(completed, undefined)).toBe(true);
  });
  it('keeps the pair together and child identities stable in a newest-first history', () => {
    const pair = completedSeriesFixture();
    const [first, second] = sessionsFixture(pair);
    const ordinary = { ...first, runId: 'ordinary', seriesId: undefined, buildIndex: undefined };
    const groups = groupBenchmarkHistory([second, ordinary, first]);
    expect(groups.map((group) => [group.kind, group.key])).toEqual([
      ['series', pair.id],
      ['run', 'ordinary'],
    ]);
    expect(groups[0].sessions.map((child) => child.buildIndex)).toEqual([1, 2]);
  });
});
