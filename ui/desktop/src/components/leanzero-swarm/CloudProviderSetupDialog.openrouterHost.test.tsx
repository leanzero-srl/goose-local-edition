import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import CloudProviderSetupDialog from './CloudProviderSetupDialog';
import type { ProviderDetails } from '../../types/providers';

const mockLive = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpSaveProviderConfig: vi.fn(),
  acpListProviderLiveModels: (...a: unknown[]) => mockLive(...a),
  acpSaveProviderDefaultModel: vi.fn(),
  acpReadProviderConfig: vi.fn().mockResolvedValue([]),
  acpDeleteProviderConfig: vi.fn(),
}));
vi.mock('../openrouter/OpenRouterHostPicker', () => ({
  OpenRouterHostPicker: ({ model }: { model: string }) => (
    <div data-testid="host-picker" data-model={model} />
  ),
}));

function provider(name: string, display: string, defaultModel: string): ProviderDetails {
  return {
    name,
    is_configured: true,
    credentials_saved: true,
    default_model: defaultModel,
    provider_type: 'Native',
    metadata: {
      name,
      display_name: display,
      description: '',
      default_model: defaultModel,
      model_doc_link: '',
      config_keys: [
        {
          name: `${name.toUpperCase()}_API_KEY`,
          required: true,
          secret: true,
          default: null,
          oauth_flow: false,
          device_code_flow: false,
          primary: true,
        },
      ],
      known_models: [],
      setup_steps: [],
    },
  } as unknown as ProviderDetails;
}

const open = (p: ProviderDetails) =>
  render(
    <CloudProviderSetupDialog
      provider={p}
      onClose={vi.fn()}
      onSaved={vi.fn().mockResolvedValue(undefined)}
    />,
    { wrapper: IntlTestWrapper }
  );

beforeEach(() => vi.clearAllMocks());
afterEach(() => cleanup());

it('the OpenRouter default-model step carries the host picker for the model being chosen', async () => {
  mockLive.mockResolvedValue(['qwen/qwen3.8-27b', 'deepseek/deepseek-v4.1-flash']);
  open(provider('openrouter', 'OpenRouter', 'qwen/qwen3.8-27b'));
  const picker = await screen.findByTestId('host-picker');
  expect(picker).toHaveAttribute('data-model', 'qwen/qwen3.8-27b');

  await userEvent.click(await screen.findByText('deepseek/deepseek-v4.1-flash'));
  await waitFor(() =>
    expect(screen.getByTestId('host-picker')).toHaveAttribute(
      'data-model',
      'deepseek/deepseek-v4.1-flash'
    )
  );
});

it('no other provider gets an OpenRouter host picker', async () => {
  mockLive.mockResolvedValue(['gpt-5']);
  open(provider('openai', 'OpenAI', 'gpt-5'));
  await screen.findByText('gpt-5');
  expect(screen.queryByTestId('host-picker')).toBeNull();
});
