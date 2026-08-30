import { useEffect, useState } from 'react';

/**
 * The run pipeline the Formation Ribbon draws, and the mapping from ENGINE TRUTH onto it.
 *
 * The load-bearing decision here is that the active step comes from the engine's own
 * `{"event":"phase","phase":"open"|"ask"|"research"|"synthesis"|"review"|"contracts"|"build"|…}` plus the
 * task lifecycle — NEVER from pattern-matching a human phase label. Every phase the engine emits is a step
 * of its own: ASK used to be folded into Open and CONTRACTS into Build, and a fleet generating interface
 * stubs for six minutes rendered as "Build active" — the owner's words: "Contracts is somehow in build". The label version defaulted an unrecognised string to
 * Build, so a run sitting on "Paused" (or "Starting…", or any label added later) rendered as "Build active"
 * — the ribbon asserting work was happening while every node was deliberately idle.
 *
 * `null` is a legitimate state: before the first phase event, and whenever the run is held. The ribbon then
 * shows NO active step rather than inventing one.
 */
export type RunPhase =
  | 'open'
  | 'ask'
  | 'research'
  | 'synthesize'
  | 'review'
  | 'contracts'
  | 'build'
  | 'integrate'
  | 'repair'
  | 'done';

export const FORMATION_PHASES: ReadonlyArray<{ key: RunPhase; label: string; tip: string }> = [
  {
    key: 'open',
    label: 'Open',
    tip: 'One node splits the request into balanced semantic slices, and names the decisions it cannot make alone.',
  },
  {
    key: 'ask',
    label: 'Ask',
    tip: 'The opener named decisions it could not make alone. The run pauses for your answer, or goose answers from the spec.',
  },
  {
    key: 'research',
    label: 'Research',
    tip: 'Every node owns one slice: it answers that slice’s questions, then writes that module’s full specification.',
  },
  {
    key: 'synthesize',
    label: 'Synthesize',
    tip: 'One node wires the researched slices into a task DAG; the engine splices each owner’s spec in verbatim.',
  },
  {
    key: 'review',
    label: 'Review',
    tip: 'An idle node reads the ORIGINAL request against the plan and patches what is missing. It stops when it asks for no change.',
  },
  {
    key: 'contracts',
    label: 'Contracts',
    tip: 'Every node freezes a signature-only interface for one module, so the builders code against the same contract before anything is written.',
  },
  { key: 'build', label: 'Build', tip: 'Worker nodes build the planned tasks across the fleet.' },
  {
    key: 'integrate',
    label: 'Integrate',
    tip: 'The file-less join boots the app and exercises every advertised route end-to-end — the wiring itself was the FIRST task (skeleton), so this verifies, it does not assemble.',
  },
  {
    key: 'repair',
    label: 'Repair',
    tip: 'Defects found at integration are rated and repaired, then checked again.',
  },
  { key: 'done', label: 'Done', tip: 'The run has finished.' },
] as const;

export type FormationPhaseState = 'complete' | 'active' | 'upcoming' | 'skipped';

/** Phases DELETED from the engine (P1-5 removed the RESEARCH fan, P1-4 removed CONTRACTS). They are
 *  retired, not erased: archived run.jsonl files still carry their `phase` events, so a run with
 *  EVIDENCE of one renders it as a historical step — but a new run must not be offered a chip for a
 *  stage the engine can no longer reach (it would sit permanently "skipped", claiming a route that
 *  does not exist). */
export const RETIRED_PHASES: ReadonlyArray<RunPhase> = ['research', 'contracts'];

/** The steps the ribbon actually draws for THIS run: the live pipeline, plus any retired phase the
 *  run's own events prove it ran. Evidence is set the moment a phase event is seen (foldRunPhase), so
 *  an archived run mid-research still carries its step. */
export function formationPhasesFor(
  evidence?: FormationEvidence
): ReadonlyArray<{ key: RunPhase; label: string; tip: string }> {
  return FORMATION_PHASES.filter(
    (step) => !RETIRED_PHASES.includes(step.key) || evidence?.[step.key] === true
  );
}

