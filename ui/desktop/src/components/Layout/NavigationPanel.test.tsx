import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { Navigation } from './NavigationPanel';
import { SURFACE, TYPE } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * The sidebar shell in the Studio register (ui/desktop/DESIGN.md): a brand block, 36px nav rows
 * whose current view is the accent fill (never a rail), Settings pinned under a hairline. Every
 * class the shell emits is compiled against main.css — a class that produces no rule is a
 * silent no-op in the app.
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

vi.mock('./ProjectsSection', () => ({
  ProjectsSection: () => <div data-testid="projects-section" />,
}));

const renderNav = (path = '/benchmark') =>
  render(
    <MemoryRouter initialEntries={[path]}>
      <IntlProvider locale="en" messages={{}}>
        <Navigation />
      </IntlProvider>
    </MemoryRouter>
  );

describe('NavigationPanel (Studio shell)', () => {
  it('opens with the brand block: a solid accent square mark beside the wordmark in the h2 step', () => {
    renderNav();
    const brand = screen.getByTestId('brand-block');
    const mark = screen.getByTestId('brand-mark');
    expect(mark.className).toContain('bg-lz-accent');
    expect(mark.className).toContain('text-lz-accent-ink');
    expect(mark.className).toContain('rounded-lz-control');
    expect(mark.className).not.toMatch(/gradient/);
    expect(within(mark).getByTestId('leanzero-glyph')).toBeInTheDocument();
    const wordmark = within(brand).getByText('LeanZero Swarm');
    for (const c of TYPE.h2.split(' ')) expect(wordmark.className).toContain(c);
  });

  it('nav rows are 36px icon+label rows; the current view is the accent fill with accent ink, never a rail', () => {
    renderNav('/benchmark');
    const rows = screen.getAllByRole('button');
    const current = rows.filter((b) => b.getAttribute('aria-current') === 'page');
    expect(current).toHaveLength(1);
    expect(current[0].textContent).toContain('Benchmark');
    for (const c of SURFACE.selected.split(' ')) expect(current[0].className).toContain(c);
    expect(current[0].className).toContain('h-lz-row');
    expect(current[0].className).toContain('rounded-lz-control');
    expect(current[0].className).not.toMatch(/border-l/);
    expect(current[0].querySelector('svg')).not.toBeNull();

    const idle = rows.find((b) => (b.textContent ?? '').includes('Skills'));
    expect(idle).toBeDefined();
    expect(idle?.className).not.toContain('bg-lz-accent');
    expect(idle?.className).toContain(SURFACE.hover);
    expect(idle?.className).toContain('font-lz-medium');
  });

  it('Settings is the last row, pinned under a hairline, and lights up like any other row', () => {
    renderNav('/settings');
    const rows = screen.getAllByRole('button');
    const last = rows[rows.length - 1];
    expect(last.textContent).toContain('Settings');
    expect(last.getAttribute('aria-current')).toBe('page');
    expect(last.parentElement?.className).toContain('border-t');
    expect(last.parentElement?.className).toContain('border-lz-border');
    expect(screen.getByTestId('projects-section')).toBeInTheDocument();
  });

  it('carries no banned pattern below the fade root, and every class compiles against main.css', async () => {
    const { container } = renderNav();
    // The root is framer-motion's mount fade (opacity 0 → 1, collapse behaviour unchanged); the
    // Studio bans are asserted on everything it contains.
    const shell = container.firstElementChild as HTMLElement;
    expect(shell.className).toContain('bg-lz-surface');
    assertStudioClean(shell);
    const classes = allClasses(shell).filter((c) => !c.startsWith('lucide') && c !== 'no-drag');
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
