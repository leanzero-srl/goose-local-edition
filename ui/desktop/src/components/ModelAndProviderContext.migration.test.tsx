import React from 'react';
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
const setSessionProviderModel = vi.fn();
vi.mock('../acp/providers', () => ({
  acpReadDefaults: () => readDefaults(),
  acpSaveDefaults: (...a: unknown[]) => saveDefaults(...a),
  acpSetSessionProviderModel: (...a: unknown[]) => setSessionProviderModel(...a),
}));
const setSessionMetadata = vi.fn();
const storeSessions: Record<string, { provider_name: string; model_config: { model_name: string } }> =
  {};
vi.mock('../acp/chatSessionStore', () => ({
  acpChatSessionActions: { setSessionMetadata: (...a: unknown[]) => setSessionMetadata(...a) },
  acpChatSessionStore: {
    getSnapshot: (id: string) => (storeSessions[id] ? { session: storeSessions[id] } : undefined),
  },
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

/** Stands in for BaseChat: the session's provider arrives with its metadata and the effect fires. */
function SessionProbe({ sessionId, provider }: { sessionId: string; provider: string | null }) {
  const { migrateLegacySessionProvider } = useModelAndProvider();
  React.useEffect(() => {
    if (!provider) return;
    void migrateLegacySessionProvider(sessionId, provider);
  }, [sessionId, provider, migrateLegacySessionProvider]);
  return null;
}

const renderSession = (sessionId: string, provider: string | null) =>
  render(
    <ModelAndProviderProvider>
      <SessionProbe sessionId={sessionId} provider={provider} />
    </ModelAndProviderProvider>,
    { wrapper: IntlTestWrapper }
  );

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
  setSessionProviderModel.mockResolvedValue({ providerId: 'swarm', modelId: 'swarm' });
  for (const k of Object.keys(storeSessions)) delete storeSessions[k];
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

describe('legacy SESSION provider migration (a resumed omlx/lmstudio session -> swarm/swarm)', () => {
  const settle = () => new Promise((r) => setTimeout(r, 0));

  beforeEach(() => {
    readDefaults.mockResolvedValue({ providerId: 'swarm', modelId: 'swarm' });
  });

  it('local edition: a session resumed on omlx is written to swarm/swarm through the session write, the store is patched, and the line is logged', async () => {
    storeSessions['20260831_2'] = {
      provider_name: 'omlx',
      model_config: { model_name: 'qwen3-30b-served' },
    };
    renderSession('20260831_2', 'omlx');
    await waitFor(() => expect(setSessionProviderModel).toHaveBeenCalledTimes(1));
    expect(setSessionProviderModel).toHaveBeenCalledWith('20260831_2', 'swarm', 'swarm', null);
    await waitFor(() =>
      expect(infoSpy).toHaveBeenCalledWith(
        '[provider-migration] session 20260831_2: omlx/qwen3-30b-served → swarm/swarm'
      )
    );
    expect(setSessionMetadata).toHaveBeenCalledWith(
      '20260831_2',
      expect.objectContaining({
        provider_name: 'swarm',
        model_config: expect.objectContaining({ model_name: 'swarm' }),
      })
    );
    // the default-provider half did not fire: the default was already swarm
    expect(saveDefaults).not.toHaveBeenCalled();
  });

  it('local edition: a session on lmstudio moves too', async () => {
    renderSession('s-lms', 'lmstudio');
    await waitFor(() => expect(setSessionProviderModel).toHaveBeenCalledTimes(1));
    expect(setSessionProviderModel).toHaveBeenCalledWith('s-lms', 'swarm', 'swarm', null);
  });

  it('a swarm session is untouched', async () => {
    renderSession('s-swarm', 'swarm');
    await settle();
    expect(setSessionProviderModel).not.toHaveBeenCalled();
    expect(setSessionMetadata).not.toHaveBeenCalled();
    expect(infoSpy).not.toHaveBeenCalled();
  });

  it('a session whose provider has not loaded yet is left alone (no guess from a null provider)', async () => {
    renderSession('s-pending', null);
    await settle();
    expect(setSessionProviderModel).not.toHaveBeenCalled();
  });

  it('standard edition: an omlx session is untouched', async () => {
    mockEdition = 'standard';
    renderSession('s-std', 'omlx');
    await settle();
    expect(setSessionProviderModel).not.toHaveBeenCalled();
    expect(infoSpy).not.toHaveBeenCalled();
  });

  it('once per session per launch: resuming the same session again does not re-apply', async () => {
    const first = renderSession('s-twice', 'omlx');
    await waitFor(() => expect(setSessionProviderModel).toHaveBeenCalledTimes(1));
    first.unmount();
    renderSession('s-twice', 'omlx');
    await settle();
    expect(setSessionProviderModel).toHaveBeenCalledTimes(1);
    // a DIFFERENT legacy session in the same launch still moves
    renderSession('s-other', 'omlx');
    await waitFor(() => expect(setSessionProviderModel).toHaveBeenCalledTimes(2));
    expect(setSessionProviderModel).toHaveBeenLastCalledWith('s-other', 'swarm', 'swarm', null);
  });

  it('a failed session write is reported with console.error and the store keeps the old provider', async () => {
    setSessionProviderModel.mockRejectedValue(new Error('agent gone'));
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    renderSession('s-fail', 'omlx');
    await waitFor(() =>
      expect(
        errSpy.mock.calls.some((c) => String(c[0]).startsWith('[provider-migration] session s-fail'))
      ).toBe(true)
    );
    expect(setSessionMetadata).not.toHaveBeenCalled();
    expect(infoSpy).not.toHaveBeenCalled();
    errSpy.mockRestore();
  });
});
