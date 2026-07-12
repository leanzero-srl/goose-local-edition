import { useEffect, useState } from 'react';

/**
 * A UI display preference: show the swarm run panel VERBOSE (every dispatch, judge verdict + hint,
 * pre-review, completion, smoke result, full per-task tool calls) vs. compact (headline phases only).
 * Stored in localStorage — it's a pure view preference, not a swarm behavior knob, so it does NOT go in
 * the swarm config the CLI reads. A custom event keeps every mounted reader (settings toggle + the run
 * panel) in sync live, without a full config round-trip.
 */

const KEY = 'goose.swarm.verboseActivity';
const EVT = 'goose-swarm-verbose-changed';

function read(): boolean {
  // Default ON: the swarm run panel should show the full picture out of the box. An explicit choice to go
  // Compact ('0') is respected; only an absent preference defaults to verbose.
  try {
    const v = localStorage.getItem(KEY);
    return v === null ? true : v === '1';
  } catch {
    return true;
  }
}

export function setVerboseSwarm(v: boolean): void {
  try {
    localStorage.setItem(KEY, v ? '1' : '0');
  } catch {
    /* storage unavailable — non-fatal */
  }
  window.dispatchEvent(new CustomEvent(EVT, { detail: v }));
}

export function useVerboseSwarm(): [boolean, (v: boolean) => void] {
  const [verbose, setLocal] = useState(read);
  useEffect(() => {
    const onChange = () => setLocal(read());
    window.addEventListener(EVT, onChange);
    window.addEventListener('storage', onChange);
    return () => {
      window.removeEventListener(EVT, onChange);
      window.removeEventListener('storage', onChange);
    };
  }, []);
  return [verbose, setVerboseSwarm];
}
