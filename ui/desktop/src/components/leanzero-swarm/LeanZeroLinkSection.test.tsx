import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, cleanup, render as rtlRender, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import LeanZeroLinkSection from './LeanZeroLinkSection';
import type { LinkHealth, LinkState, NodesResponse } from '../../acp/leanzero-link';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

// Stub the seven network fns; keep the REAL error helpers (linkBannerText/linkErrorText),
// which the component relies on to render backend text verbatim.
const mockStatus = vi.fn();
const mockRequestCode = vi.fn();
const mockVerify = vi.fn();
const mockConnect = vi.fn();
const mockLogout = vi.fn();
const mockDisconnect = vi.fn();
const mockNodes = vi.fn();
const mockHealth = vi.fn();

vi.mock('../../acp/leanzero-link', async (importActual) => {
  const actual = await importActual<typeof import('../../acp/leanzero-link')>();
  return {
    ...actual,
    leanzeroLinkStatus: (...a: unknown[]) => mockStatus(...a),
    leanzeroLinkRequestCode: (...a: unknown[]) => mockRequestCode(...a),
    leanzeroLinkVerify: (...a: unknown[]) => mockVerify(...a),
    leanzeroLinkConnect: (...a: unknown[]) => mockConnect(...a),
    leanzeroLinkLogout: (...a: unknown[]) => mockLogout(...a),
    leanzeroLinkDisconnect: (...a: unknown[]) => mockDisconnect(...a),
    leanzeroLinkNodes: (...a: unknown[]) => mockNodes(...a),
    leanzeroLinkHealth: (...a: unknown[]) => mockHealth(...a),
  };
});

// My Macs reads every Mac's engine and models folder, and writes the owner's switches.
const mockEngineStatus = vi.fn();
const mockModelsList = vi.fn();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: (...a: unknown[]) => mockEngineStatus(...a),
  mlxEngineModelsList: (...a: unknown[]) => mockModelsList(...a),
  mlxEngineDownload: vi.fn(),
  mlxEngineDownloadCancel: vi.fn(),
  mlxEngineDownloadPause: vi.fn(),
  mlxEngineDownloadProgress: vi.fn(async () => null),
  mlxEngineDownloadResume: vi.fn(),
  mlxEngineModelDelete: vi.fn(),
}));
vi.mock('../../acp/mlx-replica', () => ({
  mlxEngineReplicaTargets: vi.fn(async () => ({ meshConnected: true, targets: [] })),
  mlxEngineReplicate: vi.fn(),
  mlxEngineReplicaProgress: vi.fn(async () => null),
  mlxEngineReplicaCancel: vi.fn(),
}));
const mockUpsert = vi.fn();
vi.mock('../../acp/config', () => ({
  acpUpsertConfig: (...a: unknown[]) => mockUpsert(...a),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ leanzeroLink: true, mlxDistributed: false, mlxEngine: true }),
}));

const GIB = 1024 * 1024 * 1024;

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const render = () => rtlRender(<LeanZeroLinkSection />, { wrapper: IntlTestWrapper });

/** A Mac's card on My Macs, keyed by its key (`self`, or the peer's node id). */
const macCard = (key: string) => screen.findByTestId(`my-mac-${key}`);

/** The Details fold of a card holds Disconnect / Log out; open it first. */
async function openDetails(card: HTMLElement) {
  const fold = within(card).getByTestId('my-mac-details');
  await userEvent.click(within(fold).getByRole('button', { name: /details/i }));
}

/** A RequestError carries the backend sentence in `.data` (SDK `.message` is generic). */
function rpcError(data: string): Error {
  return Object.assign(new Error('Invalid params'), { data });
}

const HEALTHY: LinkHealth = {
  ok: true,
  version: '1.0.0',
  capabilities: { mail: true, audience: true, mesh: true },
};

const LOGGED_OUT: LinkState = { auth: { state: 'loggedOut' }, nodeCount: 0 };

