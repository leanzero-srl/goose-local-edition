import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/react';
import { ProviderConnectionStartup } from './ProviderConnectionStartup';
import { IntlTestWrapper } from '../i18n/test-utils';
import { getAcpClient } from '../acp/acpConnection';
import { toastError } from '../toasts';

vi.mock('../contexts/EditionContext', () => ({
  useEdition: () => ({ edition: 'local', isLocal: true, setEdition: vi.fn() }),
}));
vi.mock('../acp/acpConnection', () => ({ getAcpClient: vi.fn() }));
vi.mock('../toasts', () => ({ toastError: vi.fn() }));

const entry = (providerId: string) => ({
  providerId,
  providerName: providerId,
  configured: true,
  configKeys: [],
  models: [],
  defaultModel: 'm',
});

/** The engine's answer to the launch check, as `providers/config/status` gives it: OpenAI points at
 *  goose's MLX engine on 127.0.0.1:8090 with no model loaded, Anthropic's key was revoked. */
function engineAnswers() {
  const client = {
    goose: {
      providersConfigStatus_unstable: vi.fn().mockResolvedValue({
        statuses: [
          {
            providerId: 'openai',
            isConfigured: true,
            connectionChecked: true,
            connectionError: null,
            localServerNotRunning: 'http://127.0.0.1:8090',
          },
          {
            providerId: 'anthropic',
            isConfigured: true,
            connectionChecked: true,
            connectionError:
              'Anthropic could not list models: Authentication error: invalid x-api-key',
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

describe('ProviderConnectionStartup', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    engineAnswers();
  });
  afterEach(cleanup);

  it('alarms for a broken cloud key, never for a local server that is not started', async () => {
    render(
      <IntlTestWrapper>
        <ProviderConnectionStartup />
      </IntlTestWrapper>
    );
    await waitFor(() => expect(toastError).toHaveBeenCalledTimes(1));
    const [{ title, msg }] = vi.mocked(toastError).mock.calls[0];
    expect(title).toBe('Provider connections need attention');
    expect(msg).toContain('invalid x-api-key');
    expect(msg).not.toContain('OpenAI');
    expect(msg).not.toContain('8090');
  });
});
