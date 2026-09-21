import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient } from '../acpConnection';
import {
  acpSetSessionProviderModel,
  acpListProviderDetails,
  acpRecheckProviderConnections,
} from '../providers';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

function selectConfigOption(id: string, currentValue: string) {
  return {
    id,
    name: id,
    type: 'select',
    currentValue,
    options: [],
  };
}

describe('ACP providers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('sets thinking effort after provider and model, then returns the final config response', async () => {
    const client = {
      setSessionConfigOption: vi
        .fn()
        .mockResolvedValueOnce({
          configOptions: [
            selectConfigOption('provider', 'anthropic'),
            selectConfigOption('model', 'provider-default-model'),
          ],
        })
        .mockResolvedValueOnce({
          configOptions: [
            selectConfigOption('provider', 'anthropic'),
            selectConfigOption('model', 'claude-sonnet-4-5'),
          ],
        })
        .mockResolvedValueOnce({
          configOptions: [
            selectConfigOption('provider', 'anthropic'),
            selectConfigOption('model', 'claude-sonnet-4-5'),
            selectConfigOption('thinking_effort', 'high'),
          ],
        }),
    };
    vi.mocked(getAcpClient).mockResolvedValue(
      client as unknown as Awaited<ReturnType<typeof getAcpClient>>
    );

    const applied = await acpSetSessionProviderModel(
      'session-1',
      'anthropic',
      'claude-sonnet-4-5',
      'high'
    );

    expect(client.setSessionConfigOption).toHaveBeenCalledTimes(3);
    expect(client.setSessionConfigOption).toHaveBeenNthCalledWith(1, {
      sessionId: 'session-1',
      configId: 'provider',
      value: 'anthropic',
    });
    expect(client.setSessionConfigOption).toHaveBeenNthCalledWith(2, {
      sessionId: 'session-1',
      configId: 'model',
      value: 'claude-sonnet-4-5',
    });
    expect(client.setSessionConfigOption).toHaveBeenNthCalledWith(3, {
      sessionId: 'session-1',
      configId: 'thinking_effort',
      value: 'high',
    });
    expect(applied).toEqual({
      providerId: 'anthropic',
      modelId: 'claude-sonnet-4-5',
    });
  });
});

describe('saved provider connection checks', () => {
  it('checks once per backend connection and hides failed or unchecked providers', async () => {
    const entries = ['google', 'openai', 'anthropic'].map((providerId) => ({
      providerId,
      providerName: providerId,
      configured: true,
      configKeys: [],
      models: [],
      defaultModel: 'test-model',
    }));
    const status = vi.fn().mockResolvedValue({
      statuses: [
        {
          providerId: 'google',
          isConfigured: true,
          connectionChecked: true,
          connectionError: null,
        },
        {
          providerId: 'openai',
          isConfigured: true,
          connectionChecked: true,
          connectionError: 'Expired API key',
        },
      ],
    });
    const client = {
      goose: {
        providersConfigStatus_unstable: status,
        providersList_unstable: vi.fn().mockResolvedValue({ entries }),
      },
    };
    vi.mocked(getAcpClient).mockResolvedValue(
      client as unknown as Awaited<ReturnType<typeof getAcpClient>>
    );
    const first = await acpListProviderDetails();
    expect(first.map((provider) => provider.is_configured)).toEqual([true, false, false]);
    expect(first[1].connection_error).toBe('Expired API key');
    await acpListProviderDetails();
    expect(status.mock.calls.filter(([request]) => request.checkConnections)).toHaveLength(1);
    await acpRecheckProviderConnections();
    expect(status.mock.calls.filter(([request]) => request.checkConnections)).toHaveLength(2);
    const newClient = { goose: { ...client.goose } };
    vi.mocked(getAcpClient).mockResolvedValue(
      newClient as unknown as Awaited<ReturnType<typeof getAcpClient>>
    );
    await acpListProviderDetails();
    expect(status.mock.calls.filter(([request]) => request.checkConnections)).toHaveLength(3);
  });
});