function codeSent(secondsAhead = 300): LinkState {
  return {
    auth: {
      state: 'codeSent',
      email: 'user@example.com',
      expiresAt: new Date(Date.now() + secondsAhead * 1000).toISOString(),
    },
    nodeCount: 0,
  };
}

const LOGGED_IN: LinkState = {
  auth: { state: 'loggedIn', email: 'user@example.com' },
  nodeCount: 0,
};

const CONNECTED: LinkState = {
  auth: { state: 'connected', email: 'user@example.com', meshIp: '100.64.0.1' },
  mesh: {
    selfIp: '100.64.0.1',
    selfHostname: 'works-mac-studio',
    backendState: 'Running',
    online: true,
    peers: [],
  },
  nodeCount: 2,
};

const NODES_WITH_PEERS: NodesResponse = {
  self: {
    node_id: 'works-mac-studio-ab12cd',
    hostname: 'works-mac-studio',
    mesh_ip: '100.64.0.1',
    status: { type: 'Busy', session_id: 'sess-abc12345' },
    sessions_active: 1,
    updated_at: new Date().toISOString(),
  },
  peers: [
    {
      node_id: 'mihai-macbook-2-ff99aa',
      hostname: 'mihai-macbook-2',
      mesh_ip: '100.64.0.2',
      status: { type: 'Idle' },
      sessions_active: 0,
      updated_at: new Date(Date.now() - 30_000).toISOString(),
    },
    {
      node_id: 'studio-b-771122',
      hostname: 'studio-b',
      mesh_ip: '100.64.0.3',
      status: { type: 'Offline' },
      sessions_active: 0,
      updated_at: new Date(Date.now() - 3_600_000).toISOString(),
    },
  ],
};

/** A roster for the run-card: an idle self, plus idle / busy / offline peers. */
const RUN_NODES: NodesResponse = {
  self: {
    node_id: 'self-aa',
    hostname: 'works-mac-studio',
    mesh_ip: '100.64.0.1',
    status: { type: 'Idle' },
    sessions_active: 0,
    updated_at: new Date().toISOString(),
  },
  peers: [
    {
      node_id: 'peer-idle',
      hostname: 'mihai-macbook-2',
      mesh_ip: '100.64.0.2',
      status: { type: 'Idle' },
      sessions_active: 0,
      updated_at: new Date().toISOString(),
    },
    {
      node_id: 'peer-busy',
      hostname: 'studio-b',
      mesh_ip: '100.64.0.3',
      status: { type: 'Busy', session_id: 'sess-xyz9' },
      sessions_active: 1,
      updated_at: new Date().toISOString(),
    },
    {
      node_id: 'peer-offline',
      hostname: 'studio-c',
      mesh_ip: '100.64.0.4',
      status: { type: 'Offline' },
      sessions_active: 0,
      updated_at: new Date().toISOString(),
    },
  ],
};

/** Drive the status poll from a single mutable state so optimistic + poll agree. */
let currentState: LinkState;

