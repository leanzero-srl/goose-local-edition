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
    label: 'Build',
    tip: 'Worker nodes build the planned tasks across the fleet.',
  },
  {
    label: 'Integrate',
    tip: 'When the plan has an integration sink, it assembles and verifies the built work.',
  },
  {
    label: 'Repair',
    tip: 'Verification findings are repaired and checked again until the run converges.',
  },
  { label: 'Done', tip: 'The run has finished.' },
] as const;

export type FormationPhaseState = 'complete' | 'active' | 'upcoming' | 'skipped';

export const FORMATION_FALLBACKS = [
  '#17c4c4',
  '#2e8bff',
  '#8277ff',
  '#b14cff',
  '#ff3ea5',
  '#ff5c7a',
] as const;

export const FORMATION_RAMP = [
  `var(--color-node-1, ${FORMATION_FALLBACKS[0]})`,
  `var(--color-node-2, ${FORMATION_FALLBACKS[1]})`,
  `var(--color-node-3, ${FORMATION_FALLBACKS[2]})`,
  `var(--color-node-4, ${FORMATION_FALLBACKS[3]})`,
  `var(--color-node-5, ${FORMATION_FALLBACKS[4]})`,
  `var(--color-node-6, ${FORMATION_FALLBACKS[5]})`,
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
  if (/repair|fix/.test(normalized)) return 4;
  if (/verif|integrat|sink/.test(normalized)) return 3;
  if (/build|execut|dispatch|working/.test(normalized)) return 2;
  // Legacy contract events were planning evidence before V21 moved directly into build. Preserve their
  // position in the run without advertising a stage the current engine does not execute.
  if (/contract|plan|synthesis/.test(normalized)) return 1;
  if (/research|scout|pillar.open|start/.test(normalized)) return 0;
  return 2;
}

export function contrastRatio(foreground: string, background: string): number {
  const luminance = (hex: string) => {
    const channels = hex
      .replace('#', '')
      .match(/.{2}/g)!
      .map((channel) => parseInt(channel, 16) / 255)
      .map((channel) =>
        channel <= 0.04045 ? channel / 12.92 : Math.pow((channel + 0.055) / 1.055, 2.4)
      );
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
  };
  const foregroundLuminance = luminance(foreground);
  const backgroundLuminance = luminance(background);
  return (
    (Math.max(foregroundLuminance, backgroundLuminance) + 0.05) /
    (Math.min(foregroundLuminance, backgroundLuminance) + 0.05)
  );
}

export function formationPhaseState(
  phase: string,
  index: number,
  evidence?: { integrationObserved: boolean; repairObserved: boolean }
): FormationPhaseState {
  const active = phaseStepIndex(phase);
  if (index < active && index === 3 && evidence && !evidence.integrationObserved) return 'skipped';
  if (index < active && index === 4 && evidence && !evidence.repairObserved) return 'skipped';
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
