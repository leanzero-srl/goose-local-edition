import { useContext, useEffect, useMemo, useState } from 'react';
import { UNSAFE_LocationContext } from 'react-router-dom';

/** The query part of the HashRouter location (`#/benchmark?run=…` → `run`), read without a router
 *  context so a view stays renderable on its own (tests mount views bare). */
export function parseHashQuery(hash: string): URLSearchParams {
  const q = hash.indexOf('?');
  return new URLSearchParams(q >= 0 ? hash.slice(q + 1) : '');
}

/**
 * The current route's query. Inside the app's router it follows the ROUTER's location: the sidebar
 * navigates with `navigate()`, which is a history.pushState — no hashchange and no popstate fire, so a
 * window-event reader kept the previous run on screen after a history row was clicked (measured
 * 2026-10-03: the URL said ?run=B, the panel showed A until reload). Outside a router (bare test
 * mounts) it reads window.location.hash and follows hashchange/popstate as before.
 */
export function useHashQuery(): URLSearchParams {
  // The context's default is null outside a router; useContext is unconditional, so hook order holds.
  const routed = useContext(UNSAFE_LocationContext) as { location?: { search?: string } } | null;
  const routedSearch = routed?.location ? (routed.location.search ?? '') : null;
  const [params, setParams] = useState(() => parseHashQuery(window.location.hash));
  useEffect(() => {
    const read = () => setParams(parseHashQuery(window.location.hash));
    window.addEventListener('hashchange', read);
    window.addEventListener('popstate', read);
    return () => {
      window.removeEventListener('hashchange', read);
      window.removeEventListener('popstate', read);
    };
  }, []);
  return useMemo(
    () => (routedSearch != null ? new URLSearchParams(routedSearch) : params),
    [routedSearch, params]
  );
}
