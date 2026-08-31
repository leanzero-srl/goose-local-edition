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
  default: (props: { providers: ProviderDetails[] }) => {
    gridSpy(props.providers);
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
  it('lists ONLY cloud providers — every local/swarm provider is filtered out', async () => {
    mockList.mockResolvedValue([
      provider('anthropic', true),
      provider('openai', false),
      provider('lmstudio', true),
      provider('ollama', false),
      provider('omlx', true),
      provider('swarm', true),
    ]);
    render();
    await waitFor(() => {
      expect(screen.getByTestId('provider-grid')).toBeInTheDocument();
    });
    expect(screen.getByTestId('grid-provider-anthropic')).toBeInTheDocument();
    expect(screen.getByTestId('grid-provider-openai')).toBeInTheDocument();
    for (const local of ['lmstudio', 'ollama', 'omlx', 'swarm']) {
      expect(screen.queryByTestId(`grid-provider-${local}`)).not.toBeInTheDocument();
    }
    // the count chip counts what the grid shows: 1 of the 2 CLOUD providers is configured
    expect(screen.getByText('1 of 2 configured')).toBeInTheDocument();
  });

  it('a failed provider list renders the failure twin with a working Retry — never a clean empty grid', async () => {
    mockList.mockRejectedValueOnce(new Error('agent unreachable'));
    mockList.mockResolvedValueOnce([provider('anthropic', true)]);
    render();
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('agent unreachable');
    });
    expect(screen.queryByTestId('provider-grid')).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => {
      expect(screen.getByTestId('grid-provider-anthropic')).toBeInTheDocument();
    });
  });
});
