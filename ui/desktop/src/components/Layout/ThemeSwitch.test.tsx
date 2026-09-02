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
 * agree with: System is the default and follows the OS through the prefers-color-scheme listener;
 * Light/Dark set <html>'s class the way the app always has; either control moves the other.
 */

const os = vi.hoisted(() => ({
  dark: false,
  listeners: new Set<() => void>(),
}));

beforeEach(() => {
  os.dark = false;
  os.listeners.clear();
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: vi.fn((query: string) => ({
      get matches() {
        return os.dark;
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
  document.documentElement.className = '';
});

const flipOs = (dark: boolean) => {
  os.dark = dark;
  act(() => os.listeners.forEach((fn) => fn()));
};

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

  it('is a Studio control: icon segments with titles and screen-reader labels, no ban, every class compiles', async () => {
    mount();
    const group = screen.getByRole('radiogroup', { name: 'Theme' });
    expect(group.className).toContain('h-lz-row');
    const radios = screen.getAllByRole('radio');
    expect(radios.map((r) => r.getAttribute('title'))).toEqual(['System', 'Light', 'Dark']);
    expect(radios.every((r) => r.querySelector('svg') != null)).toBe(true);
    assertStudioClean(group);
    const classes = allClasses(group).filter((c) => !c.startsWith('lucide') && c !== 'no-drag');
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
