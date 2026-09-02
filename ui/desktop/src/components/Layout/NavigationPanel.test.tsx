import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
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

const navMock = vi.hoisted(() => ({ expanded: true }));

vi.mock('./NavigationContext', () => ({
  useNavigationContext: () => ({ isNavExpanded: navMock.expanded, setIsNavExpanded: vi.fn() }),
}));

afterEach(() => {
  navMock.expanded = true;
});

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

const themeMock = vi.hoisted(() => ({
  preference: { current: 'system' as 'system' | 'light' | 'dark' },
  set: vi.fn(),
}));

vi.mock('../../contexts/ThemeContext', () => ({
  useTheme: () => ({
    userThemePreference: themeMock.preference.current,
    setUserThemePreference: themeMock.set,
    resolvedTheme: 'light',
    mcpHostStyles: {},
  }),
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
    const wordmark = within(brand).getByText('LeanZero Flock');
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
    const bottom = screen.getByTestId('nav-bottom');
    expect(bottom.contains(last)).toBe(true);
    expect(bottom.className).toContain('border-t');
    expect(bottom.className).toContain('border-lz-border');
    expect(screen.getByTestId('projects-section')).toBeInTheDocument();
  });

  it('the theme switch is its own full-width 36px row of three equal segments directly ABOVE Settings, so the Settings label keeps its room', () => {
    // Owner, 2026-09-02: side by side at the sidebar's 240px the switch (~112px) left the Settings
    // row ~38px for a ~52px word and it was clipped. Stacked, each row owns the whole width.
    renderNav('/settings');
    const bottom = screen.getByTestId('nav-bottom');
    expect(bottom.className).toContain('flex-col');
    const group = within(bottom).getByRole('radiogroup', { name: 'Theme' });
    const settings = within(bottom).getByRole('button', { name: /Settings/ });
    // Two rows of one block — never two cells of one row — and the switch comes first.
    expect(group.parentElement).toBe(bottom);
    expect(settings.parentElement).toBe(bottom);
    expect(group.compareDocumentPosition(settings) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(group.className).toContain('h-lz-row');
    expect(group.className).toContain('w-full');
    expect(group.className).toContain('[&>button]:flex-1');
    expect(group.className).toContain('[&>button]:justify-center');
    const label = within(settings).getByText('Settings');
    expect(label.className).toContain('whitespace-nowrap');
    expect(label.className).not.toMatch(/truncate|overflow-hidden|max-w-/);
    const radios = within(group).getAllByRole('radio');
    expect(radios.map((r) => r.getAttribute('title'))).toEqual(['System', 'Light', 'Dark']);
    expect(radios.map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    for (const r of radios) {
      expect(r.querySelector('svg')).not.toBeNull();
      expect(r.querySelector('.sr-only')?.textContent).toBe(r.getAttribute('title'));
    }
    // The checked segment is the accent fill; the others are ink on the surface.
    for (const c of SURFACE.selected.split(' ')) expect(radios[0].className).toContain(c);
    expect(radios[1].className).not.toContain('bg-lz-accent');
    fireEvent.click(radios[2]);
    expect(themeMock.set).toHaveBeenCalledWith('dark');
    // The bottom block's own button is still Settings — the segments are radios, not buttons.
    expect(within(bottom).getAllByRole('button')).toHaveLength(1);
  });

  it('no nav row label truncates, ellipsizes or is width-capped — every row is laid out with room for its word', () => {
    renderNav('/settings');
    const rows = screen.getAllByRole('button');
    expect(rows.length).toBeGreaterThan(3);
    for (const row of rows) {
      const label = row.querySelector('span.flex-1') as HTMLElement | null;
      expect(label, row.textContent ?? '').not.toBeNull();
      expect(label?.className).toContain('whitespace-nowrap');
      expect(label?.className).not.toMatch(/truncate|overflow-hidden|max-w-|text-ellipsis|line-clamp/);
    }
  });

  it('a collapsed sidebar renders neither rows nor the theme switch — nothing to clip', () => {
    navMock.expanded = false;
    const { container } = renderNav('/settings');
    expect(container.innerHTML).toBe('');
    expect(screen.queryByRole('radiogroup')).toBeNull();
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByText('Settings')).toBeNull();
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
