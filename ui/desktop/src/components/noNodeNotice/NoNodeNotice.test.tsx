import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxServingIntentRead } from '../../acp/mlx-serving-intent';
import { IntlProvider } from 'react-intl';
import { createUserMessage, type Message } from '../../types/message';
import { assertStudioClean } from '../lz/assertStudioClean';
import GooseMessage from '../GooseMessage';
import NoNodeNotice, { routedPeerName } from './NoNodeNotice';
import { resolveMountTarget, shortModelName } from './mlxMount';
import { parseNoNodeError } from './parseNoNodeError';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { FLASH_READY } from '../leanzero-swarm/mlxDistributed.fixtures';
import { DIED_MS, SPLIT_STOPPED_E2E2, WARN_MS } from '../chatServedBy/splitStop.fixtures';

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
const mockMount = vi.fn<(modelId: string, nodeId?: string) => Promise<void>>();
const mockSettings = vi.fn<() => Promise<MlxEngineSettings>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: (modelId: string, nodeId?: string) => mockMount(modelId, nodeId),
  mlxEngineSettingsRead: () => mockSettings(),
}));

const mockExtMethod = vi.fn();
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));

const mockRead = vi.fn();
const mockUpsert = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead, upsert: mockUpsert }),
}));
const mockIntent = vi.fn(
  async (): Promise<MlxServingIntentRead> => ({ intent: null, error: null })
);
vi.mock('../../acp/mlx-serving-intent', () => ({
  mlxServingIntent: () => mockIntent(),
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
const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
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
  mockUpsert.mockResolvedValue(undefined);
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

  it('names the peer when chat is routed to a LeanZero Link peer (the router’s own words)', () => {
    const rows = parseNoNodeError(
      "swarm chat: no node can serve this turn — mihai-mlx: this Mac's MLX chat is served from worksmacstudio-lan-9c1e2a (remote single, node remote-worksmacstudio-lan-9c1e2a) — stop it to use this Mac's own engine; remote-worksmacstudio-lan-9c1e2a: worksmacstudio-lan-9c1e2a's MLX engine is not serving through Link — v1/models answered 502 Bad Gateway: engineUnreachable: no MLX engine answers at http://127.0.0.1:8090 on WorksMacStudio.lan"
    );
    expect(rows?.map((r) => [r.nodeId, r.reason])).toEqual([
      ['mihai-mlx', { kind: 'mlx-routed-remote', peer: 'worksmacstudio-lan-9c1e2a' }],
      [
        'remote-worksmacstudio-lan-9c1e2a',
        { kind: 'mlx-remote-down', peer: 'worksmacstudio-lan-9c1e2a' },
      ],
    ]);
  });

  it('the router says the Mac’s one name — spaces and apostrophes included — and the row keeps it', () => {
    const rows = parseNoNodeError(
      "swarm chat: no node can serve this turn — mihai-mlx: this Mac's MLX chat is served from Work's Mac Studio (remote single, node remote-WorksMacStudio.lan) — stop it to use this Mac's own engine; remote-WorksMacStudio.lan: Work's Mac Studio's MLX engine is not serving through Link — v1/models answered 502 Bad Gateway: engineUnreachable"
    );
    expect(rows?.map((r) => [r.nodeId, r.reason])).toEqual([
      ['mihai-mlx', { kind: 'mlx-routed-remote', peer: "Work's Mac Studio" }],
      ['remote-WorksMacStudio.lan', { kind: 'mlx-remote-down', peer: "Work's Mac Studio" }],
    ]);
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

describe('routedPeerName — the router’s hostname, read as the Mac’s one name', () => {
  const ROUTE = {
    state: 'ready',
    peer: 'worksmacstudio-lan-9c1e2a',
    peerHostname: 'WorksMacStudio.lan',
    peerComputerName: "Work's Mac Studio",
  };
  it('the route this window knows names the Mac by its owner’s name', () => {
    expect(routedPeerName('WorksMacStudio.lan', ROUTE)).toBe("Work's Mac Studio");
    expect(routedPeerName('worksmacstudio-lan-9c1e2a', ROUTE)).toBe("Work's Mac Studio");
  });
  it('another Mac, or no route known, keeps the router’s word', () => {
    expect(routedPeerName('mini.lan', ROUTE)).toBe('mini.lan');
    expect(routedPeerName('WorksMacStudio.lan', null)).toBe('WorksMacStudio.lan');
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

  it('while the distributed engine owns the Mac the row names it and its state — never Mount', async () => {
    const user = userEvent.setup();
    const starting: MlxDistributedStatus = {
      ...FLASH_READY,
      state: 'starting',
      modelId: HF,
      servedModelId: ALIAS,
    };
    mockExtMethod.mockResolvedValue({ status: starting });
    await mlxDistributedStatus();
    const onRetry = vi.fn();
    wrap(<NoNodeNotice rows={rows} live retryText="hello" onRetry={onRetry} />);
    const cell = await screen.findByTestId('no-node-distributed-mihai-mlx');
    expect(cell.textContent).toBe('Distributed · 2 nodes · JACCL · Starting');
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    expect(screen.getByTestId('no-node-retry')).toBeDisabled();

    mockExtMethod.mockResolvedValue({ status: { ...starting, state: 'ready' } });
    await act(async () => {
      await mlxDistributedStatus();
    });
    expect(screen.getByTestId('no-node-distributed-mihai-mlx').textContent).toBe(
      'Distributed · 2 nodes · JACCL · Ready'
    );
    await user.click(screen.getByTestId('no-node-retry'));
    expect(onRetry).toHaveBeenCalledWith('hello');

    // Q-128 (12:1x): the split serving the node's model under its HF id is the node's model — no
    // "wants another model" line. Only a different model is.
    mockExtMethod.mockResolvedValue({ status: { ...starting, state: 'ready', servedModelId: HF } });
    await act(async () => {
      await mlxDistributedStatus();
    });
    expect(screen.queryByText(/this node wants/)).toBeNull();
    mockExtMethod.mockResolvedValue({
      status: { ...starting, state: 'ready', modelId: FLASH, servedModelId: FLASH },
    });
    await act(async () => {
      await mlxDistributedStatus();
    });
    expect(
      screen.getByText(`The distributed engine serves ${FLASH}; this node wants ${ALIAS}.`)
    ).toBeInTheDocument();

    mockExtMethod.mockRejectedValue(new Error('gone'));
    await act(async () => {
      await mlxDistributedStatus().catch(() => undefined);
    });
    expect(await screen.findByTestId('no-node-mount-mihai-mlx')).toBeInTheDocument();
  });

  /**
   * Q-128: the engine serves a model this node is not set to and nobody here started it (the
   * router's own words, e.g. a Link peer's Mount). One click points the node at what serves —
   * the swarm block is written through the one node-model writer — and Retry becomes the move.
   */
  it('a real mismatch offers "Chat with <served>" and one click writes the node to it', async () => {
    const user = userEvent.setup();
    const text = `Ran into this error: Execution error: swarm chat: no node can serve this turn — mihai-mlx: MLX engine serves '${FLASH}', the device wants '${ALIAS}' — it was not started from this Mac's goose, so chat does not follow it.`;
    const wrong = parseNoNodeError(text)!;
    expect(wrong.map((r) => r.reason)).toEqual([
      { kind: 'mlx-wrong-model', served: FLASH, wanted: ALIAS },
    ]);
    const other = { id: 'studio-lm', model_id: 'qwen', weight: 1, enabled: true };
    mockRead.mockResolvedValue({ endpoint: 'http://localhost:1234', devices: [DEVICE, other] });
    const onRetry = vi.fn();
    wrap(<NoNodeNotice rows={wrong} live retryText="hi" onRetry={onRetry} />);
    expect(screen.getByTestId('no-node-retry').getAttribute('data-variant')).toBe('secondary');
    const fix = screen.getByTestId('no-node-chat-with-mihai-mlx');
    expect(fix.textContent).toBe('Chat with Qwen3.8-Flash-Next-4bit');
    await user.click(fix);
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledTimes(1));
    expect(mockUpsert).toHaveBeenCalledWith(
      'swarm',
      {
        endpoint: 'http://localhost:1234',
        devices: [{ ...DEVICE, model_id: FLASH }, other],
      },
      false
    );
    expect(await screen.findByTestId('no-node-chat-with-done-mihai-mlx')).toHaveTextContent(
      'This node now chats with Qwen3.8-Flash-Next-4bit — retry to send your message.'
    );
    expect(screen.getByTestId('no-node-retry').getAttribute('data-variant')).toBe('primary');
    await user.click(screen.getByTestId('no-node-retry'));
    expect(onRetry).toHaveBeenCalledWith('hi');
  });

  it('the one-click fix names a failed write and writes nothing for a node gone from the pool', async () => {
    const user = userEvent.setup();
    const wrong = parseNoNodeError(
      `swarm chat: no node can serve this turn — mihai-mlx: MLX engine serves '${FLASH}', the device wants '${ALIAS}'`
    )!;
    mockRead.mockResolvedValue({ devices: [] });
    wrap(<NoNodeNotice rows={wrong} live retryText="hi" onRetry={vi.fn()} />);
    await user.click(screen.getByTestId('no-node-chat-with-mihai-mlx'));
    expect(await screen.findByTestId('no-node-chat-with-error-mihai-mlx')).toHaveTextContent(
      'mihai-mlx is no longer in the swarm pool'
    );
    expect(mockUpsert).not.toHaveBeenCalled();
  });

  it('a record of the refusal (not live) offers no fix', () => {
    const wrong = parseNoNodeError(
      `swarm chat: no node can serve this turn — mihai-mlx: MLX engine serves '${FLASH}', the device wants '${ALIAS}'`
    )!;
    wrap(<NoNodeNotice rows={wrong} live={false} retryText="hi" onRetry={vi.fn()} />);
    expect(screen.queryByTestId('no-node-chat-with-mihai-mlx')).toBeNull();
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

describe('the split chat was on stopped — the turn’s notice says so (Q-81, E2E #2)', () => {
  const at = (ms: number) => Math.floor(ms / 1000);
  function turn(text: string | string[], createdMs: number): Message {
    const parts = Array.isArray(text) ? text : [text];
    return {
      id: 'a1',
      role: 'assistant',
      created: at(createdMs),
      content: parts.map((t) => ({ type: 'text' as const, text: t })),
      metadata: { userVisible: true, agentVisible: true },
    };
  }
  function show(message: Message, append = vi.fn()) {
    const userTurn = createUserMessage('First thing Aoife will ask: how long can they stay on DC?');
    return wrap(
      <GooseMessage
        sessionId="s1"
        message={message}
        messages={[userTurn, message]}
        toolCallNotifications={new Map()}
        append={append}
        isStreaming={false}
      />
    );
  }

  afterEach(async () => {
    mockExtMethod.mockRejectedValue(new Error('reset'));
    await mlxDistributedStatus().catch(() => undefined);
  });

  it('a refusal after the stop: the split and why, Retry — the 8090 internals only behind Details', async () => {
    mockExtMethod.mockResolvedValue({ status: SPLIT_STOPPED_E2E2 });
    await act(async () => {
      await mlxDistributedStatus();
    });
    const user = userEvent.setup();
    const append = vi.fn();
    const { container } = show(turn(OWNER_TEXT, DIED_MS + 900), append);
    const notice = await screen.findByTestId('no-node-split-stopped');
    expect(notice.textContent).toContain('The split across your Macs stopped');
    expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
      'Work’s Mac Studio ran out of memory — nothing else could take this message.'
    );
    expect(notice.textContent).not.toContain('No model is mounted');
    expect(screen.queryByTestId('no-node-open-providers')).toBeNull();
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    for (const raw of screen.getAllByTestId('no-node-raw')) expect(raw).not.toBeVisible();
    await user.click(screen.getByRole('button', { name: /Details/ }));
    expect(screen.getAllByTestId('no-node-raw')[0].textContent).toContain(
      'mihai-mlx: MLX engine is not listening on http://127.0.0.1:8090'
    );
    expect(screen.getByTestId('no-node-split-raw').textContent).toContain(
      'rank 1 ended on its Link node'
    );
    await user.click(screen.getByTestId('no-node-retry'));
    expect(append).toHaveBeenCalledWith(
      'First thing Aoife will ask: how long can they stay on DC?'
    );
    assertStudioClean(container);
  });

  it('the answer the stop CUT: kept as written, the notice below it says the answer stops there', async () => {
    mockExtMethod.mockResolvedValue({ status: SPLIT_STOPPED_E2E2 });
    await act(async () => {
      await mlxDistributedStatus();
    });
    const answer = 'Atlassian announced the Data Center end of life for 28 March 2029. The';
    show(turn([answer, OWNER_TEXT], WARN_MS - 90_000));
    expect(screen.getByText(/end of life for 28 March 2029\. The$/)).toBeInTheDocument();
    const notice = await screen.findByTestId('no-node-split-stopped');
    expect(notice.textContent).toContain('The split across your Macs stopped mid-answer');
    expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
      'Work’s Mac Studio ran out of memory — the answer above stops there.'
    );
    expect(screen.getByTestId('no-node-retry')).toBeInTheDocument();
    expect(screen.queryByText(/Please retry if you think/)).toBeNull();
  });

  it('a cut answer with no split behind it keeps the answer and the plain notice below', () => {
    const answer = 'Half an answer';
    show(turn([answer, OWNER_TEXT], DIED_MS));
    expect(screen.getByText('Half an answer')).toBeInTheDocument();
    expect(screen.getByTestId('no-node-notice').textContent).toContain('No model is mounted');
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
  });

  it('a refusal from BEFORE the split ever served is not blamed on it', async () => {
    mockExtMethod.mockResolvedValue({ status: SPLIT_STOPPED_E2E2 });
    await act(async () => {
      await mlxDistributedStatus();
    });
    show(turn(OWNER_TEXT, WARN_MS - 30 * 60_000));
    expect(await screen.findByTestId('no-node-notice')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
  });
});
