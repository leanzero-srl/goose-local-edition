import { describe, it, expect } from 'vitest';
import { partitionProviderRows, providerRowState } from './cloudProviderState';
import type { ProviderDetails } from '../../types/providers';

const row = (name: string, facts: Partial<ProviderDetails>) =>
  ({
    name,
    provider_type: 'Preferred',
    is_configured: false,
    ...facts,
  }) as ProviderDetails;

describe('providerRowState — the one rule for chip, list and count', () => {
  it('reads the engine facts in order: error, checked, saved, nothing', () => {
    expect(
      providerRowState(
        row('openai', {
          credentials_saved: true,
          connection_checked: true,
          connection_error: '401',
        })
      )
    ).toBe('failed');
    expect(
      providerRowState(
        row('google', { is_configured: true, credentials_saved: true, connection_checked: true })
      )
    ).toBe('connected');
    expect(providerRowState(row('mistral', { credentials_saved: true }))).toBe('unchecked');
    expect(providerRowState(row('xai', {}))).toBe('not-set-up');
  });

  it('a saved OpenAI-compatible endpoint is never "not set up", even with its key missing', () => {
    expect(providerRowState(row('custom_gw', { provider_type: 'Custom' }))).toBe('unchecked');
    // A bundled declarative provider with a custom_ id is a cloud family, not an endpoint.
    expect(providerRowState(row('custom_deepseek', { provider_type: 'Custom' }))).toBe(
      'not-set-up'
    );
  });

  it('the count is the lengths of the two lists it returns', () => {
    const { configured, available, count } = partitionProviderRows([
      row('openai', { credentials_saved: true, connection_error: '401 Incorrect API key' }),
      row('google', {}),
      row('xai', {}),
    ]);
    expect(configured.map((r) => r.name)).toEqual(['openai']);
    expect(available.map((r) => r.name)).toEqual(['google', 'xai']);
    expect(count).toEqual({ configured: 1, total: 3 });
  });
});
