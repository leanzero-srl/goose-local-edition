import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import LeanZeroSwarmView from './LeanZeroSwarmView';

// The three sections have their own suites — here only the SHELL is under test:
// the header, the three tabs, and which section each one mounts.
vi.mock('./MlxEngineView', () => ({ default: () => <div data-testid="mlx-panel" /> }));
vi.mock('./CloudProvidersSection', () => ({ default: () => <div data-testid="cloud-panel" /> }));
vi.mock('./SwarmNodesSection', () => ({ default: () => <div data-testid="swarm-panel" /> }));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const render = () => rtlRender(<LeanZeroSwarmView />, { wrapper: IntlTestWrapper });

afterEach(() => cleanup());

describe('LeanZeroSwarmView shell', () => {
  it('is titled LeanZero Swarm and renders exactly the three tabs', () => {
    render();
    expect(screen.getByRole('heading', { name: 'LeanZero Swarm' })).toBeInTheDocument();
    expect(screen.getByTestId('leanzero-swarm-tab-mlx')).toHaveTextContent('LeanZero MLX');
    expect(screen.getByTestId('leanzero-swarm-tab-cloud')).toHaveTextContent('Cloud Providers');
    expect(screen.getByTestId('leanzero-swarm-tab-swarm')).toHaveTextContent('Swarm Settings');
  });

  it('defaults to the LeanZero MLX tab and switches sections per tab', async () => {
    render();
    expect(screen.getByTestId('mlx-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('cloud-panel')).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId('leanzero-swarm-tab-cloud'));
    expect(screen.getByTestId('cloud-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-panel')).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId('leanzero-swarm-tab-swarm'));
    expect(screen.getByTestId('swarm-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('cloud-panel')).not.toBeInTheDocument();

    // aria-pressed follows the active tab (the state is visible to more than a sighted mouse user)
    expect(screen.getByTestId('leanzero-swarm-tab-swarm').getAttribute('aria-pressed')).toBe(
      'true'
    );
    expect(screen.getByTestId('leanzero-swarm-tab-mlx').getAttribute('aria-pressed')).toBe('false');
  });
});
