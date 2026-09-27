import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { PROVIDER_ROUTES } from './providerRoutes';
import { ENGINE_ROUTE } from '../noNodeNotice/ComposerReadiness';
import { providersHref, type MlxTab } from '../../utils/navigationUtils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * Every row of DESIGN-NODES-AND-STRATEGIES.md §5.2, through the SAME route table App.tsx mounts
 * (PROVIDER_ROUTES): the Nodes page and its tabs, the Providers tabs and LeanZero MLX's routed inner
 * tabs, the two retired-tab redirects, the old /mlx-engine path, ComposerReadiness's ENGINE_ROUTE and
 * main's set-view sections. The section bodies have their own suites; here each is a marker that
 * shows what the route handed it.
 */
vi.mock('../leanzero-swarm/MlxEngineView', () => ({
  default: ({
    tab,
    onTabChange,
    onOpenNodes,
  }: {
    tab: MlxTab;
    onTabChange: (tab: MlxTab) => void;
    onOpenNodes: () => void;
  }) => (
    <div data-testid="mlx-panel" data-tab={tab}>
      <button onClick={() => onTabChange('sampling')}>stub: open Sampling</button>
      <button onClick={onOpenNodes}>stub: open Nodes</button>
    </div>
  ),
}));
vi.mock('../leanzero-swarm/CloudProvidersSection', () => ({
  default: () => <div data-testid="cloud-panel" />,
}));
vi.mock('../leanzero-swarm/SwarmNodesSection', () => ({
  default: ({ onOpenCloudProviders }: { onOpenCloudProviders: () => void }) => (
    <div data-testid="swarm-nodes-section">
      <button onClick={onOpenCloudProviders}>stub: no key, open Cloud Providers</button>
    </div>
  ),
}));
vi.mock('../leanzero-swarm/useMacs', () => ({
  MacsProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: async () => ({ devices: [{ id: 'a' }, { id: 'b' }] }) }),
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', ResizeObserverMock);

/** The current location, and two ways to leave it — the history a person walks with Back. */
function Where() {
  const location = useLocation();
  const navigate = useNavigate();
  return (
    <>
      <output data-testid="where">{location.pathname + location.search}</output>
      <button onClick={() => navigate('/elsewhere')}>test: leave</button>
      <button onClick={() => navigate(-1)}>test: back</button>
    </>
  );
}

function renderAt(path: string) {
  return render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          {PROVIDER_ROUTES.map((route) => (
            <Route key={route.path} path={route.path} element={route.element} />
          ))}
          <Route path="elsewhere" element={<div data-testid="elsewhere" />} />
        </Routes>
        <Where />
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

const where = () => screen.getByTestId('where').textContent;
const radio = (group: string, name: string) =>
  within(screen.getByRole('radiogroup', { name: group })).getByRole('radio', { name });

afterEach(cleanup);

describe('§5.2 — the Nodes page', () => {
  it.each(['/nodes', '/nodes?tab=nodes'])('%s opens the Nodes tab hosting the pool', (path) => {
    renderAt(path);
    expect(screen.getByRole('heading', { name: 'Nodes' })).toBeInTheDocument();
    expect(radio('Nodes sections', 'Nodes')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('swarm-nodes-section')).toBeInTheDocument();
    expect(screen.queryByTestId('strategies-tab')).not.toBeInTheDocument();
  });

  it('/nodes?tab=strategies opens the Strategies tab with its honest empty state — no fake editor', () => {
    renderAt('/nodes?tab=strategies');
    expect(radio('Nodes sections', 'Strategies')).toHaveAttribute('aria-checked', 'true');
    const tab = screen.getByTestId('strategies-tab');
    expect(
      within(tab).getByRole('heading', { name: 'Strategies are coming in this release' })
    ).toBeInTheDocument();
    expect(within(tab).queryByRole('button')).toBeNull();
    expect(screen.queryByTestId('swarm-nodes-section')).not.toBeInTheDocument();
  });

  it('a node= or strategy= deep link opens its tab and keeps the id in the URL for S2/S6', () => {
    renderAt('/nodes?tab=nodes&node=mihai-mlx');
    expect(screen.getByTestId('swarm-nodes-section')).toBeInTheDocument();
    expect(where()).toBe('/nodes?tab=nodes&node=mihai-mlx');
    cleanup();
    renderAt('/nodes?tab=strategies&strategy=everyday');
    expect(screen.getByTestId('strategies-tab')).toBeInTheDocument();
    expect(where()).toBe('/nodes?tab=strategies&strategy=everyday');
  });

  it('a tab click writes the URL in place, and Back from another page restores that tab', async () => {
    renderAt('/nodes');
    await userEvent.click(radio('Nodes sections', 'Strategies'));
    expect(where()).toBe('/nodes?tab=strategies');
    await userEvent.click(screen.getByText('test: leave'));
    expect(screen.getByTestId('elsewhere')).toBeInTheDocument();
    await userEvent.click(screen.getByText('test: back'));
    expect(radio('Nodes sections', 'Strategies')).toHaveAttribute('aria-checked', 'true');
  });

  it('the page links to where its resources are managed: My Macs and Cloud Providers', async () => {
    renderAt('/nodes');
    await userEvent.click(screen.getByRole('button', { name: 'Manage Macs and models' }));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=macs');
    cleanup();
    renderAt('/nodes');
    await userEvent.click(screen.getByRole('button', { name: 'Manage cloud providers' }));
    expect(where()).toBe('/leanzero-swarm?tab=cloud');
    cleanup();
    renderAt('/nodes');
    await userEvent.click(screen.getByText('stub: no key, open Cloud Providers'));
    expect(where()).toBe('/leanzero-swarm?tab=cloud');
  });

  it('the page is Studio-clean on both tabs and every class compiles', async () => {
    const { container } = renderAt('/nodes');
    assertStudioClean(container);
    await userEvent.click(radio('Nodes sections', 'Strategies'));
    assertStudioClean(container);
    const classes = allClasses(container).filter(
      (c) => !c.startsWith('lucide') && c !== 'page-transition'
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('§5.2 — Providers and LeanZero MLX', () => {
  it.each<[string, MlxTab]>([
    ['/leanzero-swarm', 'engine'],
    ['/leanzero-swarm?tab=mlx', 'engine'],
    ['/leanzero-swarm?tab=mlx&mlx=engine', 'engine'],
    ['/leanzero-swarm?tab=mlx&mlx=macs', 'macs'],
    ['/leanzero-swarm?tab=mlx&mlx=models', 'models'],
    ['/leanzero-swarm?tab=mlx&mlx=sampling', 'sampling'],
    ['/leanzero-swarm?tab=mlx&mlx=bogus', 'engine'],
  ])('%s opens LeanZero MLX on its %s tab', (path, inner) => {
    renderAt(path);
    expect(radio('Providers sections', 'LeanZero MLX')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', inner);
  });

  it('/leanzero-swarm?tab=cloud opens Cloud Providers', () => {
    renderAt('/leanzero-swarm?tab=cloud');
    expect(radio('Providers sections', 'Cloud Providers')).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('cloud-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('mlx-panel')).not.toBeInTheDocument();
  });

  it('?tab=swarm (old Swarm Settings) redirects to the Nodes page', () => {
    renderAt('/leanzero-swarm?tab=swarm');
    expect(where()).toBe('/nodes');
    expect(screen.getByTestId('swarm-nodes-section')).toBeInTheDocument();
  });

  it('?tab=link (old My Macs) redirects to LeanZero MLX › My Macs', () => {
    renderAt('/leanzero-swarm?tab=link');
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=macs');
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'macs');
  });

  it('the old /mlx-engine path opens the Engine tab', () => {
    renderAt('/mlx-engine');
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=engine');
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'engine');
  });

  it("ComposerReadiness's ENGINE_ROUTE (Open Engine) still lands on the Engine tab", () => {
    renderAt(ENGINE_ROUTE);
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'engine');
  });

  it('an inner tab change writes mlx= in place; Back from another page restores it', async () => {
    renderAt('/leanzero-swarm?tab=mlx');
    await userEvent.click(screen.getByText('stub: open Sampling'));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=sampling');
    await userEvent.click(screen.getByText('test: leave'));
    await userEvent.click(screen.getByText('test: back'));
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'sampling');
  });

  it('switching to Cloud and back keeps the URL the single source of the tab', async () => {
    renderAt('/leanzero-swarm?tab=mlx&mlx=models');
    await userEvent.click(radio('Providers sections', 'Cloud Providers'));
    expect(where()).toBe('/leanzero-swarm?tab=cloud');
    await userEvent.click(radio('Providers sections', 'LeanZero MLX'));
    expect(where()).toBe('/leanzero-swarm?tab=mlx&mlx=engine');
  });

  it("the setup strip's Nodes step opens the Nodes page", async () => {
    renderAt('/leanzero-swarm?tab=mlx');
    await userEvent.click(screen.getByText('stub: open Nodes'));
    expect(where()).toBe('/nodes');
  });

  it('Providers keeps exactly two tabs: LeanZero MLX and Cloud Providers', async () => {
    renderAt('/leanzero-swarm');
    await act(async () => {});
    const group = screen.getByRole('radiogroup', { name: 'Providers sections' });
    expect(
      within(group)
        .getAllByRole('radio')
        .map((r) => r.textContent)
    ).toEqual(['LeanZero MLX', 'Cloud Providers']);
  });
});

describe("main's set-view deep links (App.tsx handleSetView → providersHref)", () => {
  it.each([
    // tray "Open Providers" and the engine glance's click-to-Engine (main.ts runMlxTrayAction)
    ['mlx', '/leanzero-swarm?tab=mlx&mlx=engine'],
    // the Link tray's "Open My Macs" (main.ts runLinkTrayAction sends 'link')
    ['link', '/leanzero-swarm?tab=mlx&mlx=macs'],
    ['macs', '/leanzero-swarm?tab=mlx&mlx=macs'],
    ['models', '/leanzero-swarm?tab=mlx&mlx=models'],
    ['cloud', '/leanzero-swarm?tab=cloud'],
    ['swarm', '/nodes'],
    ['unknown', '/leanzero-swarm'],
  ])('section %s opens %s', (section, href) => {
    expect(providersHref(section)).toBe(href);
  });

  it("the Link tray's deep link lands on My Macs end to end", () => {
    renderAt(providersHref('link'));
    expect(screen.getByTestId('mlx-panel')).toHaveAttribute('data-tab', 'macs');
  });
});
