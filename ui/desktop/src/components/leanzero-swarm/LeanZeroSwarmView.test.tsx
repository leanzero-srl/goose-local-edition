import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import LeanZeroSwarmView from './LeanZeroSwarmView';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

// The sections have their own suites — here only the SHELL is under test:
// the header, the section Segmented, which section each segment mounts, and that the
// LeanZero Link segment is gated on the `leanzeroLink` capability.
vi.mock('./MlxEngineView', () => ({ default: () => <div data-testid="mlx-panel" /> }));
vi.mock('./CloudProvidersSection', () => ({ default: () => <div data-testid="cloud-panel" /> }));
vi.mock('./SwarmNodesSection', () => ({ default: () => <div data-testid="swarm-panel" /> }));
vi.mock('./LeanZeroLinkSection', () => ({ default: () => <div data-testid="link-panel" /> }));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

let mockLeanzeroLink = false;
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: false,
    mlxEngine: false,
    leanzeroLink: mockLeanzeroLink,
    isLoading: false,
  }),
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

const render = () => rtlRender(<LeanZeroSwarmView />, { wrapper: IntlTestWrapper });
const segment = (name: string) => screen.getByRole('radio', { name });

afterEach(() => {
  cleanup();
  mockLeanzeroLink = false;
});

describe('LeanZeroSwarmView shell', () => {
  it('is titled Goose Swarm and renders the base three segments in one radiogroup', () => {
    render();
    expect(screen.getByRole('heading', { name: 'Goose Swarm' })).toBeInTheDocument();
    const group = screen.getByRole('radiogroup', { name: 'Goose Swarm sections' });
    expect(group).toBeInTheDocument();
    expect(segment('LeanZero MLX')).toBeInTheDocument();
    expect(segment('Cloud Providers')).toBeInTheDocument();
    expect(segment('Swarm Settings')).toBeInTheDocument();
  });

  it('defaults to the LeanZero MLX segment and switches sections per segment', async () => {
    render();
    expect(screen.getByTestId('mlx-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('cloud-panel')).not.toBeInTheDocument();

    await userEvent.click(segment('Cloud Providers'));
    expect(screen.getByTestId('cloud-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-panel')).not.toBeInTheDocument();

    await userEvent.click(segment('Swarm Settings'));
    expect(screen.getByTestId('swarm-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('cloud-panel')).not.toBeInTheDocument();

    // aria-checked follows the active segment (the state is visible to more than a sighted mouse user)
    expect(segment('Swarm Settings').getAttribute('aria-checked')).toBe('true');
    expect(segment('LeanZero MLX').getAttribute('aria-checked')).toBe('false');
  });

  it('hides the LeanZero Link segment when the capability is absent', () => {
    mockLeanzeroLink = false;
    render();
    expect(screen.queryByRole('radio', { name: 'LeanZero Link' })).not.toBeInTheDocument();
  });

  it('shows the LeanZero Link segment and mounts its section when the capability is present', async () => {
    mockLeanzeroLink = true;
    render();
    const linkTab = segment('LeanZero Link');
    await userEvent.click(linkTab);
    expect(screen.getByTestId('link-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-panel')).not.toBeInTheDocument();
  });

  it('the shell is Studio-clean (no rail, no tint, no native control) and every class compiles', async () => {
    mockLeanzeroLink = true;
    const { container } = render();
    assertStudioClean(container);
    // `page-transition` is a plain rule in main.css, not a utility; lucide stamps its own names.
    const classes = allClasses(container).filter(
      (c) => !c.startsWith('lucide') && c !== 'page-transition'
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
