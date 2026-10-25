import { useEffect, useState } from 'react';
import { acpReadConfig } from '../../acp/config';
import { MEMFREE_ENABLED_KEY, memfreeEnabledFrom } from './memfree';

let cached: Promise<boolean> | null = null;

/** Read GOOSE_MEMFREE_ENABLED once per app session (every card asks; one config read answers all). */
function readEnabled(): Promise<boolean> {
  if (!cached) {
    cached = Promise.resolve()
      .then(() => acpReadConfig(MEMFREE_ENABLED_KEY, false))
      .then(memfreeEnabledFrom, () => true);
  }
  return cached;
}

/** Forget the cached reading (Settings › App changed it). */
export function forgetMemfreeEnabled(): void {
  cached = null;
}

/**
 * Whether the "Free up memory" entry is offered: on by default (a failed read is on too, the engine
 * keeps the final say). false until the reading lands so nothing flashes in and out.
 */
export function useMemfreeEnabled(): boolean {
  const [enabled, setEnabled] = useState(false);
  useEffect(() => {
    let alive = true;
    void readEnabled().then((v) => {
      if (alive) setEnabled(v);
    });
    return () => {
      alive = false;
    };
  }, []);
  return enabled;
}
