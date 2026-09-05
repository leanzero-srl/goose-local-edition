import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import ProviderSettings from './ProviderSettingsPage';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import type { ProviderDetails } from '../../../types/providers';

let mockIsLocal = false;
vi.mock('../../../contexts/EditionContext', () => ({
  useEdition: () => ({
    edition: mockIsLocal ? 'local' : 'standard',
    isLocal: mockIsLocal,
    setEdition: vi.fn(),
  }),
}));

const gridSpy = vi.fn();
vi.mock('./ProviderGrid', () => ({
  default: (props: { providers: ProviderDetails[]; allowCustomProvider?: boolean }) => {
    gridSpy(props);
    return (
      <div data-testid="provider-grid">
        {props.providers.map((p) => (
          <span key={p.name} data-testid={`grid-provider-${p.name}`} />
        ))}
      </div>
    );
  },
}));

const mockList = vi.fn();
vi.mock('../../../acp/providers', () => ({
  acpListProviderDetails: (...a: unknown[]) => mockList(...a),
}));

const p = (name: string) =>
  ({
    name,
    is_configured: true,
    provider_type: 'Builtin',
    metadata: { name, display_name: name, config_keys: [], known_models: [] },
  }) as unknown as ProviderDetails;

const REGISTRY = ['anthropic', 'openai', 'ollama', 'omlx', 'lmstudio', 'swarm', 'google', 'zai', 'aws_bedrock', 'custom_deepseek'].map(p);

const render = () =>
  rtlRender(
    <MemoryRouter>
      <ProviderSettings onClose={vi.fn()} isOnboarding={false} />
    </MemoryRouter>,
    { wrapper: IntlTestWrapper }
  );

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockResolvedValue(REGISTRY);
});
afterEach(() => cleanup());

describe('ProviderSettingsPage (/configure-providers)', () => {
  it('local edition: the grid is the allow-list (swarm + four cloud families) with no custom card', async () => {
    mockIsLocal = true;
    render();
    await waitFor(() => expect(screen.getByTestId('provider-grid')).toBeInTheDocument());
    const shown = (gridSpy.mock.lastCall?.[0].providers as ProviderDetails[]).map((x) => x.name);
    expect(shown.sort()).toEqual(['aws_bedrock', 'custom_deepseek', 'google', 'swarm', 'zai']);
    expect(gridSpy).toHaveBeenLastCalledWith(expect.objectContaining({ allowCustomProvider: false }));
  });

  it('standard edition: every provider, custom card allowed', async () => {
    mockIsLocal = false;
    render();
    await waitFor(() => expect(screen.getByTestId('provider-grid')).toBeInTheDocument());
    expect(gridSpy.mock.lastCall?.[0].providers).toHaveLength(REGISTRY.length);
    expect(gridSpy).toHaveBeenLastCalledWith(expect.objectContaining({ allowCustomProvider: true }));
  });
});