/** Which phases the engine actually EMITTED. A phase behind the active one that was never observed reads
 *  'skipped' — the ribbon never back-fills a stage the run did not run (Integrate and Repair are both
 *  conditional, and a resumed run can start past Open). */
export type FormationEvidence = Partial<Record<RunPhase, boolean>>;

/** The node-identity ramp: six solid, distinct hues so two adjacent node chips are never confusable. The
 *  var() carries the theme swap; the fallback is the light-theme LeanZero value. */
export const FORMATION_FALLBACKS = [
  '#1d4ed8',
  '#0891b2',
  '#7c3aed',
  '#ea580c',
  '#db2777',
  '#16a34a',
] as const;

export const FORMATION_RAMP = FORMATION_FALLBACKS.map(
  (hex, i) => `var(--color-node-${i + 1}, ${hex})`
);

/** The glyph colour ON each ramp hue. A single ink cannot clear AA across six saturated fills — white is
 *  6.7:1 on the blue and 3.3:1 on the green — so the ink is chosen per hue rather than the hue being washed
 *  out to suit one glyph colour. Kept in lockstep with FORMATION_FALLBACKS and pinned by a contrast test. */
export const FORMATION_INK_FALLBACKS = [
  '#ffffff',
  '#0b0b0b',
  '#ffffff',
  '#0b0b0b',
  '#ffffff',
  '#0b0b0b',
] as const;

export const FORMATION_INK = FORMATION_INK_FALLBACKS.map(
  (hex, i) => `var(--color-node-${i + 1}-ink, ${hex})`
);

/** The LeanZero status palette. The `solid*` entries are FILLS that carry white text; the plain entries are
 *  foreground/border colors. Both are fully saturated — never a tint, never an opacity fade. */
export const SWARM_STATUS = {
  running: 'var(--color-status-warn, #d97706)',
  done: 'var(--color-status-ok, #16a34a)',
  error: 'var(--color-status-err, #dc2626)',
  action: 'var(--color-action-solid, #1d4ed8)',
  stopped: 'var(--color-status-stopped, #475569)',
  solidRunning: 'var(--color-status-warn-solid, #b45309)',
  solidDone: 'var(--color-status-ok-solid, #15803d)',
  solidError: 'var(--color-status-err-solid, #dc2626)',
  solidStopped: 'var(--color-status-stopped-solid, #475569)',
} as const;

/** The eyebrow register every ribbon/zone label uses — LeanZero's: mono, 10px, 700, .18em, uppercase. */
export const EYEBROW_CLASS = 'font-mono text-[10px] font-bold uppercase tracking-[0.18em]';

/** Chip and panel radii — the two values the swarm view is allowed to use. */
export const CHIP_RADIUS = 6;
export const PANEL_RADIUS = 8;

/** Position of a phase in the pipeline, or -1 for "no phase" (before the first phase event, or held).
 *  `steps` is the run's own list (formationPhasesFor) — indices are only meaningful against the list
 *  being rendered, which no longer always equals FORMATION_PHASES. */
export function formationPhaseIndex(
  phase: RunPhase | null,
  steps: ReadonlyArray<{ key: RunPhase }> = FORMATION_PHASES
): number {
  if (!phase) return -1;
  return steps.findIndex((step) => step.key === phase);
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
  phase: RunPhase | null,
  index: number,
  evidence?: FormationEvidence,
  steps: ReadonlyArray<{ key: RunPhase }> = FORMATION_PHASES
): FormationPhaseState {
  const active = formationPhaseIndex(phase, steps);
  if (active < 0) return 'upcoming';
  if (index < active) {
    return evidence && evidence[steps[index].key] !== true ? 'skipped' : 'complete';
  }
  if (index === active) return 'active';
  return 'upcoming';
}

/** One frame of the typewriter reveal. Any non-append change SNAPS to the target: re-typing from a shrunken
 *  prefix made the live text jump backwards, which read as broken. */
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
