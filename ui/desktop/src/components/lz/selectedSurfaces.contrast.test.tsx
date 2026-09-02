import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import * as TabsPrimitive from '@radix-ui/react-tabs';
import { NavRow } from '../Layout/NavigationPanel';
import { Tabs, TabsList, TabsTrigger } from '../ui/tabs';
import { DataTable, Segmented, SURFACE } from './index';
import { contrast, resolvedPaint, studioToken } from './resolvedPaint';

/**
 * Every other surface with a SELECTED item, resolved through the compiled CSS in both themes:
 * the sidebar nav row, the Settings hub strip (lz Segmented in `tabs` mode worn by Radix
 * triggers), the host TabsTrigger, and DataTable's `selectedKey` row. Each selected element must
 * resolve to a solid fill whose ink contrasts, at rest AND under the pointer — a neutral hover
 * step that lands under white ink is the invisibility the Memories/Skills rows shipped with.
 */

const Icon = () => <svg data-testid="icon" />;
const THEMES = ['light', 'dark'] as const;
// Not Tailwind utilities, so they compile to nothing by design: `.no-drag` is the app's own rule
// (index.css); `ring-offset-background` is a shadcn leftover on ui/tabs — a no-op, reported.
const NOT_UTILITIES = new Set(['no-drag', 'ring-offset-background']);

async function expectSelectedReadable(el: HTMLElement, inheritBg?: (t: 'light' | 'dark') => string) {
  for (const theme of THEMES) {
    const inherit = inheritBg ? { bg: inheritBg(theme) } : undefined;
    const rest = await resolvedPaint(el, theme, { inherit });
    expect(rest.missing.filter((c) => !NOT_UTILITIES.has(c)), `${theme} rest`).toEqual([]);
    expect(rest.bg, `${theme} fill`).toMatch(/^#[0-9a-f]{6}$/);
    expect(rest.text, `${theme} ink`).toMatch(/^#[0-9a-f]{6}$/);
    expect(contrast(rest.bg, rest.text), `${theme} rest contrast`).toBeGreaterThan(4.5);
    const hovered = await resolvedPaint(el, theme, { hover: true, inherit });
    expect(contrast(hovered.bg, hovered.text), `${theme} hover contrast`).toBeGreaterThan(4.5);
  }
}

describe('selected surfaces resolve to a readable fill + ink in both themes, at rest and hovered', () => {
  it('the sidebar nav row (accent fill, accent ink; no neutral hover on the selected row)', async () => {
    render(
      <IntlProvider locale="en" messages={{}}>
        <NavRow item={{ key: 'skills', icon: Icon, label: 'Skills' } as never} active onClick={() => {}} />
      </IntlProvider>
    );
    const row = screen.getByRole('button');
    expect(row.className).not.toContain(SURFACE.hover);
    await expectSelectedReadable(row);
    const light = await resolvedPaint(row, 'light');
    expect(light.bg).toBe(studioToken('--color-lz-accent', 'light'));
  }, 30_000);

  it('the Settings hub strip — lz Segmented in tabs mode worn by Radix triggers', async () => {
    render(
      <TabsPrimitive.Root value="app">
        <TabsPrimitive.List asChild aria-label="Settings">
          <Segmented
            as="tabs"
            aria-label="Settings"
            value="app"
            onChange={() => {}}
            options={[
              { value: 'chat', label: 'Chat', icon: <Icon /> },
              { value: 'app', label: 'App', icon: <Icon /> },
            ]}
            renderOption={({ option, className, content }) => (
              <TabsPrimitive.Trigger value={option.value} className={className}>
                {content}
              </TabsPrimitive.Trigger>
            )}
          />
        </TabsPrimitive.List>
      </TabsPrimitive.Root>
    );
    const active = screen.getByRole('tab', { name: 'App' });
    expect(active.getAttribute('data-state')).toBe('active');
    expect(active.className).not.toContain(SURFACE.hover);
    await expectSelectedReadable(active);
    // The idle tab sits on the strip's surface; its hover is the neutral step, still readable.
    const idle = screen.getByRole('tab', { name: 'Chat' });
    await expectSelectedReadable(idle, (t) => studioToken('--color-lz-surface', t));
  }, 30_000);

  it('the host TabsTrigger (ui/tabs) — a solid host tint with primary ink, the same under hover', async () => {
    render(
      <Tabs value="b">
        <TabsList>
          <TabsTrigger value="a">Alpha</TabsTrigger>
          <TabsTrigger value="b">Beta</TabsTrigger>
        </TabsList>
      </Tabs>
    );
    const active = screen.getByRole('tab', { name: 'Beta' });
    expect(active.getAttribute('data-state')).toBe('active');
    await expectSelectedReadable(active);
  }, 30_000);

  it('DataTable selectedKey row', async () => {
    render(
      <DataTable
        aria-label="nodes"
        columns={[{ key: 'name', header: 'Name', cell: (r: { id: string }) => r.id }]}
        rows={[{ id: 'a' }, { id: 'b' }]}
        rowKey={(r) => r.id}
        selectedKey="b"
        onRowClick={() => {}}
        empty={<span>none</span>}
      />
    );
    const rows = screen.getAllByTestId('lz-row');
    const selected = rows.find((r) => r.getAttribute('aria-selected') === 'true') as HTMLElement;
    expect(selected).toBeDefined();
    expect(selected.className).not.toContain(SURFACE.hover);
    await expectSelectedReadable(selected);
  }, 30_000);
});
