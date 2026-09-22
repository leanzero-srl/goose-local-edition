import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import OnboardingGuard from './OnboardingGuard';
import { IntlTestWrapper } from '../../i18n/test-utils';

vi.mock('../ConfigContext', () => ({ useConfig: () => ({ upsert: vi.fn() }) }));
vi.mock('../ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    getFallbackModelAndProvider: vi.fn().mockResolvedValue({ provider: null, model: null }),
    refreshCurrentModelAndProvider: vi.fn().mockResolvedValue(undefined),
  }),
}));
vi.mock('../../utils/analytics', () => ({
  trackOnboardingStarted: vi.fn(),
  trackOnboardingCompleted: vi.fn(),
  trackOnboardingProviderSelected: vi.fn(),
  trackTelemetryPreference: vi.fn(),
  setTelemetryEnabled: vi.fn(),
}));
vi.mock('./ProviderSelector', () => ({
  default: ({ onConfigured }: { onConfigured: (p: string, m?: string) => void }) => (
    <button data-testid="onboarding-use-swarm" onClick={() => onConfigured('swarm', 'swarm')}>
      Use Goose Swarm
    </button>
  ),
}));
vi.mock('./OnboardingSuccess', () => ({
  default: ({ providerName }: { providerName: string }) => (
    <div data-testid="onboarding-success">{providerName}</div>
  ),
}));

const mockSaveDefaults = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpReadDefaults: vi.fn().mockResolvedValue({ providerId: null, modelId: null }),
  acpListProviderDetails: vi.fn().mockResolvedValue([
    {
      name: 'swarm',
      is_configured: true,
      provider_type: 'Preferred',
      metadata: {
        name: 'swarm',
        display_name: 'Goose Swarm',
        default_model: 'swarm',
        config_keys: [],
        known_models: [],
        description: '',
        model_doc_link: '',
      },
    },
  ]),
  acpSaveDefaults: (...a: unknown[]) => mockSaveDefaults(...a),
}));

const render = () =>
  rtlRender(
    <MemoryRouter>
      <OnboardingGuard>
        <div data-testid="app" />
      </OnboardingGuard>
    </MemoryRouter>,
    { wrapper: IntlTestWrapper }
  );

beforeEach(() => vi.clearAllMocks());
afterEach(() => cleanup());

describe('OnboardingGuard', () => {
  it('a refused defaults save is SHOWN with the engine’s words — never a silent stay on the same screen', async () => {
    mockSaveDefaults.mockRejectedValue(
      Object.assign(new Error('Invalid params'), { data: 'Provider is not configured: swarm' })
    );
    render();
    await userEvent.click(await screen.findByTestId('onboarding-use-swarm'));
    await waitFor(() => {
      expect(screen.getByTestId('onboarding-configure-error')).toHaveTextContent(
        'Could not set up swarm: Provider is not configured: swarm'
      );
    });
    expect(screen.queryByTestId('onboarding-success')).not.toBeInTheDocument();
  });

  it('an accepted defaults save moves on to the success screen', async () => {
    mockSaveDefaults.mockResolvedValue(undefined);
    render();
    await userEvent.click(await screen.findByTestId('onboarding-use-swarm'));
    await waitFor(() => {
      expect(screen.getByTestId('onboarding-success')).toHaveTextContent('Goose Swarm');
    });
    expect(mockSaveDefaults).toHaveBeenCalledWith('swarm', 'swarm');
  });
});