beforeEach(() => {
  currentState = LOGGED_OUT;
  mockStatus.mockImplementation(async () => currentState);
  mockHealth.mockResolvedValue(HEALTHY);
  mockNodes.mockResolvedValue({ self: NODES_WITH_PEERS.self, peers: [] });
  mockEngineStatus.mockResolvedValue({
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 89.7,
    totalMemoryGb: 128,
    chip: { hwModel: 'Mac16,5', brand: 'Apple M4 Max', gpuCores: 40 },
  });
  mockModelsList.mockResolvedValue({
    models: [{ id: 'm/one', sizeBytes: 31 * GIB, complete: true, missingFiles: 0 }],
    diskAvailableBytes: 149 * GIB,
    diskTotalBytes: 926 * GIB,
  });
  mockUpsert.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('LeanZeroLinkSection — each AuthState renders its card', () => {
  it('loggedOut renders the sign-in login card with the tagline', async () => {
    currentState = LOGGED_OUT;
    render();
    expect(await screen.findByTestId('link-login-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-email-input')).toBeInTheDocument();
    expect(screen.getByText(/no password, just a code by email/i)).toBeInTheDocument();
  });

  it('codeSent renders the code entry with a masked email and a countdown', async () => {
    currentState = codeSent(300);
    render();
    expect(await screen.findByTestId('link-code-input')).toBeInTheDocument();
    expect(screen.getByTestId('link-masked-email')).toHaveTextContent('u***@example.com');
    expect(screen.getByTestId('link-countdown').textContent).toMatch(/^[45]:\d\d$/);
  });

  it('loggedIn renders the Connect-to-mesh card', async () => {
    currentState = LOGGED_IN;
    render();
    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-connect')).toHaveTextContent('Connect to mesh');
  });

  it('connecting renders the benchmark connecting state', async () => {
    currentState = { auth: { state: 'connecting', email: 'user@example.com' }, nodeCount: 0 };
    render();
    expect(await screen.findByTestId('link-connecting')).toBeInTheDocument();
    expect(screen.getByText(/joining your private mesh/i)).toBeInTheDocument();
  });

  it('connected renders the dashboard with the account email', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    render();
    expect(await screen.findByTestId('link-connected')).toBeInTheDocument();
    const self = await macCard('self');
    await openDetails(self);
    expect(within(self).getByText('user@example.com')).toBeInTheDocument();
    expect(within(self).getByText(/Running · online · 2 Macs/)).toBeInTheDocument();
  });
});

describe('LeanZeroLinkSection — login flow', () => {
  it('requestCode success advances to code entry with a 5:00 countdown', async () => {
    currentState = LOGGED_OUT;
    render();
    await screen.findByTestId('link-login-card');

    mockRequestCode.mockResolvedValue({ email: 'user@example.com', expiresInSeconds: 300 });
    currentState = codeSent(300);

    await userEvent.type(screen.getByTestId('link-email-input'), 'user@example.com');
    await userEvent.click(screen.getByTestId('link-send-code'));

    expect(mockRequestCode).toHaveBeenCalledWith('user@example.com');
    expect(await screen.findByTestId('link-code-input')).toBeInTheDocument();
    expect(screen.getByTestId('link-countdown').textContent).toMatch(/^[45]:\d\d$/);
  });

  it('verify success moves to loggedIn (the Connect card)', async () => {
    currentState = codeSent(300);
    render();
    await screen.findByTestId('link-code-input');

    mockVerify.mockResolvedValue({
      state: 'loggedIn',
      email: 'user@example.com',
      audienceSync: 'synced',
    });
    currentState = LOGGED_IN;

    await userEvent.type(screen.getByTestId('link-code-input'), '123456');
    await userEvent.click(screen.getByTestId('link-verify'));

    expect(mockVerify).toHaveBeenCalledWith('user@example.com', '123456');
    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
  });

  it('audienceSync "failed" shows a small amber note, not a blocker', async () => {
    currentState = codeSent(300);
    render();
    await screen.findByTestId('link-code-input');

    mockVerify.mockResolvedValue({
      state: 'loggedIn',
      email: 'user@example.com',
      audienceSync: 'failed',
    });
    currentState = LOGGED_IN;

    await userEvent.type(screen.getByTestId('link-code-input'), '654321');
    await userEvent.click(screen.getByTestId('link-verify'));

    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-audience-note')).toBeInTheDocument();
  });
});

describe('LeanZeroLinkSection — connect lifecycle', () => {
  it('connect goes connecting → connected via the resolved state and loads peers', async () => {
    currentState = LOGGED_IN;
    render();
    await screen.findByTestId('link-connect-card');

    let resolveConnect: (v: LinkState) => void = () => {};
    mockConnect.mockReturnValue(
      new Promise<LinkState>((resolve) => {
        resolveConnect = resolve;
      })
    );

    await userEvent.click(screen.getByTestId('link-connect'));
    // Optimistic connecting card while the mesh is coming up.
    expect(await screen.findByTestId('link-connecting')).toBeInTheDocument();

    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    resolveConnect(CONNECTED);

    expect(await screen.findByTestId('link-connected')).toBeInTheDocument();
    expect(await macCard('mihai-macbook-2-ff99aa')).toBeInTheDocument();
  });

  it('connect failure renders lastError in a solid banner and stays on the Connect card', async () => {
    currentState = LOGGED_IN;
    render();
    await screen.findByTestId('link-connect-card');

    mockConnect.mockRejectedValue(
      rpcError('mesh joined but reported no IP — cannot compose a Connected state')
    );
    // The status poll after failure reconciles back to loggedIn.
    currentState = LOGGED_IN;

    await userEvent.click(screen.getByTestId('link-connect'));

    expect(await screen.findByText(/mesh joined but reported no IP/i)).toBeInTheDocument();
    expect(screen.getByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-connect')).toHaveTextContent('Retry connect');
  });
});

describe('LeanZeroLinkSection — My Macs', () => {
  it('one card per Mac under the ONE name it reports, its state in the palette and its facts', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({
      self: { ...NODES_WITH_PEERS.self, computer_name: 'Mihai Macbook' },
      peers: [
        { ...NODES_WITH_PEERS.peers[0], computer_name: 'Work’s Mac Studio' },
        NODES_WITH_PEERS.peers[1],
      ],
    });
    render();

    const self = await macCard('self');
    expect(within(self).getByTestId('my-mac-name')).toHaveTextContent('Mihai Macbook');
    await waitFor(() =>
      expect(within(self).getByTestId('my-mac-state')).toHaveAttribute('data-phase', 'unloaded')
    );
    expect(within(self).getByTestId('my-mac-state')).toHaveTextContent('Not loaded');
    expect(within(self).getByTestId('my-mac-line')).toHaveTextContent('No model loaded');
    expect(within(self).getByText('89.7 GB free of 128 GB')).toBeInTheDocument();
    expect(within(self).getByText('149 GB free of 926 GB')).toBeInTheDocument();
    expect(within(self).getByText('Apple M4 Max · 40-core GPU')).toBeInTheDocument();
    expect(within(self).getByTestId('my-mac-models-self')).toHaveTextContent('1');

    // The peer is called by its ComputerName, never by its hostname or a link: id.
    const peer = await macCard('mihai-macbook-2-ff99aa');
    expect(within(peer).getByTestId('my-mac-name')).toHaveTextContent('Work’s Mac Studio');

    // An older goose reports no name: its hostname is all there is. An offline Mac says so.
    const offline = await macCard('studio-b-771122');
    expect(within(offline).getByTestId('my-mac-name')).toHaveTextContent('studio-b');
    expect(within(offline).getByTestId('my-mac-state')).toHaveTextContent('Offline');
  });

  it('a running Mac says what it runs and how fast, in the writing green', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({ self: NODES_WITH_PEERS.self, peers: [] });
    mockEngineStatus.mockResolvedValue({
      state: 'running',
      modelId: 'Mihai-LeanZero/Qwen3.8-27B',
      baseUrl: 'http://127.0.0.1:8095',
      restartRequired: false,
      availableMemoryGb: 60,
      totalMemoryGb: 128,
    });
    const bridge = window.electron as unknown as Record<string, unknown>;
    bridge.mlxLiveStatus = async () => ({
        ok: true,
        body: {
          status: 'running',
          uptime_s: 12,
          num_running: 1,
          num_waiting: 0,
          requests: [
            {
              request_id: 'r1',
              phase: 'generation',
              status: 'running',
              completion_tokens: 40,
              tokens_per_second: 21.9,
            },
          ],
        },
    });
    render();
    const self = await macCard('self');
    await waitFor(() =>
      expect(within(self).getByTestId('my-mac-line')).toHaveTextContent('Qwen3.8-27B · 21.9 tok/s')
    );
    expect(within(self).getByTestId('my-mac-state')).toHaveAttribute('data-phase', 'writing');
    delete bridge.mlxLiveStatus;
  });

  it('a peer with model management off says where to turn it on — never a raw 403, never "0 models"', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({
      self: NODES_WITH_PEERS.self,
      peers: [
        {
          ...NODES_WITH_PEERS.peers[0],
          computer_name: 'Work’s Mac Studio',
          allows: { manage_models: false, answer_chat: true, run_split: false },
        },
      ],
    });
    render();
    const peer = await macCard('mihai-macbook-2-ff99aa');
    expect(within(peer).getByTestId('my-mac-off')).toHaveTextContent(
      'Load and download models is off on Work’s Mac Studio — turn on “Let my other Macs use this Mac” there'
    );
    expect(within(peer).getByTestId('my-mac-state')).toHaveTextContent('Off');
    expect(peer).not.toHaveTextContent(/403/);
    expect(within(peer).queryByText(/models/i, { selector: 'dt' })).toBeNull();
    // The peer was never asked — its own roster entry said why.
    expect(mockEngineStatus).not.toHaveBeenCalledWith('mihai-macbook-2-ff99aa');
    expect(mockModelsList).not.toHaveBeenCalledWith('mihai-macbook-2-ff99aa');
    const lets = within(peer).getByTestId('my-mac-lets-mihai-macbook-2-ff99aa');
    expect(lets).toHaveTextContent('Load and download models · off');
    expect(lets).toHaveTextContent('Answer chat · on');
  });

  it('a peer whose goose predates the switches and answers 403 still reads in words, red', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({
      self: NODES_WITH_PEERS.self,
      peers: [{ ...NODES_WITH_PEERS.peers[0], computer_name: 'Work’s Mac Studio' }],
    });
    mockModelsList.mockImplementation(async (nodeId?: string) => {
      if (nodeId) {
        throw Object.assign(new Error('Internal error'), {
          data: 'mlx proxy request to a peer failed: peer returned 403: remote model management is disabled on this node',
        });
      }
      return { models: [], diskAvailableBytes: 1, diskTotalBytes: 2 };
    });
    render();
    const peer = await macCard('mihai-macbook-2-ff99aa');
    await waitFor(() =>
      expect(peer).toHaveTextContent(
        'Can’t read: Load and download models is off on Work’s Mac Studio'
      )
    );
    expect(peer).not.toHaveTextContent('peer returned 403');
  });

  it('this Mac: one master switch and three parts, each writing its own config key', async () => {
    currentState = { ...CONNECTED, remoteExecutionAllowed: false, chatServingAllowed: false };
    render();
    const self = await macCard('self');
    const master = within(self).getByRole('switch', { name: 'Let my other Macs use this Mac' });
    expect(master).toHaveAttribute('aria-checked', 'false');
    // Off: the three parts wait for the master.
    expect(within(self).getByTestId('my-mac-permission-chat')).toBeDisabled();

    // goose persists what was written; the next status read says so.
    mockUpsert.mockImplementation(async () => {
      currentState = {
        ...CONNECTED,
        remoteExecutionAllowed: true,
        remoteExecutionAllowedLive: true,
        chatServingAllowed: true,
        distributedNodeAllowed: true,
      };
    });
    await userEvent.click(master);
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledTimes(3));
    expect(mockUpsert).toHaveBeenCalledWith('LEANZERO_LINK_ALLOW_REMOTE_EXECUTION', true);
    expect(mockUpsert).toHaveBeenCalledWith('LEANZERO_LINK_ALLOW_CHAT_SERVING', true);
    expect(mockUpsert).toHaveBeenCalledWith('LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE', true);

    mockUpsert.mockClear();
    await waitFor(() =>
      expect(within(self).getByTestId('my-mac-permission-chat')).not.toBeDisabled()
    );
    await userEvent.click(within(self).getByTestId('my-mac-permission-chat'));
    await waitFor(() =>
      expect(mockUpsert).toHaveBeenCalledWith('LEANZERO_LINK_ALLOW_CHAT_SERVING', false)
    );
    expect(mockUpsert).toHaveBeenCalledTimes(1);
  });

  it('a model-management change waiting on a reconnect says so, with the reconnect one click away', async () => {
    currentState = {
      ...CONNECTED,
      remoteExecutionAllowed: true,
      remoteExecutionAllowedLive: false,
    };
    mockDisconnect.mockResolvedValue(LOGGED_IN);
    mockConnect.mockResolvedValue(CONNECTED);
    render();
    const self = await macCard('self');
    const note = await within(self).findByTestId('my-mac-apply-on-reconnect');
    expect(note).toHaveTextContent('takes effect when this Mac reconnects');
    await userEvent.click(within(note).getByRole('button', { name: 'Reconnect now' }));
    await waitFor(() => expect(mockConnect).toHaveBeenCalledTimes(1));
    expect(mockDisconnect).toHaveBeenCalledTimes(1);
  });

  it('with no other Mac, it says how to add one', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({ self: NODES_WITH_PEERS.self, peers: [] });
    render();
    expect(await screen.findByTestId('link-peers-empty')).toHaveTextContent(
      /No other Mac is on your LeanZero Link account yet/i
    );
  });

  it('logout confirms via a custom dialog and passes wipe:true when the box is checked', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    mockLogout.mockResolvedValue(LOGGED_OUT);
    render();
    await screen.findByTestId('link-connected');
    await openDetails(await macCard('self'));

    await userEvent.click(screen.getByTestId('link-logout'));
    // Custom dialog, not window.confirm.
    const checkbox = await screen.findByTestId('link-wipe-checkbox');
    expect(checkbox).toHaveAttribute('aria-checked', 'false');
    await userEvent.click(checkbox);
    expect(checkbox).toHaveAttribute('aria-checked', 'true');

    currentState = LOGGED_OUT;
    await userEvent.click(screen.getByRole('button', { name: 'Log out' }));

    expect(mockLogout).toHaveBeenCalledWith(true);
    expect(await screen.findByTestId('link-login-card')).toBeInTheDocument();
  });

  it('logout defaults to wipe:false when the box is left unchecked', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    mockLogout.mockResolvedValue(LOGGED_OUT);
    render();
    await screen.findByTestId('link-connected');
    await openDetails(await macCard('self'));

    await userEvent.click(screen.getByTestId('link-logout'));
    await screen.findByTestId('link-wipe-checkbox');
    currentState = LOGGED_OUT;
    await userEvent.click(screen.getByRole('button', { name: 'Log out' }));

    expect(mockLogout).toHaveBeenCalledWith(false);
  });
});

