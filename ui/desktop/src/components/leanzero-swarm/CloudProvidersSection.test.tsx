import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import CloudProvidersSection from './CloudProvidersSection';
import type { ProviderDetails } from '../../types/providers';

// ProviderGrid (cards + ProviderConfigurationModal + custom-provider form) is REUSED, not forked —
// its own behavior is covered where it lives. Here the contract is the FILTER and the failure twin.
const gridSpy = vi.fn();
vi.mock('../settings/providers/ProviderGrid', () => ({
  default: (props: { providers: ProviderDetails[]; allowCustomProvider?: boolean }) => {
    gridSpy(props);
    return (
      <div data-testid="provider-grid">
        {props.providers.map((p) => (
          <span key={p.name} data-testid={`grid-provider-${p.name}`}>
            {p.name}
          </span>
        ))}
      </div>
    );
  },
}));

const mockList = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: (...a: unknown[]) => mockList(...a),
}));

function provider(name: string, configured: boolean): ProviderDetails {
  return {
    name,
    is_configured: configured,
    provider_type: 'Native',
    metadata: {
      name,
      display_name: name,
      description: '',
      default_model: '',
      model_doc_link: '',
      model_selection_hint: null,
      config_keys: [],
      known_models: [],
      setup_steps: [],
    },
  } as unknown as ProviderDetails;
}

const render = () =>
  rtlRender(
    <MemoryRouter>
      <CloudProvidersSection />
    </MemoryRouter>,
    { wrapper: IntlTestWrapper }
  );

beforeEach(() => {
  vi.clearAllMocks();
});
afterEach(() => cleanup());

describe('CloudProvidersSection', () => {
  it('shows all registry cloud providers and custom setup, excluding local engines', async () => {
    mockList.mockResolvedValue([
      provider('anthropic', true),
      provider('openai', false),
      provider('lmstudio', true),
      provider('ollama', false),
      provider('omlx', true),
      provider('swarm', true),
      provider('aws_bedrock', true),
      provider('zai', false),
      provider('google', true),
      provider('custom_deepseek', false),
    ]);
    render();
    await waitFor(() => {
      expect(screen.getAllByTestId('provider-grid')[0]).toBeInTheDocument();
    });
    for (const allowed of [
      'anthropic',
      'openai',
      'aws_bedrock',
      'zai',
      'google',
      'custom_deepseek',
    ]) {
      expect(screen.getByTestId(`grid-provider-${allowed}`)).toBeInTheDocument();
    }
    for (const hidden of ['lmstudio', 'ollama', 'omlx', 'swarm']) {
      expect(screen.queryByTestId(`grid-provider-${hidden}`)).not.toBeInTheDocument();
    }
    expect(gridSpy).toHaveBeenLastCalledWith(
      expect.objectContaining({ allowCustomProvider: false })
    );
    expect(gridSpy.mock.lastCall?.[0].providers).toHaveLength(3);
    expect(screen.getByRole('region', { name: 'Configured' })).toHaveTextContent('anthropic');
    expect(screen.getByRole('region', { name: 'Configured' })).not.toHaveTextContent('openai');
    expect(screen.getByRole('region', { name: 'Available to set up' })).toHaveTextContent('openai');
    // The count describes the visible registry rows.
    expect(screen.getByText('3 of 6 configured')).toBeInTheDocument();
  });

  it('a failed provider list renders the failure twin with a working Retry — never a clean empty grid', async () => {
    mockList.mockRejectedValueOnce(new Error('agent unreachable'));
    mockList.mockResolvedValueOnce([provider('google', true)]);
    render();
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('agent unreachable');
    });
    expect(screen.queryByTestId('provider-grid')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => {
      expect(screen.getByTestId('grid-provider-google')).toBeInTheDocument();
    });
  });
});
