import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import {
  ModelAndProviderProvider,
  useModelAndProvider,
  __resetLegacyProviderMigrationForTests,
} from './ModelAndProviderContext';
import { IntlTestWrapper } from '../i18n/test-utils';

let mockEdition: 'local' | 'standard' = 'local';
vi.mock('../contexts/EditionContext', () => ({
  useEdition: () => ({
    edition: mockEdition,
    isLocal: mockEdition === 'local',
    setEdition: vi.fn(),
  }),
}));

const readDefaults = vi.fn();
const saveDefaults = vi.fn();
vi.mock('../acp/providers', () => ({
  acpReadDefaults: () => readDefaults(),
  acpSaveDefaults: (...a: unknown[]) => saveDefaults(...a),
  acpSetSessionProviderModel: vi.fn(),
}));
vi.mock('../acp/chatSessionStore', () => ({
  acpChatSessionActions: { setSessionMetadata: vi.fn() },
  acpChatSessionStore: { getSnapshot: () => null },
}));
vi.mock('../toasts', () => ({ toastError: vi.fn(), toastSuccess: vi.fn() }));
vi.mock('./settings/models/modelInterface', () => ({
  default: {},
  getProviderMetadata: vi.fn().mockResolvedValue({ display_name: 'x' }),
}));

function Probe() {
  const { currentProvider, currentModel } = useModelAndProvider();
  return (
    <span data-testid="pm">
      {currentProvider ?? '-'}/{currentModel ?? '-'}
    </span>
  );
}

const renderProvider = () =>
  render(
    <ModelAndProviderProvider>
      <Probe />
    </ModelAndProviderProvider>,
    { wrapper: IntlTestWrapper }
  );

let infoSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  vi.clearAllMocks();
  __resetLegacyProviderMigrationForTests();
  mockEdition = 'local';
  saveDefaults.mockResolvedValue(undefined);
  (window as unknown as { appConfig: unknown }).appConfig = { get: () => undefined };
  infoSpy = vi.spyOn(console, 'info').mockImplementation(() => {});
});
afterEach(() => {
  infoSpy.mockRestore();
  cleanup();
});

describe('legacy active-provider migration (omlx/lmstudio -> Goose Swarm) at app start', () => {
  it('local edition + active omlx: writes swarm/swarm once, updates the context, and logs before/after', async () => {
    readDefaults.mockResolvedValue({ providerId: 'omlx', modelId: 'qwen3-30b-served' });
    renderProvider();
    await waitFor(() => expect(screen.getByTestId('pm')).toHaveTextContent('swarm/swarm'));
    expect(saveDefaults).toHaveBeenCalledTimes(1);
    expect(saveDefaults).toHaveBeenCalledWith('swarm', 'swarm');
    expect(infoSpy).toHaveBeenCalledWith(
      '[provider-migration] active provider moved to Goose Swarm',
      expect.objectContaining({
        before: { provider: 'omlx', model: 'qwen3-30b-served' },
        after: { provider: 'swarm', model: 'swarm' },
        edition: 'local',
      })
    );
  });

  it('local edition + active lmstudio migrates too', async () => {
    readDefaults.mockResolvedValue({ providerId: 'lmstudio', modelId: 'some-gguf' });
    renderProvider();
    await waitFor(() => expect(screen.getByTestId('pm')).toHaveTextContent('swarm/swarm'));
    expect(saveDefaults).toHaveBeenCalledWith('swarm', 'swarm');
  });

  it('runs ONCE per launch: a second provider mount seeing omlx again does not write', async () => {
    readDefaults.mockResolvedValue({ providerId: 'omlx', modelId: 'm' });
    const first = renderProvider();
    await waitFor(() => expect(saveDefaults).toHaveBeenCalledTimes(1));
    first.unmount();
    renderProvider();
    await waitFor(() => expect(screen.getByTestId('pm')).toHaveTextContent('omlx/m'));
    expect(saveDefaults).toHaveBeenCalledTimes(1);
  });

  it('standard edition: an omlx default is left alone', async () => {
    mockEdition = 'standard';
    readDefaults.mockResolvedValue({ providerId: 'omlx', modelId: 'm' });
    renderProvider();
    await waitFor(() => expect(screen.getByTestId('pm')).toHaveTextContent('omlx/m'));
    expect(saveDefaults).not.toHaveBeenCalled();
    expect(infoSpy).not.toHaveBeenCalled();
  });

  it('local edition with an allowed provider (google, swarm): nothing moves', async () => {
    readDefaults.mockResolvedValue({ providerId: 'google', modelId: 'gemini-2.5-pro' });
    renderProvider();
    await waitFor(() =>
      expect(screen.getByTestId('pm')).toHaveTextContent('google/gemini-2.5-pro')
    );
    expect(saveDefaults).not.toHaveBeenCalled();
  });

  it('a failed defaults write is reported, and the context keeps the truthful old provider', async () => {
    readDefaults.mockResolvedValue({ providerId: 'omlx', modelId: 'm' });
    saveDefaults.mockRejectedValue(new Error('config locked'));
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    renderProvider();
    await waitFor(() => expect(errSpy).toHaveBeenCalled());
    expect(screen.getByTestId('pm')).toHaveTextContent('omlx/m');
    expect(errSpy.mock.calls.some((c) => String(c[0]).includes('[provider-migration]'))).toBe(true);
    errSpy.mockRestore();
  });
});
