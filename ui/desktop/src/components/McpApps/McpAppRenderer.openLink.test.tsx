import { render, screen, waitFor, within, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

type OpenLinkHandler = (params: {
  url: string;
}) => Promise<{ status: 'success' | 'error'; message?: string }>;

// The guest app reaches the host's open-link handler through the AppBridge; the fake captures the
// handler the renderer installs so the test can make the request a real guest would make.
const bridgeState = vi.hoisted(() => ({ onopenlink: null as OpenLinkHandler | null }));

vi.mock('@mcp-ui/client', () => ({
  AppBridge: class {
    set onopenlink(handler: OpenLinkHandler) {
      bridgeState.onopenlink = handler;
    }
    connect() {
      return Promise.resolve();
    }
    close() {}
    getAppVersion() {
      return undefined;
    }
    getAppCapabilities() {
      return undefined;
    }
    setHostContext() {}
    sendToolInput() {}
    sendToolResult() {}
    sendSandboxResourceReady() {
      return Promise.resolve();
    }
  },
  PostMessageTransport: class {},
}));
vi.mock('../../acp/mcp-apps', () => ({
  readMcpAppResource: vi.fn(async () => ({ text: '<html><body>app</body></html>' })),
  callMcpAppTool: vi.fn(),
}));
vi.mock('./toolsCache', () => ({ getCachedTools: vi.fn(async () => null) }));
vi.mock('../../contexts/ThemeContext', () => ({
  useTheme: () => ({ resolvedTheme: 'light', mcpHostStyles: {} }),
}));
vi.mock('../settings/extensions/subcomponents/ExtensionList', () => ({
  formatExtensionName: (name: string) => name,
}));

import McpAppRenderer from './McpAppRenderer';

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

const electron = window.electron as unknown as Record<string, unknown>;

describe('an MCP app asking to open an unknown-protocol link gets an in-app confirm', () => {
  let nativeBox: ReturnType<typeof vi.fn>;
  let openExternal: ReturnType<typeof vi.fn>;
  let previous: Record<string, unknown>;

  beforeEach(() => {
    bridgeState.onopenlink = null;
    vi.stubGlobal('ResizeObserver', ResizeObserverMock);
    vi.stubGlobal(
      'matchMedia',
      vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
    );
    nativeBox = vi.fn(() => Promise.resolve({ response: 1 }));
    openExternal = vi.fn(() => Promise.resolve());
    previous = {
      showMessageBox: electron.showMessageBox,
      openExternal: electron.openExternal,
      getAcpUrl: electron.getAcpUrl,
      getSecretKey: electron.getSecretKey,
    };
    Object.assign(electron, {
      showMessageBox: nativeBox,
      openExternal,
      getAcpUrl: vi.fn(async () => 'ws://127.0.0.1:3000/acp'),
      getSecretKey: vi.fn(async () => 'secret'),
    });
  });

  afterEach(() => {
    Object.assign(electron, previous);
    vi.unstubAllGlobals();
  });

  it('cancel answers the guest "User cancelled"; Open opens the link and answers success', async () => {
    const user = userEvent.setup();
    render(
      <IntlTestWrapper>
        <McpAppRenderer resourceUri="ui://app" extensionName="demo" sessionId="s1" />
      </IntlTestWrapper>
    );
    await waitFor(() => expect(bridgeState.onopenlink).not.toBeNull());

    let answer!: Promise<{ status: string; message?: string }>;
    act(() => {
      answer = bridgeState.onopenlink!({ url: 'myapp://thing' });
    });
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Open myapp: link?')).toBeInTheDocument();
    expect(within(dialog).getByText('This will open: myapp://thing')).toBeInTheDocument();
    // Above the fullscreen (z-1000) and PiP (z-900) app containers the request can come from.
    expect(dialog).toHaveClass('z-[1100]');
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await expect(answer).resolves.toEqual({ status: 'error', message: 'User cancelled' });
    expect(openExternal).not.toHaveBeenCalled();

    act(() => {
      answer = bridgeState.onopenlink!({ url: 'myapp://thing' });
    });
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Open' }));
    await expect(answer).resolves.toEqual({ status: 'success' });
    expect(openExternal).toHaveBeenCalledWith('myapp://thing');
    expect(nativeBox).not.toHaveBeenCalled();
  });
});
