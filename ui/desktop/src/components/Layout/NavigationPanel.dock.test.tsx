import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { Navigation } from './NavigationPanel';
import { utilityCss } from '../lz/compileStudioCss';
import { resetEngineGlanceForTests } from '../engineGlance/glanceStore';
import { glancePush, runningSnapshot } from '../../utils/engineGlance.fixtures';
import { GENERATING_STATUS } from '../leanzero-swarm/mlxLiveStatus.fixtures';

/**
 * Q-216: an expanded tree drew UNDER the engine card. The card is now its own item in the
 * sidebar's flex column, AFTER the trees' scroll area — so by the flexbox rules the scroll area's
 * bottom is the card's top at every window height: the trees scroll, they never pass under it.
 *
 * jsdom lays nothing out, so this pins the facts that guarantee it, each compiled with the real
 * Tailwind pipeline (a class that compiles to nothing would be a silent no-op): the column is a
 * full-height flex column; the scroll area is the one item that grows (flex 1), keeps a fifth of the
 * column and scrolls; the card is the NEXT item, gives way in a short window (shrinks, scrolls in its
 * slot), and nothing on it or between it and the column is positioned out of the flow. The same
 * layout was measured in Chromium at 900/760/700/600/520/420 px (Q-216 commit): trees bottom ==
 * card slot top at every height, Settings inside the frame at every height.
 */

vi.mock('./NavigationContext', () => ({
  useNavigationContext: () => ({ isNavExpanded: true, setIsNavExpanded: vi.fn() }),
}));
vi.mock('../../contexts/EditionContext', () => ({
  useEdition: () => ({ edition: 'local', isLocal: true, setEdition: vi.fn() }),
}));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({
    localInference: true,
    mlxEngine: true,
    leanzeroLink: true,
    isLoading: false,
  }),
}));
vi.mock('./AgentWorkSection', () => ({ AgentWorkSection: () => <div /> }));
vi.mock('./BenchmarkSection', () => ({ BenchmarkSection: () => <div /> }));
vi.mock('./ProjectsSection', () => ({ ProjectsSection: () => <div /> }));
vi.mock('../../contexts/ThemeContext', () => ({
  useTheme: () => ({
    userThemePreference: 'system',
    setUserThemePreference: vi.fn(),
    resolvedTheme: 'light',
    mcpHostStyles: {},
  }),
}));

afterEach(() => resetEngineGlanceForTests(null));

async function cssOf(el: Element): Promise<string> {
  const classes = [...el.classList];
  return (await utilityCss(classes)).filter((c): c is string => c != null).join('\n');
}

describe('the engine card takes its own place in the sidebar column (Q-216)', () => {
  it('the trees’ scroll area ends where the card begins, by construction', async () => {
    resetEngineGlanceForTests(glancePush(runningSnapshot(GENERATING_STATUS)));
    render(
      <MemoryRouter initialEntries={['/']}>
        <IntlProvider locale="en" messages={{}}>
          <Navigation />
        </IntlProvider>
      </MemoryRouter>
    );
    const trees = screen.getByTestId('nav-trees');
    const dock = screen.getByTestId('engine-glance-dock');
    const column = trees.parentElement as HTMLElement;

    // Siblings in one column, the card straight after the scroll area, the bottom block after it.
    expect(dock.parentElement).toBe(column);
    expect(trees.nextElementSibling).toBe(dock);
    expect(dock.nextElementSibling).toBe(screen.getByTestId('nav-bottom'));

    const columnCss = await cssOf(column);
    expect(columnCss).toMatch(/display:\s*flex/);
    expect(columnCss).toMatch(/flex-direction:\s*column/);
    expect(columnCss).toMatch(/height:\s*100%/);

    const treesCss = await cssOf(trees);
    expect(treesCss).toMatch(/flex:\s*1/);
    // The sessions keep a fifth of the column however short the window: a card never takes it all.
    expect(treesCss).toMatch(/min-height:\s*20%/);
    expect(treesCss).toMatch(/overflow-y:\s*auto/);

    // The card gives way in a short window (it shrinks and scrolls inside its slot), so Settings
    // below it never leaves the frame.
    const dockCss = await cssOf(dock);
    expect(dockCss).toMatch(/min-height:\s*(0|calc\(var\(--spacing\) \* 0\))/);
    expect(dockCss).toMatch(/flex-shrink:\s*1/);
    expect(dockCss).toMatch(/overflow-y:\s*auto/);
    // Nothing from the card up to the column takes it out of the flow.
    for (let el: HTMLElement | null = dock; el && el !== column; el = el.parentElement) {
      expect(await cssOf(el)).not.toMatch(/position:\s*(absolute|fixed)/);
    }
    for (const card of dock.querySelectorAll('[data-testid="engine-glance"]')) {
      expect(await cssOf(card)).not.toMatch(/position:\s*(absolute|fixed)/);
    }
  }, 30_000);

  it('hidden, the same place holds the one row that brings it back — and nothing floats over the content', () => {
    const push = glancePush(runningSnapshot(GENERATING_STATUS), {}, undefined, { inApp: false });
    resetEngineGlanceForTests(push);
    render(
      <MemoryRouter initialEntries={['/']}>
        <IntlProvider locale="en" messages={{}}>
          <Navigation />
        </IntlProvider>
      </MemoryRouter>
    );
    expect(screen.queryByTestId('engine-glance')).toBeNull();
    expect(screen.getByTestId('nav-trees').nextElementSibling).toBe(
      screen.getByTestId('engine-glance-restore-slot')
    );
    expect(document.querySelector('[data-testid="engine-glance-float"]')).toBeNull();
  });
});
