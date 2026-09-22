import { useEffect, useState } from 'react';

/** The query part of the HashRouter location (`#/benchmark?run=…` → `run`), read without a router
 *  context so a view stays renderable on its own (tests mount views bare). */
export function parseHashQuery(hash: string): URLSearchParams {
  const q = hash.indexOf('?');
  return new URLSearchParams(q >= 0 ? hash.slice(q + 1) : '');
}

export function useHashQuery(): URLSearchParams {
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
  return params;
}
