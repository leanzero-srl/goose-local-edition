import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import { formatMlxMode } from './mlxModeLabel';

const intl = createIntl({ locale: 'en', onError: () => undefined });
const hosting = {
  mode: 'hosting' as const,
  rank: 1,
  requester: 'workhorse',
  modelId: 'mlx-community/Qwen4-Flash-4bit',
  backend: 'jaccl',
};

describe('formatMlxMode — the part a peer Mac plays (owner 2026-10-27)', () => {
  it('a layer split: "Serving layers 17–47 for workhorse"', () => {
    const text = formatMlxMode(intl, { ...hosting, layers: { first: 17, last: 47 } }, null);
    expect(text).toMatch(/^Serving layers 17–47 for workhorse · /);
  });
  it('no requester known: another Mac, never an empty name', () => {
    expect(formatMlxMode(intl, { ...hosting, requester: '' }, null)).toMatch(
      /^Rank 1 of another Mac's split · /
    );
    expect(
      formatMlxMode(intl, { ...hosting, requester: '', layers: { first: 0, last: 16 } }, null)
    ).toMatch(/^Serving layers 0–16 for another Mac · /);
  });
  it('a tensor split keeps the rank line', () => {
    expect(formatMlxMode(intl, hosting, null)).toMatch(/^Rank 1 of workhorse's split · /);
  });
});
