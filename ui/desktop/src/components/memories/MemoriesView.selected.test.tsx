import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import MemoriesView from './MemoriesView';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { SURFACE } from '../lz';
import { assertStudioClean } from '../lz/assertStudioClean';
import { contrast, resolveExpr, resolvedPaint, studioToken } from '../lz/resolvedPaint';

/**
 * The owner's case (2026-09-02, packaged build, light theme): "on selection an item becomes
 * invisible". MEASURED on the old row through the compiled CSS: `bg-background-accent` produced
 * NO rule (the token is registered nowhere in main.css) and twMerge had already dropped the
 * Card's own fill for it, so the selected row painted `text-white` over nothing — white on the
 * white page, contrast 1:1. This fixture drives the real view: light theme → click a memory →
 * the row's resolved fill is the accent and its ink contrasts, at rest and under the pointer.
 */

vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));

const memories = [
  {
    id: 'm1',
    category: 'absorb-human-voice',
    scope: 'global',
    tags: ['user:voice'],
    content: '# voice\nWrite as he does.',
    updatedAt: 1,
  },
  {
    id: 'm2',
    category: 'swarm-end-goal',
    scope: 'local',
    tags: ['project:swarm'],
    content: 'Functional apps on local models.',
    updatedAt: 2,
  },
];

const mount = () => {
  (window as unknown as { electron: Record<string, unknown> }).electron.listMemories = vi.fn(
    async () => memories
  );
  return render(
    <IntlTestWrapper>
      <MemoriesView />
    </IntlTestWrapper>
  );
};

const rowOf = async (title: string) =>
  (await screen.findByText(title, {}, { timeout: 3000 })).closest('button') as HTMLElement;

describe('MemoriesView — a selected memory is visible in both themes', () => {
  it('light theme → click a memory → accent fill with accent ink; hover deepens the fill, never a neutral step', async () => {
    mount();
    const row = await rowOf('Absorb Human Voice');
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

      // The one-line snippet under the title sets its own ink: it must read on the accent too.
      const snippet = row.querySelector('.line-clamp-1') as HTMLElement;
      const snippetPaint = await resolvedPaint(snippet, theme, {
        inherit: { bg: rest.bg ?? undefined, text: rest.text ?? undefined },
      });
      expect(snippetPaint.missing).toEqual([]);
      expect(snippetPaint.text).toMatch(/^#[0-9a-f]{6}$/);
      expect(contrast(snippetPaint.bg, snippetPaint.text)).toBeGreaterThan(4.5);
    }
  }, 30_000);

  it('the idle rows read as ink on the page, and the neutral hover step keeps them readable', async () => {
    mount();
    const idle = await rowOf('Swarm End Goal');
    expect(idle.getAttribute('aria-current')).toBeNull();
    expect(idle.className).not.toContain('bg-lz-accent');
    for (const theme of ['light', 'dark'] as const) {
      const page = resolveExpr('var(--color-background-primary)', theme);
      const rest = await resolvedPaint(idle, theme, { inherit: { bg: page } });
      expect(rest.missing).toEqual([]);
      expect(contrast(rest.bg, rest.text)).toBeGreaterThan(4.5);
      const hovered = await resolvedPaint(idle, theme, { hover: true, inherit: { bg: page } });
      expect(hovered.bg).toBe(studioToken('--color-lz-surface-2', theme));
      expect(contrast(hovered.bg, hovered.text)).toBeGreaterThan(4.5);
      const snippet = idle.querySelector('.line-clamp-1') as HTMLElement;
      const snippetPaint = await resolvedPaint(snippet, theme, { inherit: { bg: rest.bg ?? undefined } });
      expect(contrast(snippetPaint.bg, snippetPaint.text)).toBeGreaterThan(4.5);
    }
  }, 30_000);

  it('selecting moves the accent: the previous row returns to idle, and the list carries no ban', async () => {
    mount();
    const first = await rowOf('Absorb Human Voice');
    const second = await rowOf('Swarm End Goal');
    fireEvent.click(first);
    fireEvent.click(second);
    expect(first.getAttribute('aria-current')).toBeNull();
    expect(first.className).not.toContain('bg-lz-accent');
    expect(second.getAttribute('aria-current')).toBe('true');
    expect(second.className).toContain('bg-lz-accent');
    assertStudioClean(first.parentElement as HTMLElement);
  }, 30_000);
});
