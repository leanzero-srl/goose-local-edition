import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { RequestError } from '@agentclientprotocol/sdk';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { ProviderDetails } from '../../types/providers';
import ProviderConfigForm from './ProviderConfigForm';

const acp = vi.hoisted(() => ({ submit: vi.fn(), authenticate: vi.fn() }));

vi.mock('../settings/providers/modal/subcomponents/handlers/DefaultSubmitHandler', () => ({
  providerConfigSubmitHandler: acp.submit,
}));
vi.mock('../../acp/providers', () => ({ acpAuthenticateProvider: acp.authenticate }));
vi.mock('../settings/providers/modal/subcomponents/ProviderLogo', () => ({
  default: () => null,
}));

// What goosed answers for a refused key: the JSON-RPC code's name as `message`, and the refusal
// (built by `key_connection::save_failure_text`) as `data`.
const REFUSAL =
  'OpenAI rejected the key (401 Unauthorized) at http://127.0.0.1:8899/v1/models: Incorrect API key provided: sk-bad. Your previous settings were kept.';

function provider(overrides: Partial<ProviderDetails['metadata']> = {}): ProviderDetails {
  return {
    name: 'openai',
    is_configured: false,
    provider_type: 'Preferred',
    metadata: {
      name: 'openai',
      display_name: 'OpenAI',
      description: 'GPT models',
      default_model: 'gpt-4o',
      known_models: [],
      model_doc_link: '',
      config_keys: [
        { name: 'OPENAI_API_KEY', required: true, secret: true, default: null, oauth_flow: false },
      ],
      ...overrides,
    },
  } as unknown as ProviderDetails;
}

function renderForm(details: ProviderDetails) {
  return render(
    <IntlTestWrapper>
      <ProviderConfigForm provider={details} onConfigured={vi.fn()} />
    </IntlTestWrapper>
  );
}

describe('onboarding ProviderConfigForm errors (Q-478)', () => {
  beforeEach(() => {
    acp.submit.mockReset();
    acp.authenticate.mockReset();
  });

  it("shows the provider's refusal and status, not the JSON-RPC code's name", async () => {
    acp.submit.mockRejectedValue(new RequestError(-32602, 'Invalid params', REFUSAL));
    const { container } = renderForm(provider());
    const key = container.querySelector('input') as HTMLInputElement;
    fireEvent.change(key, { target: { value: 'sk-bad' } });
    fireEvent.submit(container.querySelector('form')!);

    const error = await screen.findByTestId('provider-config-error');
    expect(error.textContent).toBe(REFUSAL);
    expect(error.textContent).toContain('401');
    expect(error.textContent).not.toContain('Invalid params');
  });

  it('a failed sign-in carries the same reason', async () => {
    acp.authenticate.mockRejectedValue(
      new RequestError(-32602, 'Invalid params', 'the browser sign-in was cancelled')
    );
    renderForm(
      provider({
        config_keys: [
          { name: 'TOKEN', required: true, secret: true, default: null, oauth_flow: true },
        ],
      } as unknown as Partial<ProviderDetails['metadata']>)
    );
    fireEvent.click(screen.getByRole('button', { name: /Sign in with OpenAI/ }));
    const error = await screen.findByTestId('provider-config-error');
    expect(error.textContent).toBe('Sign-in failed: the browser sign-in was cancelled');
  });
});
