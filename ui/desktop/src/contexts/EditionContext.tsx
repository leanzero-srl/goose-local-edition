import React, { createContext, useContext, useEffect, useState, useCallback } from 'react';

/**
 * Goose Local Edition state.
 *
 * The `edition` (`standard` | `local`) selects the "goose" vs "Goose Local Edition" UX skin — a presentation
 * choice only, never gating capability. It is persisted through the settings bridge and mirrored in
 * `localStorage` so the document class can be stamped synchronously before first paint (no flash), exactly
 * like the theme. Local Edition adds the `.local-edition` class on the document root, which the CSS theme
 * overrides key off.
 */

export type Edition = 'standard' | 'local';

interface EditionContextValue {
  edition: Edition;
  setEdition: (edition: Edition) => void;
  isLocal: boolean;
}

const EditionContext = createContext<EditionContextValue | null>(null);

const LOCAL_STORAGE_KEY = 'edition';
const LOCAL_EDITION_CLASS = 'local-edition';

/** Read the cached edition synchronously (for a pre-paint stamp that avoids a one-frame flash). */
export function getCachedEdition(): Edition {
  try {
    return localStorage.getItem(LOCAL_STORAGE_KEY) === 'local' ? 'local' : 'standard';
  } catch {
    return 'standard';
  }
}

/** Toggle the `.local-edition` class on the document root. */
export function applyEditionToDocument(edition: Edition): void {
  const root = document.documentElement;
  if (edition === 'local') {
    root.classList.add(LOCAL_EDITION_CLASS);
  } else {
    root.classList.remove(LOCAL_EDITION_CLASS);
  }
}

interface EditionProviderProps {
  children: React.ReactNode;
}

export function EditionProvider({ children }: EditionProviderProps) {
  // Start from the synchronous cache so the React state matches the pre-paint stamp.
  const [edition, setEditionState] = useState<Edition>(getCachedEdition);

  // Keep the document class in sync with state.
  useEffect(() => {
    applyEditionToDocument(edition);
  }, [edition]);

  // Load the authoritative persisted edition once.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const saved = await window.electron.getSetting('edition');
        if (!cancelled && (saved === 'local' || saved === 'standard')) {
          setEditionState(saved);
          try {
            localStorage.setItem(LOCAL_STORAGE_KEY, saved);
          } catch {
            // localStorage may be unavailable; the setting remains authoritative.
          }
        }
      } catch (error) {
        console.warn('[EditionContext] Failed to load edition setting:', error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const setEdition = useCallback((next: Edition) => {
    setEditionState(next);
    applyEditionToDocument(next);
    try {
      localStorage.setItem(LOCAL_STORAGE_KEY, next);
    } catch {
      // ignore — the setting write below is authoritative
    }
    window.electron.setSetting('edition', next).catch((error: unknown) => {
      console.warn('[EditionContext] Failed to save edition setting:', error);
    });
  }, []);

  return (
    <EditionContext.Provider value={{ edition, setEdition, isLocal: edition === 'local' }}>
      {children}
    </EditionContext.Provider>
  );
}

export function useEdition(): EditionContextValue {
  const ctx = useContext(EditionContext);
  if (!ctx) {
    throw new Error('useEdition must be used within an EditionProvider');
  }
  return ctx;
}
