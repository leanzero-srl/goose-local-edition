import { useEffect, useState } from 'react';

/**
 * How much of a swarm run the panel shows — a UI display preference (not a swarm behavior knob), so it lives
 * in localStorage, not the swarm config the CLI reads. Three tiers:
 *   - 'compact'   headline phases only (a few recent lines)
 *   - 'verbose'   the full timeline + per-task reasoning + tool calls (expand to read)
 *   - 'developer' everything expanded by default — all lanes open, all tool outputs open, full reasoning
 *                 uncapped, raw fields — for watching a build in dev.
 * A custom event keeps every mounted reader (the settings control + the run panel) in sync live.
 */

export type SwarmLogMode = 'compact' | 'verbose' | 'developer';
export const SWARM_LOG_MODES: SwarmLogMode[] = ['compact', 'verbose', 'developer'];

const KEY = 'goose.swarm.logMode';
const LEGACY_KEY = 'goose.swarm.verboseActivity'; // the old boolean flag
const EVT = 'goose-swarm-logmode-changed';

function read(): SwarmLogMode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === 'compact' || v === 'verbose' || v === 'developer') return v;
    // Migrate the old boolean: '0' -> compact, anything else (incl. unset) -> verbose (the default).
    const legacy = localStorage.getItem(LEGACY_KEY);
    return legacy === '0' ? 'compact' : 'verbose';
  } catch {
    return 'verbose';
  }
}

export function setSwarmLogMode(mode: SwarmLogMode): void {
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* storage unavailable — non-fatal */
  }
  window.dispatchEvent(new CustomEvent(EVT, { detail: mode }));
}

export function useSwarmLogMode(): [SwarmLogMode, (m: SwarmLogMode) => void] {
  const [mode, setLocal] = useState<SwarmLogMode>(read);
  useEffect(() => {
    const onChange = () => setLocal(read());
    window.addEventListener(EVT, onChange);
    window.addEventListener('storage', onChange);
    return () => {
      window.removeEventListener(EVT, onChange);
      window.removeEventListener('storage', onChange);
    };
  }, []);
  return [mode, setSwarmLogMode];
}