describe('LeanZeroLinkSection — the persisted intent and the launch reconnect', () => {
  const CONNECTED_INTENT = {
    intent: 'connected' as const,
    cause: 'userConnect' as const,
    updatedAt: '2026-09-24T10:00:00Z',
  };

  it('a failed launch reconnect is named with its reason and a Retry — not a bare "not connected"', async () => {
    currentState = {
      ...LOGGED_IN,
      intent: CONNECTED_INTENT,
      lastError: 'mesh join failed: control plane unreachable',
      reconnect: {
        state: 'failed',
        reason: 'mesh join failed: control plane unreachable',
        at: '2026-09-24T10:00:05Z',
      },
    };
    render();
    const banner = await screen.findByTestId('link-reconnect-failed');
    expect(banner).toHaveTextContent(
      /did not come back: mesh join failed: control plane unreachable/
    );
    expect(screen.getByTestId('link-mesh-state')).toHaveTextContent('reconnect failed');
    // The same text is not shown twice as a second "Connect failed" banner.
    expect(screen.queryByText('Connect failed')).not.toBeInTheDocument();

    mockConnect.mockResolvedValue(CONNECTED);
    await userEvent.click(screen.getByTestId('link-connect'));
    expect(mockConnect).toHaveBeenCalledTimes(1);
  });

  it('a reconnect that found no credential says so above the sign-in card', async () => {
    currentState = {
      ...LOGGED_OUT,
      reconnect: { state: 'failed', reason: 'not signed in: sign in again', at: 'x' },
    };
    render();
    expect(await screen.findByTestId('link-reconnect-failed')).toHaveTextContent(
      'not signed in: sign in again'
    );
    expect(screen.getByTestId('link-login-card')).toBeInTheDocument();
  });

  it('the reconnect in flight reads as Reconnecting', async () => {
    currentState = {
      auth: { state: 'connecting', email: 'user@example.com' },
      nodeCount: 0,
      reconnect: { state: 'reconnecting', startedAt: 'x' },
    };
    render();
    expect(
      await screen.findByText(/bringing this mac back onto your private mesh/i)
    ).toBeInTheDocument();
  });

  it('Disconnect keeps the account signed in and the card says the Mac stays off', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    const off: LinkState = {
      ...LOGGED_IN,
      intent: { intent: 'disconnected', cause: 'userDisconnect', updatedAt: 'x' },
      reconnect: { state: 'idle' },
    };
    mockDisconnect.mockResolvedValue(off);
    render();
    await screen.findByTestId('link-connected');
    await openDetails(await macCard('self'));

    currentState = off;
    await userEvent.click(screen.getByTestId('link-disconnect'));
    expect(mockDisconnect).toHaveBeenCalledTimes(1);
    expect(mockLogout).not.toHaveBeenCalled();
    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-mesh-state')).toHaveTextContent(
      'disconnected · stays off until you connect'
    );
    expect(screen.getByText(/stays off the mesh — across restarts too/)).toBeInTheDocument();
  });

  it("a reconnect left to another window's backend says so; a fresh sign-in gets the plain card", async () => {
    currentState = {
      ...LOGGED_IN,
      intent: CONNECTED_INTENT,
      reconnect: { state: 'skipped', reason: 'another goose on this Mac already holds the mesh' },
    };
    render();
    expect(await screen.findByTestId('link-reconnect-skipped')).toHaveTextContent(
      'another goose on this Mac already holds the mesh'
    );
    cleanup();

    currentState = {
      ...LOGGED_IN,
      intent: { intent: 'disconnected', cause: 'noRecord', updatedAt: 'x' },
      reconnect: {
        state: 'skipped',
        reason: 'this Mac has never been connected under this sign-in',
      },
    };
    render();
    expect(await screen.findByTestId('link-mesh-state')).toHaveTextContent('not connected');
    expect(screen.queryByTestId('link-reconnect-skipped')).not.toBeInTheDocument();
    expect(screen.getByText(/reconnects by itself whenever the app starts/)).toBeInTheDocument();
  });

  it('an unreadable intent is shown as itself', async () => {
    currentState = { ...LOGGED_IN, intentError: 'the Link intent file is malformed' };
    render();
    expect(await screen.findByTestId('link-intent-error')).toHaveTextContent(
      'the Link intent file is malformed'
    );
  });
});

