import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import { ThemeProvider } from '../../contexts/ThemeContext';
import ThemeSelector from '../GooseSidebar/ThemeSelector';
import { ThemeSwitch } from './ThemeSwitch';
import { assertStudioClean } from '../lz/assertStudioClean';
import { allClasses } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * The sidebar theme switch over the REAL ThemeProvider, beside the Settings › App buttons it must
 * agree with: System is the default and paints what MAIN reports (nativeTheme.shouldUseDarkColors,
 * pushed through `theme-set` and re-resolved on nativeTheme's 'updated' event); Light/Dark set
 * <html>'s class the way the app always has; either control moves the other.
 *
 * The OS model: `os.dark` is what main sees; `os.rendererDark`, when set, pins the renderer's own
 * prefers-color-scheme to a different value — the stale state measured on 2026-09-02.
 */

const os = vi.hoisted(() => ({
  dark: false,
  rendererDark: null as boolean | null,
  source: 'system' as 'system' | 'light' | 'dark',
  listeners: new Set<() => void>(),
  nativeListeners: new Set<(dark: boolean) => void>(),
}));

const mainReports = () => (os.source === 'system' ? os.dark : os.source === 'dark');

beforeEach(() => {
  os.dark = false;
  os.rendererDark = null;
  os.source = 'system';
  os.listeners.clear();
  os.nativeListeners.clear();
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: vi.fn((query: string) => ({
      get matches() {
        return os.rendererDark ?? os.dark;
      },
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: (_: string, fn: () => void) => os.listeners.add(fn),
      removeEventListener: (_: string, fn: () => void) => os.listeners.delete(fn),
      dispatchEvent: vi.fn(),
    })),
  });
  const e = (window as unknown as { electron: Record<string, unknown> }).electron;
  e.broadcastThemeChange = vi.fn();
  e.getSetting = vi.fn(async (key: string) =>
    key === 'useSystemTheme' ? true : key === 'theme' ? 'light' : undefined
  );
  e.setSetting = vi.fn(async () => undefined);
  e.setThemeSource = vi.fn(async (preference: 'system' | 'light' | 'dark') => {
    os.source = preference;
    return { dark: mainReports() };
  });
  e.onNativeThemeUpdated = vi.fn((cb: (dark: boolean) => void) => {
    os.nativeListeners.add(cb);
    return () => os.nativeListeners.delete(cb);
  });
  document.documentElement.className = '';
});

const electron = () =>
  (window as unknown as { electron: { setThemeSource: ReturnType<typeof vi.fn> } }).electron;

const flipOs = (dark: boolean) => {
  os.dark = dark;
  act(() => {
    os.listeners.forEach((fn) => fn());
    os.nativeListeners.forEach((fn) => fn(mainReports()));
  });
};

const nativeUpdated = (dark: boolean) => act(() => os.nativeListeners.forEach((fn) => fn(dark)));

const mount = () =>
  render(
    <IntlProvider locale="en" messages={{}}>
      <ThemeProvider>
        <ThemeSwitch />
        <ThemeSelector hideTitle horizontal />
      </ThemeProvider>
    </IntlProvider>
  );

const radio = (name: 'System' | 'Light' | 'Dark') => screen.getByRole('radio', { name });
const html = () => document.documentElement;

