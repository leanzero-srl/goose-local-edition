import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import { IntlProvider } from 'react-intl';
import { createUserMessage, type Message } from '../../types/message';
import { assertStudioClean } from '../lz/assertStudioClean';
import GooseMessage from '../GooseMessage';
import NoNodeNotice from './NoNodeNotice';
import { resolveMountTarget, shortModelName } from './mlxMount';
import { parseNoNodeError } from './parseNoNodeError';

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
const mockMount = vi.fn<(modelId: string, nodeId?: string) => Promise<void>>();
const mockSettings = vi.fn<() => Promise<MlxEngineSettings>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: (modelId: string, nodeId?: string) => mockMount(modelId, nodeId),
  mlxEngineSettingsRead: () => mockSettings(),
}));

const mockRead = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead }),
}));

/** The owner's screenshot, 2026-09-23, verbatim as the agent loop wraps it. */
const OWNER_TEXT =
  "Ran into this error: Execution error: swarm chat: no node can serve this turn — mihai-mlx: MLX engine is not listening on http://127.0.0.1:8090 — mount it in the MLX window (this process's manager: stopped; error sending request for url (http://127.0.0.1:8090/v1/models)).\n\nPlease retry if you think this is a transient or recoverable error.";

const HF = 'mlx-community/Qwen3.6-35B-A3B-4bit';
const ALIAS = 'mihai-qwen3.6-35b-a3b-4bit-mlx';
const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const DEVICE = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 40,
  totalMemoryGb: 64,
};

function wrap(ui: React.ReactElement) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter initialEntries={['/pair']}>
        <Routes>
          <Route path="/pair" element={ui} />
          <Route path="/leanzero-swarm" element={<div data-testid="providers-view" />} />
        </Routes>
      </MemoryRouter>
    </IntlProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mockStatus.mockResolvedValue(STOPPED);
  mockMount.mockResolvedValue(undefined);
  mockSettings.mockResolvedValue(SETTINGS);
  mockRead.mockResolvedValue({ devices: [DEVICE] });
});

describe('parseNoNodeError', () => {
  it('parses the owner’s refusal: one row, the parenthesised `; ` kept inside the reason', () => {
    const rows = parseNoNodeError(OWNER_TEXT);
    expect(rows).toEqual([
      {
        nodeId: 'mihai-mlx',
        raw: "MLX engine is not listening on http://127.0.0.1:8090 — mount it in the MLX window (this process's manager: stopped; error sending request for url (http://127.0.0.1:8090/v1/models))",
        reason: { kind: 'mlx-down', base: 'http://127.0.0.1:8090' },
      },
    ]);
  });

  it('splits several nodes and classifies each; an unknown reason stays verbatim as other', () => {
    const rows = parseNoNodeError(
      "swarm chat: no node can serve this turn — a-lm: http://10.0.0.2:1234/v1/models unreachable (connect refused); b-lm: model 'qwen' is not listed by http://x/v1/models; c-mlx: MLX engine serves 'x', the device wants 'y'; d: refused admission this turn; e-cloud: creating the 'bedrock' provider: no key"
    );
    expect(rows?.map((r) => [r.nodeId, r.reason.kind])).toEqual([
      ['a-lm', 'lm-unreachable'],
      ['b-lm', 'lm-not-listed'],
      ['c-mlx', 'mlx-wrong-model'],
      ['d', 'busy'],
      ['e-cloud', 'other'],
    ]);
    expect(rows?.[4].raw).toBe("creating the 'bedrock' provider: no key");
  });

  it('parses the pool-level reason (no devices) with no node id', () => {
    expect(
      parseNoNodeError(
        'swarm chat: no node can serve this turn — no enabled device is configured under `swarm.devices`'
      )
    ).toEqual([
      {
        nodeId: null,
        raw: 'no enabled device is configured under `swarm.devices`',
        reason: { kind: 'no-devices' },
      },
    ]);
  });

  it('is null for any other text', () => {
    expect(parseNoNodeError('Ran into this error: rate limited.')).toBeNull();
  });
});

