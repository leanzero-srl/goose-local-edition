import React, { createContext, useContext, useEffect, useRef, useState, useCallback } from 'react';
import { applyThemeTokens, buildMcpHostStyles } from '../theme/theme-tokens';
import type { McpUiHostStyles } from '@modelcontextprotocol/ext-apps/app-bridge';

export type ThemePreference = 'light' | 'dark' | 'system';
type ResolvedTheme = 'light' | 'dark';

interface ThemeContextValue {
  userThemePreference: ThemePreference;
  setUserThemePreference: (pref: ThemePreference) => void;
  resolvedTheme: ResolvedTheme;
  mcpHostStyles: McpUiHostStyles;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function applyThemeToDocument(theme: ResolvedTheme): void {
  const toRemove = theme === 'dark' ? 'light' : 'dark';
  document.documentElement.classList.add(theme);
  document.documentElement.classList.remove(toRemove);
  document.documentElement.style.colorScheme = theme;
}

// Built once — light-dark() values are theme-independent
const mcpHostStyles = buildMcpHostStyles();

interface ThemeProviderProps {
  children: React.ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  // Start with light theme to avoid flash, will update once settings load
  const [userThemePreference, setUserThemePreferenceState] = useState<ThemePreference>('light');
  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>('light');
  const preferenceRef = useRef<ThemePreference>('light');

  // One motion for every way a preference arrives (settings load, a click, another window): paint
  // it, push it to main (nativeTheme.themeSource = preference), and under 'system' paint main's
  // answer — nativeTheme.shouldUseDarkColors — over the renderer's guess. A fixed Light/Dark is
  // final on the spot, exactly as before.
  //
  // Under 'system' the renderer's prefers-color-scheme is NEVER read: it was measured stale twice
  // (2026-09-02, and Q-300 on 3.0.68 — OS Dark, System chosen, matchMedia false; clicking System
  // painted dark, a viewport change painted light again through a matchMedia 'change' listener).
  // The paint holds until main answers.
  const applyPreference = useCallback((preference: ThemePreference) => {
    preferenceRef.current = preference;
    setUserThemePreferenceState(preference);
    if (preference !== 'system') setResolvedTheme(preference);
    void (async () => {
      try {
        const { dark } = await window.electron.setThemeSource(preference);
        if (preference === 'system' && preferenceRef.current === 'system') {
          setResolvedTheme(dark ? 'dark' : 'light');
        }
      } catch (error) {
        console.warn('[ThemeContext] theme-set failed:', error);
      }
    })();
  }, []);

  useEffect(() => {
    async function loadThemeFromSettings() {
      try {
        const [useSystemTheme, savedTheme] = await Promise.all([
          window.electron.getSetting('useSystemTheme'),
          window.electron.getSetting('theme'),
        ]);
        applyPreference(useSystemTheme ? 'system' : savedTheme);
      } catch (error) {
        console.warn('[ThemeContext] Failed to load theme settings:', error);
      }
    }

    loadThemeFromSettings();
  }, [applyPreference]);

  const setUserThemePreference = useCallback(
    async (preference: ThemePreference) => {
      applyPreference(preference);

      // Save to settings
      try {
        if (preference === 'system') {
          await window.electron.setSetting('useSystemTheme', true);
        } else {
          await window.electron.setSetting('useSystemTheme', false);
          await window.electron.setSetting('theme', preference);
        }
      } catch (error) {
        console.warn('[ThemeContext] Failed to save theme settings:', error);
      }

      // Broadcast to other windows via Electron. Under 'system' each window asks main itself, so
      // the payload carries the preference, never a resolved guess.
      window.electron?.broadcastThemeChange({
        mode: preference,
        useSystemTheme: preference === 'system',
        theme: preference,
      });
    },
    [applyPreference]
  );

  // Main's nativeTheme 'updated' event: the OS flipped (or another window set the source). Under
  // 'system' it is the theme; under a fixed choice the choice stands.
  useEffect(() => {
    if (!window.electron) return;
    return window.electron.onNativeThemeUpdated((dark) => {
      if (preferenceRef.current !== 'system') return;
      setResolvedTheme(dark ? 'dark' : 'light');
    });
  }, []);

  // Listen for theme changes from other windows (via Electron IPC)
  useEffect(() => {
    if (!window.electron) return;

    const handleThemeChanged = (_event: unknown, ...args: unknown[]) => {
      const themeData = args[0] as { useSystemTheme: boolean; theme: string };
      const newPreference: ThemePreference = themeData.useSystemTheme
        ? 'system'
        : themeData.theme === 'dark'
          ? 'dark'
          : 'light';

      applyPreference(newPreference);

      // Save to settings (don't await, fire and forget)
      if (newPreference === 'system') {
        window.electron.setSetting('useSystemTheme', true);
      } else {
        window.electron.setSetting('useSystemTheme', false);
        window.electron.setSetting('theme', newPreference);
      }
    };

    window.electron.on('theme-changed', handleThemeChanged);
    return () => {
      window.electron.off('theme-changed', handleThemeChanged);
    };
  }, [applyPreference]);

  // Apply theme class and CSS tokens whenever resolvedTheme changes
  useEffect(() => {
    applyThemeToDocument(resolvedTheme);
    applyThemeTokens(resolvedTheme);
  }, [resolvedTheme]);

  const value: ThemeContextValue = {
    userThemePreference,
    setUserThemePreference,
    resolvedTheme,
    mcpHostStyles,
  };

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}

/**
 * The resolved theme for a component that also renders outside the provider (unit renders): the
 * provider's answer when there is one, else the class the provider paints on <html>.
 */
export function useResolvedTheme(): ResolvedTheme {
  const context = useContext(ThemeContext);
  if (context) return context.resolvedTheme;
  return document.documentElement.classList.contains('dark') ? 'dark' : 'light';
}
