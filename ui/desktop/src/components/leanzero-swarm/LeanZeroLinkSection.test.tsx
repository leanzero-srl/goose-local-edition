import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, cleanup, render as rtlRender, screen } from '@testing-library/react';
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

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const render = () => rtlRender(<LeanZeroLinkSection />, { wrapper: IntlTestWrapper });

/** A peer's row in the Linked-devices DataTable, keyed by node id (rows carry `data-key`). */
const peerRow = (nodeId: string): HTMLElement => {
  const el = document.querySelector<HTMLElement>(
    `[data-testid="link-peers"] [data-testid="lz-row"][data-key="${nodeId}"]`
  );
  if (!el) throw new Error(`no peer row for ${nodeId}`);
  return el;
};

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
    expect(screen.getByTestId('link-mesh-line')).toHaveTextContent(/mesh Running · online/);
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
    expect(peerRow('mihai-macbook-2-ff99aa')).toBeInTheDocument();
  });

  it('connect failure renders lastError in a solid banner and stays on the Connect card', async () => {
    currentState = LOGGED_IN;
    render();
    await screen.findByTestId('link-connect-card');

    mockConnect.mockRejectedValue(rpcError('mesh joined but reported no IP — cannot compose a Connected state'));
    // The status poll after failure reconciles back to loggedIn.
    currentState = LOGGED_IN;

    await userEvent.click(screen.getByTestId('link-connect'));

    expect(
      await screen.findByText(/mesh joined but reported no IP/i)
    ).toBeInTheDocument();
    expect(screen.getByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-connect')).toHaveTextContent('Retry connect');
  });
});

describe('LeanZeroLinkSection — connected dashboard', () => {
  it('renders self + peers with idle/busy/offline chips', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    render();

    const self = await screen.findByTestId('link-self');
    expect(self).toHaveTextContent('works-mac-studio');
    expect(self).toHaveTextContent('busy');

    const idlePeer = peerRow('mihai-macbook-2-ff99aa');
    expect(idlePeer).toHaveTextContent('idle');
    const offlinePeer = peerRow('studio-b-771122');
    expect(offlinePeer).toHaveTextContent('offline');
  });

  it('renders the honest empty state when there are no peers', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue({ self: NODES_WITH_PEERS.self, peers: [] });
    render();
    expect(await screen.findByTestId('link-peers-empty')).toHaveTextContent(
      /No other devices linked yet/i
    );
  });

  it('logout confirms via a custom dialog and passes wipe:true when the box is checked', async () => {
    currentState = CONNECTED;
    mockNodes.mockResolvedValue(NODES_WITH_PEERS);
    mockLogout.mockResolvedValue(LOGGED_OUT);
    render();
    await screen.findByTestId('link-connected');

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
    expect(banner).toHaveTextContent(/did not come back: mesh join failed: control plane unreachable/);
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
    expect(await screen.findByText(/bringing this mac back onto your private mesh/i)).toBeInTheDocument();
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

  it('a reconnect left to another window\'s backend says so; a fresh sign-in gets the plain card', async () => {
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
      reconnect: { state: 'skipped', reason: 'this Mac has never been connected under this sign-in' },
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

    const verbatim = 'rate limited on request-code; retry after 42s (worker said: too many requests)';
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

  it('the connected dashboard (peers table, this-device panel) is Studio-clean and compiles', async () => {
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
    expect(peerRow('mihai-macbook-2-ff99aa')).toBeInTheDocument();
    // The Linked devices header counts what the table shows.
    expect(screen.getByTestId('lz-section-count')).toHaveTextContent('2');
    assertStudioClean(container);
    const classes = allClasses(container).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
