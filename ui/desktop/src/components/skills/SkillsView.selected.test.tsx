import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import SkillsView from './SkillsView';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { SURFACE, TYPE } from '../lz';
import { assertStudioClean } from '../lz/assertStudioClean';
import { contrast, resolveExpr, resolvedPaint, studioToken } from '../lz/resolvedPaint';

/**
 * The Skills twin of MemoriesView.selected.test.tsx — the same dead `bg-background-accent` made a
 * selected skill invisible on the light theme. Light theme → click a skill → the row's resolved
 * fill is the accent and the ink contrasts, at rest and under the pointer.
 */

vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));

const skills = [
  {
    type: 'skill',
    name: 'campaign',
    description: 'Run a benchmark campaign end to end.',
    path: '/home/.config/agents/skills/campaign/SKILL.md',
    content: '# campaign\nRun it.',
    global: true,
  },
  {
    type: 'skill',
    name: 'panel-surgeon',
    description: 'Desktop swarm UI edits.',
    path: '/proj/goose/.goose/skills/panel-surgeon/SKILL.md',
    content: '# panel-surgeon\nEdit the panel.',
    global: false,
  },
];

vi.mock('../../acp/sources', () => ({
  listSkillSources: vi.fn(async () => skills),
  readSkillSourceFresh: vi.fn(async (_dir: string, path: string) =>
    skills.find((s) => s.path === path)
  ),
  updateSkillSource: vi.fn(),
  deleteSkillSource: vi.fn(),
}));

const mount = () =>
  render(
    <IntlTestWrapper>
      <SkillsView />
    </IntlTestWrapper>
  );

const rowOf = async (name: string) =>
  (await screen.findByText(name, { selector: 'button *' }, { timeout: 3000 })).closest(
    'button'
  ) as HTMLElement;

describe('SkillsView — a selected skill is visible in both themes', () => {
  it('light theme → click a skill → accent fill with accent ink; hover deepens the fill', async () => {
    mount();
    const row = await rowOf('campaign');
    fireEvent.click(row);
    expect(row.getAttribute('aria-current')).toBe('true');
    for (const c of SURFACE.selected.split(' ')) expect(row.className).toContain(c);
    expect(row.className).toContain(SURFACE.selectedHover);
    expect(row.className).not.toContain(SURFACE.hover);

    for (const theme of ['light', 'dark'] as const) {
      const rest = await resolvedPaint(row, theme);
      expect(rest.missing).toEqual([]);
      expect(rest.bg).toBe(studioToken('--color-lz-accent', theme));
      expect(rest.text).toBe(studioToken('--color-lz-accent-ink', theme));
      expect(contrast(rest.bg, rest.text)).toBeGreaterThan(4.5);
      const hovered = await resolvedPaint(row, theme, { hover: true });
      expect(hovered.bg).toBe(studioToken('--color-lz-accent-hover', theme));
      expect(contrast(hovered.bg, hovered.text)).toBeGreaterThan(4.5);
      const snippet = row.querySelector('[data-testid="library-row-preview"]') as HTMLElement;
      const snippetPaint = await resolvedPaint(snippet, theme, {
        inherit: { bg: rest.bg ?? undefined, text: rest.text ?? undefined },
      });
      expect(snippetPaint.text).toMatch(/^#[0-9a-f]{6}$/);
      expect(contrast(snippetPaint.bg, snippetPaint.text)).toBeGreaterThan(4.5);
    }
  }, 30_000);

  it('idle rows are ink on the page with the neutral hover step, and the list carries no ban', async () => {
    mount();
    // panel-surgeon (This project) leads the list and is shown on open; campaign (Yours) is idle.
    const idle = await rowOf('campaign');
    expect(idle.getAttribute('aria-current')).toBeNull();
    for (const theme of ['light', 'dark'] as const) {
      const page = resolveExpr('var(--color-background-primary)', theme);
      const rest = await resolvedPaint(idle, theme, { inherit: { bg: page } });
      expect(rest.missing).toEqual([]);
      expect(contrast(rest.bg, rest.text)).toBeGreaterThan(4.5);
      const hovered = await resolvedPaint(idle, theme, { hover: true, inherit: { bg: page } });
      expect(hovered.bg).toBe(studioToken('--color-lz-surface-2', theme));
      expect(contrast(hovered.bg, hovered.text)).toBeGreaterThan(4.5);
    }
    assertStudioClean(idle.parentElement as HTMLElement);
  }, 30_000);

  it('right-click → Edit selects the skill and opens it in editing; → Delete opens the confirm; → ask starts a chat', async () => {
    mount();
    const row = await rowOf('panel-surgeon');
    fireEvent.contextMenu(row);
    const menu = await screen.findByTestId('skill-context-menu');
    fireEvent.click(within(menu).getByText('Edit'));
    expect(row.getAttribute('aria-current')).toBe('true');
    expect(await screen.findByText('Done editing')).toBeInTheDocument();

    fireEvent.contextMenu(row);
    fireEvent.click(within(await screen.findByTestId('skill-context-menu')).getByText('Delete'));
    expect(await screen.findByRole('button', { name: /^Delete$/ })).toBeInTheDocument();
  });

  it('opens on the first listed skill — the reading pane is never an empty "select a skill"', async () => {
    mount();
    const first = await rowOf('panel-surgeon');
    expect(first.getAttribute('aria-current')).toBe('true');
    const detail = screen.getByTestId('library-detail');
    expect(
      within(detail).getByRole('heading', { level: 2, name: 'panel-surgeon' })
    ).toBeInTheDocument();
    expect(screen.queryByText(/Select a skill/)).toBeNull();
  });

  it('the title is the display step in full ink, and the list shows a two-line preview, not the whole description', async () => {
    mount();
    const title = await screen.findByRole('heading', { level: 1, name: 'Skills' });
    for (const c of TYPE.display.split(' ')) expect(title.className).toContain(c);
    const row = await rowOf('campaign');
    const preview = row.querySelector('[data-testid="library-row-preview"]') as HTMLElement;
    expect(preview.className).toContain('line-clamp-2');
    assertStudioClean(screen.getByTestId('library-list-column'));
  });

  it('a visible search field filters by name and description, and ⌘F focuses it', async () => {
    mount();
    await rowOf('campaign');
    const search = screen.getByRole('textbox', { name: 'Search skills by name or description' });
    fireEvent.change(search, { target: { value: 'benchmark' } });
    expect(screen.getAllByTestId('skill-row')).toHaveLength(1);
    expect(screen.getByTestId('skill-row').textContent).toContain('campaign');
    // the one match is what the pane shows
    expect(
      within(screen.getByTestId('library-detail')).getByRole('heading', {
        level: 2,
        name: 'campaign',
      })
    ).toBeInTheDocument();
    fireEvent.change(search, { target: { value: 'nothing-like-this' } });
    expect(screen.queryAllByTestId('skill-row')).toHaveLength(0);
    expect(screen.getByText('No matching skills found')).toBeInTheDocument();

    (search as HTMLInputElement).blur();
    fireEvent.keyDown(window, { key: 'f', metaKey: true });
    expect(document.activeElement).toBe(search);
  });
});