describe('resolveMountTarget', () => {
  it('mounts the saved HF model when it serves the alias the device names', () => {
    expect(resolveMountTarget('mihai-mlx', [DEVICE], SETTINGS)).toEqual({
      kind: 'ok',
      modelId: HF,
      servedId: ALIAS,
    });
  });
  it('states a mismatch instead of mounting a model the node would still refuse', () => {
    expect(
      resolveMountTarget('mihai-mlx', [DEVICE], { ...SETTINGS, servedModelName: 'other-alias' })
    ).toEqual({ kind: 'mismatch', served: 'other-alias', wanted: ALIAS });
  });
  it('offers nothing for a node that is not a local sidecar or has no saved model', () => {
    expect(resolveMountTarget('ghost', [DEVICE], SETTINGS)).toEqual({ kind: 'none' });
    expect(resolveMountTarget('mihai-mlx', [{ ...DEVICE, host: 'studio' }], SETTINGS)).toEqual({
      kind: 'none',
    });
    expect(resolveMountTarget('mihai-mlx', [DEVICE], { ...SETTINGS, modelId: undefined })).toEqual({
      kind: 'none',
    });
  });
});

describe('shortModelName', () => {
  it('reads an HF repo id by its last path segment and leaves a bare id alone', () => {
    expect(shortModelName('Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx')).toBe(
      'Qwen3.8-27B-Atlassian-Q8-mlx'
    );
    expect(shortModelName('qwen3.6-27b')).toBe('qwen3.6-27b');
    expect(shortModelName('org/model/')).toBe('model');
  });
});

