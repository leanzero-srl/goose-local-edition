import React from 'react';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import SettingsView from './SettingsView';

/** lucide stamps `lucide lucide-<name>` identifiers on its svgs — names, not utilities. */
const utilitiesOf = (classes: string[]) => classes.filter((c) => !c.startsWith('lucide'));

/**
 * LeanZero Studio harmonization of the settings SHELL: a PageHeader, the tab strip in the
 * Segmented register (accent fill on the active tab, solid hover on the rest) over Radix tabs
 * that keep their semantics and test ids, and a zone SectionHeader over each tab's content. The
 * upstream sections are not this surface's — mocked to markers.
 */

vi.mock('./import/ImportView', () => ({ default: () => <div data-testid="import-body" /> }));
vi.mock('./chat/ChatSettingsSection', () => ({ default: () => <div data-testid="chat-body" /> }));
vi.mock('./app/ExternalBackendSection', () => ({ default: () => <div /> }));
vi.mock('./PromptsSettingsSection', () => ({ default: () => <div /> }));
vi.mock('./keyboard/KeyboardShortcutsSection', () => ({ default: () => <div /> }));
vi.mock('./auth/AuthSettingsSection', () => ({ default: () => <div /> }));
vi.mock('./app/AppSettingsSection', () => ({ default: () => <div data-testid="app-body" /> }));
vi.mock('./config/ConfigSettings', () => ({ default: () => <div /> }));
vi.mock('../Layout/MainPanelLayout', () => ({
  MainPanelLayout: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));
vi.mock('../../utils/analytics', () => ({ trackSettingsTabViewed: vi.fn() }));
vi.mock('../../contexts/EditionContext', () => ({
  useEdition: () => ({ edition: 'standard', setEdition: vi.fn(), isLocal: false }),
}));

const renderSettings = () =>
  render(
    <IntlTestWrapper>
      <SettingsView onClose={() => {}} setView={() => {}} viewOptions={{}} />
    </IntlTestWrapper>
  );

describe('SettingsView — LeanZero Studio shell', () => {
  it('is a PageHeader, a segmented tab strip with the accent on the active tab, and a zone header per tab', () => {
    renderSettings();
    const header = screen.getByTestId('lz-page-header');
    expect(within(header).getByRole('heading', { level: 1 }).textContent).toBe('Settings');

    const strip = screen.getByRole('tablist');
    expect(strip.className).toContain('rounded-lz-control');
    const chat = screen.getByTestId('settings-chat-tab');
    expect(chat.getAttribute('aria-selected')).toBe('true');
    expect(chat.className).toContain('bg-lz-accent');
    for (const tab of ['sharing', 'prompts', 'keyboard', 'auth', 'app']) {
      const el = screen.getByTestId(`settings-${tab}-tab`);
      expect(el.className).not.toContain('bg-lz-accent');
      expect(el.className).toContain('hover:bg-lz-surface-2');
    }
    const zone = screen.getAllByTestId('lz-section-header').find((z) => z.textContent === 'Chat');
    expect(zone).toBeTruthy();
    expect(screen.getByTestId('chat-body')).toBeTruthy();
  });

  it('moves the accent with the active tab and mounts that tab body', () => {
    renderSettings();
    fireEvent.mouseDown(screen.getByTestId('settings-app-tab'), { button: 0 });
    const app = screen.getByTestId('settings-app-tab');
    expect(app.getAttribute('data-state')).toBe('active');
    expect(app.className).toContain('bg-lz-accent');
    expect(screen.getByTestId('settings-chat-tab').className).not.toContain('bg-lz-accent');
    expect(screen.getByTestId('app-body')).toBeTruthy();
    expect(screen.queryByTestId('chat-body')).toBeNull();
    expect(screen.getAllByTestId('lz-section-header').some((z) => z.textContent === 'App')).toBe(
      true
    );
  });

  it('emits no banned pattern and only utilities the pipeline compiles', async () => {
    const { container } = renderSettings();
    assertStudioClean(container);
    expect(container.innerHTML).not.toMatch(/page-transition|color-node-/);
    const missing = await missingUtilities(utilitiesOf(allClasses(container)));
    expect(missing).toEqual([]);
  }, 30_000);
});
