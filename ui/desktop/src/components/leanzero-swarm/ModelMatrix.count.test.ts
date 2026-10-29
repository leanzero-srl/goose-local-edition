import { describe, expect, it } from 'vitest';
import { matrixRowCount } from './ModelMatrix';
import type { MacFacts, MacsValue } from './useMacs';

type Ctx = Parameters<typeof matrixRowCount>[0];

const ctx = (models: Record<string, MacFacts['models']>): Ctx =>
  ({
    macs: Object.keys(models).map((key) => ({ key })) as unknown as MacsValue['macs'],
    factsOf: (key: string) => ({ models: models[key] ?? null }) as MacFacts,
    downloads: {},
    copies: {},
  }) as Ctx;

const model = (id: string) => ({ id }) as NonNullable<MacFacts['models']>[number];

describe('matrixRowCount', () => {
  it('is unknown (null) until some Mac has been read, never a fake 0', () => {
    expect(matrixRowCount(ctx({ self: null, peer: null }))).toBeNull();
  });

  it('counts once any Mac is read, a read empty folder included (the control)', () => {
    expect(matrixRowCount(ctx({ self: [], peer: null }))).toBe(0);
    expect(matrixRowCount(ctx({ self: [model('a/x'), model('a/y')], peer: null }))).toBe(2);
    expect(matrixRowCount(ctx({ self: [model('a/x')], peer: [model('a/x'), model('b/z')] }))).toBe(
      2
    );
  });
});
