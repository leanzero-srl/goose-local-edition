import React, { createContext, useContext, useEffect, useState, useCallback } from 'react';

/**
 * Goose Swarm edition state.
 *
 * The `edition` (`standard` | `local`) selects the "goose" vs "Goose Swarm" UX skin — a presentation
 * choice only, never gating capability. The VALUE stays 'local' for storage compatibility; only the
 * display brand changed ("Goose Local Edition" → "Goose Swarm"). It is persisted through the settings
 * bridge and mirrored in `localStorage` so the document class can be stamped synchronously before first
 * paint (no flash), exactly like the theme. The local edition adds the `.local-edition` class on the
 * document root, which the CSS theme overrides key off.
 *
 * Resolution mirrors the Rust resolver (crates/goose-cli/src/edition.rs), highest first:
 *   1. an explicitly persisted `edition` setting,
 *   2. a DERIVED default — a local/swarm provider implies the local edition, so the skin is *true*
 *      rather than a badge divorced from locality.
 * (The CLI's `--local` flag and GOOSE_LOCAL_EDITION env layers have no desktop equivalent.)
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

/** Window-title brand per edition. Only these exact titles are ever swapped, so a window that set its
 *  own title (e.g. a standalone app window) is never clobbered. */
const BRAND_TITLE: Record<Edition, string> = { standard: 'Goose', local: 'Goose Swarm' };

/** Provider-name fragments that are inherently local — their presence derives the local edition.
 *  Mirrors LOCAL_PROVIDER_FRAGMENTS in crates/goose-cli/src/edition.rs. */
export const LOCAL_PROVIDER_FRAGMENTS = [
  'lmstudio',
  'ollama',
  'swarm',
  'llama',
  'localai',
  'mlx',
] as const;

/** True if a provider name reads as a local/swarm backend (Rust: `provider_is_local`). */
export function providerIsLocal(provider: string): boolean {
  const p = provider.toLowerCase();
  return LOCAL_PROVIDER_FRAGMENTS.some((frag) => p.includes(frag));
}

/**
 * PURE resolver — all inputs supplied — mirroring the Rust `resolve_edition_from` precedence
 * (sans CLI flag / env var, which the desktop does not have): a recognized persisted value wins,
 * an unrecognized one falls through to provider derivation, and the default is standard.
 */
export function resolveEdition(
  persisted: unknown,
  activeProvider: string | null | undefined
): Edition {
  if (typeof persisted === 'string') {
    const p = persisted.trim().toLowerCase();
    if (p === 'local') return 'local';
    if (p === 'standard') return 'standard';
  }
  if (activeProvider && providerIsLocal(activeProvider)) {
    return 'local';
  }
  return 'standard';
}

/** Read the cached edition synchronously (for a pre-paint stamp that avoids a one-frame flash). */
export function getCachedEdition(): Edition {
  try {
    return localStorage.getItem(LOCAL_STORAGE_KEY) === 'local' ? 'local' : 'standard';
  } catch {
    return 'standard';
  }
}

/** Stamp the edition on the document: the `.local-edition` root class and the brand window title. */
export function applyEditionToDocument(edition: Edition): void {
  const root = document.documentElement;
  if (edition === 'local') {
    root.classList.add(LOCAL_EDITION_CLASS);
  } else {
    root.classList.remove(LOCAL_EDITION_CLASS);
  }
  // Swap only between the two brand titles (or the empty boot title) — never overwrite a custom one.
  const title = document.title;
  if (title === '' || title === BRAND_TITLE.standard || title === BRAND_TITLE.local) {
    document.title = BRAND_TITLE[edition];
  }
}

function cacheEdition(edition: Edition): void {
  try {
    localStorage.setItem(LOCAL_STORAGE_KEY, edition);
  } catch {
    // localStorage may be unavailable; the async sources remain authoritative.
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

  // Resolve the authoritative edition once: explicit persisted setting first, otherwise derive it
  // from the active provider (the same config source the rest of the app reads).
  useEffect(() => {
    let cancelled = false;
    (async () => {
      let saved: unknown;
      try {
        saved = await window.electron.getSetting('edition');
      } catch (error) {
        console.warn('[EditionContext] Failed to load edition setting:', error);
        return;
      }
      if (saved === 'local' || saved === 'standard') {
        if (!cancelled) {
          setEditionState(saved);
          cacheEdition(saved);
        }
        return;
      }
      // No explicit choice stored — derive from the active provider, exactly like the Rust resolver.
      // Dynamic import: renderer.tsx imports this module for the pre-paint stamp, and a static
      // acp/config import would pull the ACP SDK into the boot chunk.
      let provider: unknown = null;
      try {
        const { acpReadConfig } = await import('../acp/config');
        provider = await acpReadConfig('GOOSE_PROVIDER', false);
      } catch (error) {
        console.warn('[EditionContext] Failed to read active provider for edition derivation:', error);
      }
      const derived = resolveEdition(saved, typeof provider === 'string' ? provider : null);
      if (!cancelled) {
        setEditionState(derived);
        cacheEdition(derived);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const setEdition = useCallback((next: Edition) => {
    setEditionState(next);
    applyEditionToDocument(next);
    cacheEdition(next);
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
