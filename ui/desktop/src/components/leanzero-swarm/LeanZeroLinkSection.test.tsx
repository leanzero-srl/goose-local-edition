import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import LeanZeroLinkSection from './LeanZeroLinkSection';
import type { LinkHealth, LinkState, NodesResponse } from '../../acp/leanzero-link';

// Stub the seven network fns; keep the REAL error helpers (linkBannerText/linkErrorText),
// which the component relies on to render backend text verbatim.
const mockStatus = vi.fn();
const mockRequestCode = vi.fn();
const mockVerify = vi.fn();
const mockConnect = vi.fn();
const mockLogout = vi.fn();
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
      email: 'mihai@wolfaenpak.com',
      expiresAt: new Date(Date.now() + secondsAhead * 1000).toISOString(),
    },
    nodeCount: 0,
  };
}

const LOGGED_IN: LinkState = {
  auth: { state: 'loggedIn', email: 'mihai@wolfaenpak.com' },
  nodeCount: 0,
};

const CONNECTED: LinkState = {
  auth: { state: 'connected', email: 'mihai@wolfaenpak.com', meshIp: '100.64.0.1' },
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
    expect(screen.getByTestId('link-masked-email')).toHaveTextContent('m****@wolfaenpak.com');
    expect(screen.getByTestId('link-countdown').textContent).toMatch(/^[45]:\d\d$/);
  });

  it('loggedIn renders the Connect-to-mesh card', async () => {
    currentState = LOGGED_IN;
    render();
    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
    expect(screen.getByTestId('link-connect')).toHaveTextContent('Connect to mesh');
  });

  it('connecting renders the benchmark connecting state', async () => {
    currentState = { auth: { state: 'connecting', email: 'mihai@wolfaenpak.com' }, nodeCount: 0 };
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

    mockRequestCode.mockResolvedValue({ email: 'mihai@wolfaenpak.com', expiresInSeconds: 300 });
    currentState = codeSent(300);

    await userEvent.type(screen.getByTestId('link-email-input'), 'mihai@wolfaenpak.com');
    await userEvent.click(screen.getByTestId('link-send-code'));

    expect(mockRequestCode).toHaveBeenCalledWith('mihai@wolfaenpak.com');
    expect(await screen.findByTestId('link-code-input')).toBeInTheDocument();
    expect(screen.getByTestId('link-countdown').textContent).toMatch(/^[45]:\d\d$/);
  });

  it('verify success moves to loggedIn (the Connect card)', async () => {
    currentState = codeSent(300);
    render();
    await screen.findByTestId('link-code-input');

    mockVerify.mockResolvedValue({
      state: 'loggedIn',
      email: 'mihai@wolfaenpak.com',
      audienceSync: 'synced',
    });
    currentState = LOGGED_IN;

    await userEvent.type(screen.getByTestId('link-code-input'), '123456');
    await userEvent.click(screen.getByTestId('link-verify'));

    expect(mockVerify).toHaveBeenCalledWith('mihai@wolfaenpak.com', '123456');
    expect(await screen.findByTestId('link-connect-card')).toBeInTheDocument();
  });

  it('audienceSync "failed" shows a small amber note, not a blocker', async () => {
    currentState = codeSent(300);
    render();
    await screen.findByTestId('link-code-input');

    mockVerify.mockResolvedValue({
      state: 'loggedIn',
      email: 'mihai@wolfaenpak.com',
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
    expect(screen.getByTestId('link-peer-mihai-macbook-2-ff99aa')).toBeInTheDocument();
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

    const idlePeer = screen.getByTestId('link-peer-mihai-macbook-2-ff99aa');
    expect(idlePeer).toHaveTextContent('idle');
    const offlinePeer = screen.getByTestId('link-peer-studio-b-771122');
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

    await userEvent.type(screen.getByTestId('link-email-input'), 'mihai@wolfaenpak.com');
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

    await userEvent.type(screen.getByTestId('link-email-input'), 'mihai@wolfaenpak.com');
    await userEvent.click(screen.getByTestId('link-send-code'));

    expect(
      await screen.findByText(/Couldn't reach the LeanZero Link service/i)
    ).toBeInTheDocument();
    // The raw URL is NOT leaked to the banner.
    expect(screen.queryByText(/failed to send/i)).not.toBeInTheDocument();
  });
});
