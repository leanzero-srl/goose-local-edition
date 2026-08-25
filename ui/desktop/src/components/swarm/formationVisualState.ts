import { useEffect, useState } from 'react';

export const FORMATION_PHASES = [
  {
    label: 'Research',
    tip: 'Scouts research libraries, architecture, and edge cases before code is written.',
  },
  {
    label: 'Plan',
    tip: 'The planner chooses a task breakdown and scores confidence in the decomposition.',
  },
  {
    label: 'Contracts',
    tip: 'Interface stubs align modules that different nodes will build in parallel.',
  },
  { label: 'Build', tip: 'Worker nodes build the planned tasks across the fleet.' },
  {
    label: 'Verify',
    tip: 'Integration, review, and an end-to-end command check the finished program.',
  },
  { label: 'Done', tip: 'The run has finished.' },
] as const;

export type FormationPhaseState = 'complete' | 'active' | 'upcoming';

export const FORMATION_RAMP = [
  'var(--color-node-1, #17c4c4)',
  'var(--color-node-2, #2e8bff)',
  'var(--color-node-3, #6a5cff)',
  'var(--color-node-4, #b14cff)',
  'var(--color-node-5, #ff3ea5)',
  'var(--color-node-6, #ff5c7a)',
] as const;

export const SWARM_STATUS = {
  running: 'var(--color-status-warn, #9a4d00)',
  done: 'var(--color-status-ok, #087a47)',
  error: 'var(--color-status-err, #c7271a)',
  action: 'var(--color-action-solid, #0b5bd3)',
  stopped: 'var(--color-status-stopped, #4b5563)',
  solidRunning: 'var(--color-status-warn-solid, #9a4d00)',
  solidDone: 'var(--color-status-ok-solid, #087a47)',
  solidError: 'var(--color-status-err-solid, #c7271a)',
  solidStopped: 'var(--color-status-stopped-solid, #4b5563)',
} as const;

export function phaseStepIndex(phase: string): number {
  const normalized = phase.toLowerCase();
  if (/done|finished|complete/.test(normalized)) return 5;
  if (/verif|integrat/.test(normalized)) return 4;
  if (/contract/.test(normalized)) return 2;
  if (/build|execut|dispatch|working/.test(normalized)) return 3;
  if (/plan/.test(normalized)) return 1;
  if (/research|scout|start/.test(normalized)) return 0;
  return 3;
}

export function formationPhaseState(phase: string, index: number): FormationPhaseState {
  const active = phaseStepIndex(phase);
  if (index < active) return 'complete';
  if (index === active) return 'active';
  return 'upcoming';
}

export function nextRevealedText({
  target,
  current,
  charsPerSec,
  deltaSeconds,
  reduceMotion,
}: {
  target: string;
  current: string;
  charsPerSec: number;
  deltaSeconds: number;
  reduceMotion: boolean;
}): string {
  if (reduceMotion || !target.startsWith(current)) return target;
  if (current.length >= target.length) return current;
  const step = Math.max(1, Math.round(charsPerSec * Math.min(0.1, deltaSeconds)));
  return target.slice(0, Math.min(target.length, current.length + step));
}

export function reducedMotionPreference(
  matchMedia: ((query: string) => { matches: boolean }) | undefined = typeof window ===
    'undefined' || !window.matchMedia
    ? undefined
    : window.matchMedia.bind(window)
): boolean {
  return matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false;
}

export function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(reducedMotionPreference);

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return;
    const media = window.matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(media.matches);
    update();
    media.addEventListener('change', update);
    return () => media.removeEventListener('change', update);
  }, []);

  return reduced;
}
