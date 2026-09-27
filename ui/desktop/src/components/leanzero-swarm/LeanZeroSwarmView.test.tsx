import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlProvider } from 'react-intl';
import { MemoryRouter, useLocation } from 'react-router-dom';
import LeanZeroSwarmView from './LeanZeroSwarmView';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import de from '../../i18n/messages/de.json';

// The sections have their own suites — here only the SHELL is under test: the header, the two
// Providers segments, which section each mounts, and what the LeanZero MLX panel is handed.
vi.mock('./MlxEngineView', () => ({
  default: ({ tab }: { tab: string }) => <div data-testid="mlx-panel" data-tab={tab} />,
}));
vi.mock('./CloudProvidersSection', () => ({ default: () => <div data-testid="cloud-panel" /> }));
// The shell provides the linked Macs to every tab; the provider has its own suite.
vi.mock('./useMacs', () => ({
  MacsProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

function Where() {
  const location = useLocation();
  return <output data-testid="where">{location.search}</output>;
}

const renderView = (
  path = '/leanzero-swarm',
  locale = 'en',
  messages: Record<string, string> = {}
) =>
  render(
    <IntlProvider locale={locale} defaultLocale="en" messages={messages}>
      <MemoryRouter initialEntries={[path]}>
        <LeanZeroSwarmView />
        <Where />
      </MemoryRouter>
    </IntlProvider>
  );
const segment = (name: string) => screen.getByRole('radio', { name });

afterEach(() => {
  cleanup();
});

describe('LeanZeroSwarmView shell', () => {
  it('is titled Providers and has exactly two segments: LeanZero MLX and Cloud Providers', () => {
    renderView();
    expect(screen.getByRole('heading', { name: 'Providers' })).toBeInTheDocument();
    const group = screen.getByRole('radiogroup', { name: 'Providers sections' });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole('radio').map((r) => r.textContent)).toEqual([
      'LeanZero MLX',
      'Cloud Providers',
    ]);
    // Swarm Settings moved to the Nodes page, My Macs inside LeanZero MLX (Q-193, Q-194).
    expect(screen.queryByRole('radio', { name: 'Swarm Settings' })).not.toBeInTheDocument();
    expect(screen.queryByRole('radio', { name: 'My Macs' })).not.toBeInTheDocument();
  });

  it('the title, subtitle and segments come from the catalog, never hardcoded English', () => {
    const messages = Object.fromEntries(
      Object.entries(de).map(([key, value]) => [key, value.defaultMessage])
    );
    renderView('/leanzero-swarm', 'de', { ...messages, 'providers.title': 'Anbieter (Test)' });
    expect(screen.getByRole('heading', { name: 'Anbieter (Test)' })).toBeInTheDocument();
    expect(screen.getByText(/^Wo deine Modelle laufen/)).toBeInTheDocument();
    expect(segment('Cloud-Anbieter')).toBeInTheDocument();
    expect(screen.getByRole('radiogroup', { name: 'Providers-Bereiche' })).toBeInTheDocument();
  });

  it('the subtitle says where models run and points at Nodes', () => {
    renderView();
    expect(
      screen.getByText(
        "Where your models run: the LeanZero MLX engine on your Macs, and the cloud providers you've signed in to. Turn them into nodes under Nodes."
      )
    ).toBeInTheDocument();
  });

  it('defaults to LeanZero MLX on its Engine tab and switches sections per segment, in the URL', async () => {
    renderView();
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'engine');
    expect(screen.queryByTestId('cloud-panel')).not.toBeInTheDocument();

    await userEvent.click(segment('Cloud Providers'));
    expect(screen.getByTestId('cloud-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-panel')).not.toBeInTheDocument();
    expect(screen.getByTestId('where').textContent).toBe('?tab=cloud');
    expect(segment('Cloud Providers').getAttribute('aria-checked')).toBe('true');
    expect(segment('LeanZero MLX').getAttribute('aria-checked')).toBe('false');
  });

  it('hands the LeanZero MLX panel the routed inner tab', () => {
    renderView('/leanzero-swarm?tab=mlx&mlx=models');
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'models');
  });

  it('the shell is Studio-clean (no rail, no tint, no native control) and every class compiles', async () => {
    const { container } = renderView();
    assertStudioClean(container);
    // `page-transition` is a plain rule in main.css, not a utility; lucide stamps its own names.
    const classes = allClasses(container).filter(
      (c) => !c.startsWith('lucide') && c !== 'page-transition'
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
