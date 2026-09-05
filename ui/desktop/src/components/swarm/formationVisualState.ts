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
  | 'split'
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
    tip: 'The fleet answers the opener’s own questions in parallel — one read-only call per question; answers splice into the briefs as facts beside the sources.',
  },
  {
    key: 'synthesize',
    label: 'Synthesize',
    tip: 'One node wires the researched slices into a task DAG; the engine splices each owner’s spec in verbatim.',
  },
  {
    key: 'review',
    label: 'Review',
    tip: 'RETIRED (2447d145c): one model round read the ORIGINAL request against the plan and patched it. Deleted after three runs produced zero effective patches — the plan repairs are deterministic now. Shown only for an archived run that ran it.',
  },
  {
    key: 'contracts',
    label: 'Contracts',
    tip: 'Every node freezes a signature-only interface for one module, so the builders code against the same contract before anything is written.',
  },
  {
    key: 'split',
    label: 'Split',
    tip: 'A task measured FAT (spec sections per owned file above the plan’s threshold) is declared into shards by one planner call, then sized to the free hosts — it runs AFTER synthesis, and it is its own step because r6j showed Synthesize for 35 minutes while synthesis took 12 and this call took 23. No phase event: derived from plan_flag{fat_task} → split_sized / plan_patched{split}.',
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

/** Phases the ribbon offers ONLY on the run's own evidence. Two classes share the rule:
 *
 *  RETIRED — deleted from the engine. Archived run.jsonl files still carry their `phase` events, so a
 *  run with EVIDENCE of one renders it as a historical step, but a new run must not be offered a chip
 *  for a stage the engine can no longer reach (it would sit permanently "skipped", claiming a route
 *  that does not exist). CONTRACTS (P1-4) and REVIEW (2447d145c, VA-014): without this entry
 *  formationPhaseState read EVERY new run as "Review — skipped" the moment Build lit.
 *
 *  CONDITIONAL — live, but only some runs walk them (VA-138: "the step list is DERIVED from events
 *  seen, never a fixed array"). ASK runs only when the opener leaves open decisions; SPLIT only when a
 *  task measures fat. Offering either up front painted a chip for a stage the run may never reach, and
 *  the r6j run (no open decision, one fat task) read "Ask — skipped" beside NO Split at all while the
 *  split lane ran 23 minutes under a "Synthesize" chip. A run that asked shows Ask; a run that split
 *  shows Split; a run that did neither shows neither.
 *
 *  Retired/conditional means "absent is not skipped", never "hidden": the archived r0 fixture still
 *  renders its Review chip off its own evidence. RESEARCH left this list when the v2 fan shipped: the
 *  engine researches on every run (one lane per slice), so the chip is always offered — it emits a
 *  `phase` event again since VA-089, and foldRunPhase also derives it from the fan's research_* events. */
export const RETIRED_PHASES: ReadonlyArray<RunPhase> = ['review', 'contracts'];
export const CONDITIONAL_PHASES: ReadonlyArray<RunPhase> = ['ask', 'split'];
const EVIDENCE_ONLY_PHASES: ReadonlyArray<RunPhase> = [...RETIRED_PHASES, ...CONDITIONAL_PHASES];

/** The steps the ribbon actually draws for THIS run: the unconditional pipeline, plus any retired or
 *  conditional phase the run's own events prove it ran. Evidence is set the moment a phase event is
 *  seen (foldRunPhase), so an archived run mid-research still carries its step. */
export function formationPhasesFor(
  evidence?: FormationEvidence
): ReadonlyArray<{ key: RunPhase; label: string; tip: string }> {
  return FORMATION_PHASES.filter(
    (step) => !EVIDENCE_ONLY_PHASES.includes(step.key) || evidence?.[step.key] === true
  );
}

/** Which phases the engine actually EMITTED. A phase behind the active one that was never observed reads
 *  'skipped' — the ribbon never back-fills a stage the run did not run (Integrate and Repair are both
 *  conditional, and a resumed run can start past Open). */
export type FormationEvidence = Partial<Record<RunPhase, boolean>>;

/** One phase's time on the clock, from the EVENT TIMESTAMPS (VA-138): `start` is the first entry
 *  (epoch ms), `ms` the closed time accumulated across every segment the run spent in it, and `since`
 *  the open segment's start while the run is still in it (null once it has left). A phase the run
 *  re-enters (Integrate → Repair → Integrate on a second verify round) keeps counting only its own
 *  segments — never the sibling's. */
export interface PhaseSpan {
  start: number;
  /** The newest exit (epoch ms); null while the run has never left it, or is back in it. */
  end: number | null;
  ms: number;
  since: number | null;
}

/** Every phase's span, plus the newest event timestamp — the clock a run that is no longer live
 *  (finished, killed, or its engine silent) reads its open phase against, so a dead run's active
 *  chip never keeps ticking. Built by foldRunPhase from the same `enter` calls that set the evidence. */
export interface PhaseSpans {
  phases: Partial<Record<RunPhase, PhaseSpan>>;
  lastTs: number | null;
}

export const EMPTY_PHASE_SPANS: PhaseSpans = { phases: {}, lastTs: null };

/** Milliseconds the run has spent in `key`, or null when the run never entered it (or its events carry
 *  no timestamps — fixtures and pre-ts archives). `now` is the live clock for an open segment; absent,
 *  the newest event timestamp stands in, which is what a finished or dead run must read. */
export function phaseDurationMs(
  spans: PhaseSpans | undefined,
  key: RunPhase,
  now?: number
): number | null {
  const span = spans?.phases[key];
  if (!span) return null;
  if (span.since == null) return span.ms;
  const clock = now ?? spans?.lastTs ?? span.since;
  return span.ms + Math.max(0, clock - span.since);
}

/** "52m", "2h 24m", "45s" — the chip's figure. Nearest minute past the first (r6j's synthesis was
 *  11m58s, and "12m" is the number a person watching the clock says): a phase measured in hours does
 *  not need its seconds, and the tooltip carries the exact clock range. */
export function fmtPhaseDuration(ms: number): string {
  const totalSec = Math.max(0, Math.round(ms / 1000));
  const totalMin = Math.round(totalSec / 60);
  if (totalMin >= 60) return `${Math.floor(totalMin / 60)}h ${totalMin % 60}m`;
  if (totalMin >= 1) return `${totalMin}m`;
  return `${totalSec}s`;
}

/** The wall-clock range a chip's tooltip states beside its duration: "18:52 → 19:04" in the reader's
 *  local time, open-ended while the phase is still running. */
export function fmtPhaseClock(span: PhaseSpan, now?: number): string {
  const hhmm = (t: number) =>
    new Date(t).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  if (span.since != null) return `${hhmm(span.start)} → ${now == null ? '…' : 'now'}`;
  return `${hhmm(span.start)} → ${span.end == null ? '…' : hhmm(span.end)}`;
}

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

/** The zone-header register every ribbon/zone label uses — the Studio `zone` type step (11px, 600,
 *  +0.08em, DESIGN.md "Typography"), which is the ONE uppercase register. `font-semibold` used to carry
 *  the weight here and compiles to NOTHING in this app (measured: the MCP theme registration sets
 *  `--font-weight-*` to `initial`), so every zone label rendered at 400; the token step carries its own
 *  weight. Inline meta stays 11px normal case on ink-3. */
export const EYEBROW_CLASS = 'text-lz-zone uppercase';

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
  if (active < 0) {
    // NO ACTIVE STEP — before the first phase event, or a caller with no live phase. History must
    // stay lit off the evidence map: nulling the phase used to return 'upcoming' for EVERYTHING,
    // so pausing a mid-build run erased every completed checkmark ("nothing has run yet" on a run
    // that ran four phases). Steps below the FURTHEST observed one render off evidence exactly as
    // they do behind an active step; the furthest observed step itself asserts neither work nor
    // completion — evidence is set on phase ENTRY, and a run interrupted mid-Build has build:true
    // with the build half-done, so a green check there would be unearned.
    let furthest = -1;
    steps.forEach((step, i) => {
      if (evidence?.[step.key] === true) furthest = i;
    });
    if (index < furthest) return evidence?.[steps[index].key] === true ? 'complete' : 'skipped';
    return 'upcoming';
  }
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

/**
 * IS THE PAGE VISIBLE AT ALL? A hidden/occluded window suspends requestAnimationFrame ENTIRELY
 * (measured over CDP on the live r0 benchmark, 2026-08-30: `visibilityState === 'hidden'`, zero rAF
 * callbacks in 1.5s, timers clamped to ~1s), so any content that advances only inside a rAF loop
 * freezes at its last frame — mid-word — while React keeps committing fresh props behind it. Content
 * must never depend on the animation loop; a consumer that types text on rAF snaps to the target
 * whenever this is false, exactly as it does for reduced motion.
 */
export function pageVisibility(
  doc: { visibilityState?: string } | undefined = typeof document === 'undefined'
    ? undefined
    : document
): boolean {
  return doc?.visibilityState !== 'hidden';
}

export function usePageVisible(): boolean {
  const [visible, setVisible] = useState(pageVisibility);

  useEffect(() => {
    if (typeof document === 'undefined') return;
    const update = () => setVisible(pageVisibility());
    update();
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);

  return visible;
}