describe('NoNodeNotice', () => {
  const rows = parseNoNodeError(OWNER_TEXT)!;

  it('renders the headline, the node row in plain words AND verbatim, with no design-ban class', async () => {
    const { container } = wrap(
      <NoNodeNotice rows={rows} live retryText="hello" onRetry={vi.fn()} />
    );
    expect(screen.getByText('No model is mounted')).toBeInTheDocument();
    const row = screen.getByTestId('no-node-row-mihai-mlx');
    expect(
      within(row).getByText(
        'The MLX engine is not running — nothing answers at http://127.0.0.1:8090.'
      )
    ).toBeInTheDocument();
    expect(within(row).getByTestId('no-node-raw').textContent).toBe(rows[0].raw);
    await screen.findByTestId('no-node-mount-mihai-mlx');
    assertStudioClean(container);
  });

  it('Mount calls mlxEngineMount with the configured model on this machine, then shows mounting', async () => {
    const user = userEvent.setup();
    wrap(<NoNodeNotice rows={rows} live retryText="hello" onRetry={vi.fn()} />);
    const mount = await screen.findByTestId('no-node-mount-mihai-mlx');
    // A1: the button names the model by its last path segment; the full HF id is the tooltip.
    expect(mount.textContent).toBe('Mount Qwen3.6-35B-A3B-4bit');
    expect(mount.getAttribute('title')).toBe(HF);
    mockStatus.mockResolvedValue({ ...STOPPED, state: 'mounting', modelId: HF });
    const pollsBefore = mockStatus.mock.calls.length;
    await user.click(mount);
    expect(mockMount).toHaveBeenCalledWith(HF, undefined);
    // Between the call and the next poll the stale "stopped" must not bring Mount back.
    expect(screen.getByText('Mounting')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    await waitFor(() => expect(mockStatus.mock.calls.length).toBeGreaterThan(pollsBefore), {
      timeout: 4000,
    });
    // Now the engine's own "mounting" drives the chip.
    expect(await screen.findByText('Mounting')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
  });

  it('a mount refusal renders the engine’s words verbatim and keeps Mount available', async () => {
    const user = userEvent.setup();
    mockMount.mockRejectedValue(new Error('not enough memory: 18 GB free, 22 GB needed'));
    wrap(<NoNodeNotice rows={rows} live retryText="hello" onRetry={vi.fn()} />);
    await user.click(await screen.findByTestId('no-node-mount-mihai-mlx'));
    expect((await screen.findByTestId('no-node-mount-error-mihai-mlx')).textContent).toContain(
      'not enough memory: 18 GB free, 22 GB needed'
    );
    expect(screen.getByTestId('no-node-mount-mihai-mlx')).toBeInTheDocument();
  });

  it('once the engine serves the node’s model the row says Mounted and Retry resends the turn', async () => {
    const user = userEvent.setup();
    mockStatus.mockResolvedValue({
      ...STOPPED,
      state: 'running',
      modelId: HF,
      servedModelId: ALIAS,
    });
    const onRetry = vi.fn();
    wrap(<NoNodeNotice rows={rows} live retryText="tell me about this skill" onRetry={onRetry} />);
    expect(await screen.findByText('Mounted')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    const retry = screen.getByTestId('no-node-retry');
    expect(retry.getAttribute('data-variant')).toBe('primary');
    await user.click(retry);
    expect(onRetry).toHaveBeenCalledWith('tell me about this skill');
  });

  it('a config read failure is stated, not hidden', async () => {
    mockRead.mockRejectedValue(new Error('config.yaml unreadable'));
    wrap(<NoNodeNotice rows={rows} live retryText="hello" onRetry={vi.fn()} />);
    expect(
      await screen.findByText('Could not read which model this node mounts: config.yaml unreadable')
    ).toBeInTheDocument();
  });

  it('an older notice in the history is a record: no poll, no Mount, no Retry', async () => {
    wrap(<NoNodeNotice rows={rows} live={false} retryText="hello" onRetry={vi.fn()} />);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(mockStatus).not.toHaveBeenCalled();
    expect(mockSettings).not.toHaveBeenCalled();
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    expect(screen.queryByTestId('no-node-retry')).toBeNull();
    expect(screen.getByTestId('no-node-raw').textContent).toBe(rows[0].raw);
  });

  it('Open Providers navigates to the Providers view', async () => {
    const user = userEvent.setup();
    wrap(<NoNodeNotice rows={rows} live={false} retryText={null} onRetry={vi.fn()} />);
    await user.click(screen.getByTestId('no-node-open-providers'));
    expect(screen.getByTestId('providers-view')).toBeInTheDocument();
  });

  it('an unknown reason renders verbatim under the generic headline', () => {
    const other = parseNoNodeError(
      "swarm chat: no node can serve this turn — e-cloud: creating the 'bedrock' provider: no key"
    )!;
    wrap(<NoNodeNotice rows={other} live retryText={null} onRetry={vi.fn()} />);
    expect(screen.getByText('No node can answer')).toBeInTheDocument();
    expect(screen.getByText("creating the 'bedrock' provider: no key")).toBeInTheDocument();
  });
});

describe('GooseMessage renders the refusal as the notice', () => {
  function assistant(text: string): Message {
    return {
      id: 'a1',
      role: 'assistant',
      created: 2,
      content: [{ type: 'text', text }],
      metadata: { userVisible: true, agentVisible: true },
    };
  }

  it('replaces the raw text and Retry resends the last user turn through append', async () => {
    const user = userEvent.setup();
    const append = vi.fn();
    const userTurn = createUserMessage('Start an AI session about the memory "fleet"');
    const refusal = assistant(OWNER_TEXT);
    wrap(
      <GooseMessage
        sessionId="s1"
        message={refusal}
        messages={[userTurn, refusal]}
        toolCallNotifications={new Map()}
        append={append}
        isStreaming={false}
      />
    );
    expect(screen.getByTestId('no-node-notice')).toBeInTheDocument();
    expect(screen.queryByText(/Please retry if you think/)).toBeNull();
    await user.click(screen.getByTestId('no-node-retry'));
    expect(append).toHaveBeenCalledWith('Start an AI session about the memory "fleet"');
    await waitFor(() => expect(mockStatus).toHaveBeenCalled());
  });

  it('offers no Retry when the last user turn carried an image it could not resend', () => {
    const userTurn = createUserMessage('look at this', [{ data: 'AAAA', mimeType: 'image/png' }]);
    const refusal = assistant(OWNER_TEXT);
    wrap(
      <GooseMessage
        sessionId="s1"
        message={refusal}
        messages={[userTurn, refusal]}
        toolCallNotifications={new Map()}
        append={vi.fn()}
        isStreaming={false}
      />
    );
    expect(screen.getByTestId('no-node-notice')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-retry')).toBeNull();
  });
});