describe('LeanZeroLinkSection — health + error surfacing', () => {
  it('health with mesh=false shows the deployment banner', async () => {
    currentState = LOGGED_OUT;
    mockHealth.mockResolvedValue({
      ok: true,
      version: '1.0.0',
      capabilities: { mail: true, audience: true, mesh: false },
    });
    render();
    expect(await screen.findByTestId('link-deploy-banner')).toHaveTextContent(
      'This LeanZero Link deployment has no mesh configured.'
    );
  });

  it('health with mail=false shows the honest half-configured banner', async () => {
    currentState = LOGGED_OUT;
    mockHealth.mockResolvedValue({
      ok: true,
      version: '1.0.0',
      capabilities: { mail: false, audience: true, mesh: true },
    });
    render();
    expect(await screen.findByTestId('link-deploy-banner')).toHaveTextContent(
      'This LeanZero Link deployment has no email sign-in configured.'
    );
  });

  it('a rate-limit error renders the worker retry wording VERBATIM', async () => {
    currentState = LOGGED_OUT;
    render();
    await screen.findByTestId('link-login-card');

    const verbatim =
      'rate limited on request-code; retry after 42s (worker said: too many requests)';
    mockRequestCode.mockRejectedValue(rpcError(verbatim));

    await userEvent.type(screen.getByTestId('link-email-input'), 'user@example.com');
    await userEvent.click(screen.getByTestId('link-send-code'));

    expect(await screen.findByText(verbatim)).toBeInTheDocument();
  });

  it('an unreachable worker renders the honest "couldn\'t reach the service" line', async () => {
    currentState = LOGGED_OUT;
    render();
    await screen.findByTestId('link-login-card');

    mockRequestCode.mockRejectedValue(
      rpcError(
        'worker request to https://link.leanzero.net/v1/auth/request-code failed to send: connection refused'
      )
    );

    await userEvent.type(screen.getByTestId('link-email-input'), 'user@example.com');
    await userEvent.click(screen.getByTestId('link-send-code'));

    expect(
      await screen.findByText(/Couldn't reach the LeanZero Link service/i)
    ).toBeInTheDocument();
    // The raw URL is NOT leaked to the banner.
    expect(screen.queryByText(/failed to send/i)).not.toBeInTheDocument();
  });
});

describe('LeanZeroLinkSection — connected-view staleness gate', () => {
  it('surfaces the Reconnecting strip only after 3 consecutive status failures, then clears on success', async () => {
    vi.useFakeTimers();
    try {
      currentState = CONNECTED;
      mockNodes.mockResolvedValue(RUN_NODES);
      render();
      // Flush the mount-time status()+nodes() poll.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByTestId('link-connected')).toBeInTheDocument();
      expect(screen.queryByTestId('link-reconnecting')).not.toBeInTheDocument();

      // status() starts failing — a goosed death mid-connected.
      mockStatus.mockRejectedValue(rpcError('agent connection lost'));

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000); // failure #1
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000); // failure #2
      });
      // A transient blip (2) must NOT yank the user out or show the strip.
      expect(screen.queryByTestId('link-reconnecting')).not.toBeInTheDocument();
      expect(screen.getByTestId('link-connected')).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000); // failure #3
      });
      expect(screen.getByTestId('link-reconnecting')).toBeInTheDocument();
      // Still connected — not flashed back to loggedOut.
      expect(screen.getByTestId('link-connected')).toBeInTheDocument();
      expect(screen.queryByTestId('link-login-card')).not.toBeInTheDocument();

      // A single successful poll clears the strip.
      mockStatus.mockImplementation(async () => CONNECTED);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(screen.queryByTestId('link-reconnecting')).not.toBeInTheDocument();
      expect(screen.getByTestId('link-connected')).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it('a genuine auth transition (status → loggedOut) updates immediately and is not debounced', async () => {
    vi.useFakeTimers();
    try {
      currentState = CONNECTED;
      mockNodes.mockResolvedValue(RUN_NODES);
      render();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByTestId('link-connected')).toBeInTheDocument();

      // goosed reports a real logout on the very next poll.
      currentState = LOGGED_OUT;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(screen.getByTestId('link-login-card')).toBeInTheDocument();
      expect(screen.queryByTestId('link-connected')).not.toBeInTheDocument();
      expect(screen.queryByTestId('link-reconnecting')).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('LeanZeroLinkSection — LeanZero Studio register', () => {
  it('the sign-in card is Studio-clean and every class compiles', async () => {
    currentState = LOGGED_OUT;
    const { container } = render();
    await screen.findByTestId('link-login-card');
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);

  it('My Macs (every card, the switches, a lastError) is Studio-clean and compiles', async () => {
    currentState = { ...CONNECTED, lastError: 'mesh hiccup' };
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    mockHealth.mockResolvedValue({
      ok: true,
      version: '1.0.0',
      capabilities: { mail: false, audience: true, mesh: true },
    });
    const { container } = render();
    await screen.findByTestId('link-connected');
    await screen.findByTestId('link-deploy-banner');
    expect(await macCard('mihai-macbook-2-ff99aa')).toBeInTheDocument();
    await openDetails(await macCard('self'));
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
