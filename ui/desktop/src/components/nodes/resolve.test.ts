import { describe, expect, it } from 'vitest';
import fixture from '../../../../../crates/goose/src/nodes/nodes.fixture.json';
import type { NodeRole, NodeRoleEntry, NodeStrategy } from './model';
import { resolve, sentenceFacts, type Decision, type Facts, type ShareState } from './resolve';

interface ResolveCase {
  name: string;
  entry: NodeRoleEntry;
  facts: Facts;
  sticky?: string;
  share: ShareState;
  expect: Decision;
  shareAfter: ShareState;
}

const cases = fixture.resolve as unknown as ResolveCase[];

describe('chain resolution — the same fixture as the router', () => {
  it('holds the whole matrix: 3 when-rules × 6 situations of the 1st × chains of 1, 2 and 3', () => {
    const matrix = cases.filter((c) => / · chain of [123]$/.test(c.name));
    expect(matrix.length).toBe(54);
    expect(cases.length).toBeGreaterThan(54);
  });

  for (const c of cases) {
    it(c.name, () => {
      const before = JSON.stringify(c.share);
      const { decision, share } = resolve(c.entry, c.facts, c.sticky ?? null, c.share);
      expect(decision).toEqual(c.expect);
      expect(share).toEqual(c.shareAfter);
      expect(JSON.stringify(c.share), "the caller's state is not mutated").toBe(before);
    });
  }

  it('shares 2:1 as a, b, a — deterministic, never random', () => {
    const entry: NodeRoleEntry = {
      chain: [
        { node: 'a', weight: 2 },
        { node: 'b', weight: 1 },
      ],
      when: 'share',
      ifNotLoaded: 'load',
    };
    const facts: Facts = { a: { kind: 'servable' }, b: { kind: 'servable' } };
    let share: ShareState = {};
    const picks: string[] = [];
    for (let i = 0; i < 6; i++) {
      const out = resolve(entry, facts, null, share);
      share = out.share;
      if (out.decision.kind !== 'serve') throw new Error(JSON.stringify(out.decision));
      picks.push(out.decision.node);
    }
    expect(picks).toEqual(['a', 'b', 'a', 'a', 'b', 'a']);
  });

  it('never serves a node it has no fact for (negative control)', () => {
    const entry: NodeRoleEntry = {
      chain: [{ node: 'ghost', weight: 1 }],
      when: 'failover',
      ifNotLoaded: 'load',
    };
    expect(resolve(entry, {}, null, {}).decision).toEqual({
      kind: 'exhausted',
      tried: [{ node: 'ghost', why: { kind: 'unknown' } }],
    });
  });
});

describe('sentence facts', () => {
  for (const c of fixture.sentenceFacts) {
    it(`${c.strategy.name} · ${c.role}`, () => {
      expect(
        sentenceFacts(
          c.strategy as unknown as NodeStrategy,
          c.role as NodeRole,
          c.names as Record<string, string>
        )
      ).toEqual(c.expect);
    });
  }
});
