import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import { ProviderCard } from './ProviderCard';
import type { ProviderDetails } from '../../../../types/providers';

function provider(extra: Partial<ProviderDetails>): ProviderDetails {
  return {
    name: 'openai',
    is_configured: false,
    credentials_saved: true,
    connection_checked: true,
    provider_type: 'Preferred',
    metadata: {
      name: 'openai',
      display_name: 'OpenAI',
      description: '',
      default_model: '',
      model_doc_link: '',
      model_selection_hint: null,
      config_keys: [],
      known_models: [],
      setup_steps: [],
    },
    ...extra,
  } as unknown as ProviderDetails;
}

const show = (p: ProviderDetails) =>
  render(
    <ProviderCard provider={p} onConfigure={vi.fn()} onLaunch={vi.fn()} isOnboarding={false} />,
    {
      wrapper: IntlTestWrapper,
    }
  );

afterEach(() => cleanup());

describe('ProviderCard — the check result on the Provider Configuration grid', () => {
  it('a stopped local server says so, without an alarm', () => {
    show(provider({ connection_error: null, local_server_not_running: 'http://127.0.0.1:8090' }));
    const note = screen.getByTestId('provider-card-not-running-openai');
    expect(note).toHaveTextContent('Not running');
    expect(note).toHaveTextContent('Nothing is running at http://127.0.0.1:8090 yet.');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('a failed check is still an alert with the provider’s words', () => {
    show(provider({ connection_error: 'OpenAI rejected the key (401 Unauthorized)' }));
    expect(screen.getByRole('alert')).toHaveTextContent(
      'OpenAI rejected the key (401 Unauthorized)'
    );
    expect(screen.queryByTestId('provider-card-not-running-openai')).toBeNull();
  });
});
