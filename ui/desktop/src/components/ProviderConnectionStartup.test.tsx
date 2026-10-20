import type { ReactElement } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { ProviderConnectionStartup } from './ProviderConnectionStartup';
import { IntlTestWrapper } from '../i18n/test-utils';
import { getAcpClient } from '../acp/acpConnection';

const toast = vi.hoisted(() => ({ error: vi.fn() }));
vi.mock('react-toastify', () => ({ toast }));
vi.mock('../contexts/EditionContext', () => ({
  useEdition: () => ({ edition: 'local', isLocal: true, setEdition: vi.fn() }),
}));
vi.mock('../acp/acpConnection', () => ({ getAcpClient: vi.fn() }));

const ANTHROPIC_REFUSED =
  'Anthropic could not list models: Authentication error: Authentication failed for v1/models. Status: 401 Unauthorized. Response: invalid x-api-key';
/** What the engine answered for OpenAI at 127.0.0.1:8090 with no model loaded, before and after. */
const OLD_ENGINE = {
  connectionError:
    'OpenAI could not list models: Network error: Could not connect to 127.0.0.1:8090 — check your network connection and try again.',
};
const NEW_ENGINE = { connectionError: null, localServerNotRunning: 'http://127.0.0.1:8090' };

const entry = (providerId: string) => ({
  providerId,
  providerName: providerId,
  configured: true,
  configKeys: [],
  models: [],
  defaultModel: 'm',
});

function engineAnswers(openai: Record<string, unknown>) {
  const client = {
    goose: {
      providersConfigStatus_unstable: vi.fn().mockResolvedValue({
        statuses: [
          { providerId: 'openai', isConfigured: true, connectionChecked: true, ...openai },
          {
            providerId: 'anthropic',
            isConfigured: true,
            connectionChecked: true,
            connectionError: ANTHROPIC_REFUSED,
          },
        ],
      }),
      providersList_unstable: vi
        .fn()
        .mockResolvedValue({ entries: [entry('openai'), entry('anthropic')] }),
    },
  };
  vi.mocked(getAcpClient).mockResolvedValue(
    client as unknown as Awaited<ReturnType<typeof getAcpClient>>
  );
}

/** The launch toast as it renders: one line per provider. */
async function launchToastLines(): Promise<string[]> {
  render(
    <IntlTestWrapper>
      <ProviderConnectionStartup />
    </IntlTestWrapper>
  );
  await waitFor(() => expect(toast.error).toHaveBeenCalledTimes(1));
  cleanup();
  render(toast.error.mock.calls[0][0] as ReactElement);
  expect(screen.getByText('Provider connections need attention')).toBeInTheDocument();
  return screen.getAllByTestId('toast-line').map((line) => line.textContent ?? '');
}

describe('the launch toast', () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(cleanup);

  it('names only the broken cloud key, on its own line, when the local engine is not started', async () => {
    engineAnswers(NEW_ENGINE);
    expect(await launchToastLines()).toEqual([`Claude: ${ANTHROPIC_REFUSED}`]);
  });

  it('the old engine answer is what put OpenAI in the toast — two providers, two lines', async () => {
    engineAnswers(OLD_ENGINE);
    expect(await launchToastLines()).toEqual([
      `OpenAI: ${OLD_ENGINE.connectionError}`,
      `Claude: ${ANTHROPIC_REFUSED}`,
    ]);
  });
});