describe('ThemeSwitch — one store, two controls', () => {
  it('defaults to System once settings load, and <html> carries the OS theme', async () => {
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    expect(html().classList.contains('light')).toBe(true);
    expect(html().classList.contains('dark')).toBe(false);
    expect(html().style.colorScheme).toBe('light');
  });

  it('Light and Dark set the document class as before and persist through the same settings', async () => {
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    const setSetting = (window as unknown as { electron: { setSetting: ReturnType<typeof vi.fn> } })
      .electron.setSetting;

    fireEvent.click(radio('Dark'));
    expect(radio('Dark').getAttribute('aria-checked')).toBe('true');
    expect(html().classList.contains('dark')).toBe(true);
    expect(html().classList.contains('light')).toBe(false);
    expect(html().style.colorScheme).toBe('dark');
    await waitFor(() => expect(setSetting).toHaveBeenCalledWith('theme', 'dark'));
    expect(setSetting).toHaveBeenCalledWith('useSystemTheme', false);

    fireEvent.click(radio('Light'));
    expect(html().classList.contains('light')).toBe(true);
    expect(html().classList.contains('dark')).toBe(false);
    await waitFor(() => expect(setSetting).toHaveBeenCalledWith('theme', 'light'));
  });

  it('System follows the OS through the prefers-color-scheme listener, and stops once a fixed theme is chosen', async () => {
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    await waitFor(() => expect(os.listeners.size).toBeGreaterThan(0));
    flipOs(true);
    expect(html().classList.contains('dark')).toBe(true);
    flipOs(false);
    expect(html().classList.contains('light')).toBe(true);

    fireEvent.click(radio('Dark'));
    await waitFor(() => expect(os.listeners.size).toBe(0));
    flipOs(false);
    expect(html().classList.contains('dark')).toBe(true);
  });

  it('the Settings › App buttons and the sidebar switch stay in sync both ways', async () => {
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    fireEvent.click(screen.getByTestId('dark-mode-button'));
    expect(radio('Dark').getAttribute('aria-checked')).toBe('true');
    expect(radio('System').getAttribute('aria-checked')).toBe('false');
    expect(html().classList.contains('dark')).toBe(true);

    fireEvent.click(radio('Light'));
    expect(screen.getByTestId('light-mode-button').className).toContain('bg-background-inverse');
    expect(screen.getByTestId('dark-mode-button').className).not.toContain('bg-background-inverse');
    expect(html().classList.contains('light')).toBe(true);
  });

  it('pushes theme-set to main with the chosen value — on load and on every click', async () => {
    mount();
    await waitFor(() => expect(electron().setThemeSource).toHaveBeenCalledWith('system'));
    fireEvent.click(radio('Dark'));
    expect(electron().setThemeSource).toHaveBeenLastCalledWith('dark');
    fireEvent.click(radio('Light'));
    expect(electron().setThemeSource).toHaveBeenLastCalledWith('light');
    fireEvent.click(screen.getByTestId('system-mode-button'));
    expect(electron().setThemeSource).toHaveBeenLastCalledWith('system');
    expect(electron().setThemeSource).toHaveBeenCalledTimes(4);
  });

  it("System paints DARK from main's shouldUseDarkColors when the renderer's own prefers-color-scheme is stale light (measured 2026-09-02)", async () => {
    os.dark = true;
    os.rendererDark = false;
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    expect(window.matchMedia('(prefers-color-scheme: dark)').matches).toBe(false);
    await waitFor(() => expect(html().classList.contains('dark')).toBe(true));
    expect(html().classList.contains('light')).toBe(false);
    expect(html().style.colorScheme).toBe('dark');
  });

  it("re-resolves on nativeTheme's 'updated' event under System, and a fixed choice ignores it", async () => {
    mount();
    await waitFor(() => expect(radio('System').getAttribute('aria-checked')).toBe('true'));
    await waitFor(() => expect(os.nativeListeners.size).toBeGreaterThan(0));
    nativeUpdated(true);
    expect(html().classList.contains('dark')).toBe(true);
    nativeUpdated(false);
    expect(html().classList.contains('light')).toBe(true);

    fireEvent.click(radio('Dark'));
    nativeUpdated(false);
    expect(html().classList.contains('dark')).toBe(true);
    expect(html().classList.contains('light')).toBe(false);
  });

  it('is a Studio control: icon segments with titles and screen-reader labels, no ban, every class compiles', async () => {
    mount();
    const group = screen.getByRole('radiogroup', { name: 'Theme' });
    expect(group.className).toContain('h-lz-row');
    expect(group.className).toContain('w-full');
    expect(group.className).toContain('[&>button]:flex-1');
    const radios = screen.getAllByRole('radio');
    expect(radios.map((r) => r.getAttribute('title'))).toEqual(['System', 'Light', 'Dark']);
    expect(radios.every((r) => r.querySelector('svg') != null)).toBe(true);
    assertStudioClean(group);
    const classes = allClasses(group).filter((c) => !c.startsWith('lucide') && c !== 'no-drag');
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
