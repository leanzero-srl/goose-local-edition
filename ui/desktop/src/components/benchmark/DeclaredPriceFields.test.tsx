import { act, renderHook, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

const canonical = vi.fn(async (_provider: string, _model: string) => ({
  inputTokenCost: 1.25,
  outputTokenCost: 10,
  cacheReadTokenCost: null as number | null,
  currency: '$',
}));
vi.mock('../../utils/canonical', () => ({
  fetchCanonicalModelInfo: (provider: string, model: string) => canonical(provider, model),
}));

import { useDeclaredPrices } from './DeclaredPriceFields';

const base = {
  resetKey: 'run-1',
  billed: false,
  provider: 'google',
  model: 'gemini-4-argon',
  cloud: true,
};

describe('useDeclaredPrices — the prefill never blocks Publish', () => {
  it('prefills a run it can price, and Publish stays possible', async () => {
    const tokens = { promptTokens: 1_000, cachedTokens: 0, completionTokens: 10 };
    const { result } = renderHook(() => useDeclaredPrices({ ...base, tokens }));
    await waitFor(() => expect(result.current.prefilled).toBe(true));
    expect(result.current.fields).toEqual({ input: '1.25', output: '10', cached: '' });
    expect(result.current.problem).toBeNull();
    expect(result.current.prices).toEqual({ inputUsdPerMTok: 1.25, outputUsdPerMTok: 10 });
  });

  it('leaves the fields empty when the list has no cached-read price for a run that read cached tokens', async () => {
    canonical.mockClear();
    const tokens = { promptTokens: 1_000, cachedTokens: 900, completionTokens: 10 };
    const { result } = renderHook(() => useDeclaredPrices({ ...base, tokens }));
    await waitFor(() => expect(canonical).toHaveBeenCalled());
    await act(async () => {
      await canonical.mock.results[0].value;
      await Promise.resolve();
    });
    expect(result.current.prefilled).toBe(false);
    expect(result.current.fields).toEqual({ input: '', output: '', cached: '' });
    expect(result.current.problem).toBeNull();
  });

  it('does not prefill a run with no token counts', async () => {
    canonical.mockClear();
    const { result } = renderHook(() => useDeclaredPrices({ ...base, tokens: null }));
    await Promise.resolve();
    expect(canonical).not.toHaveBeenCalled();
    expect(result.current.prefilled).toBe(false);
    expect(result.current.problem).toBeNull();
  });
});
