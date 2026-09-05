import { Check } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import {
  NODE_DOT,
  RADIUS,
  SectionHeader,
  TNUM,
  TONE_FILL,
  TONE_TEXT,
  WEIGHT,
  cx,
  type NodeIndex,
} from '../lz';
import {
  type FormationEvidence,
  type FormationPhaseState,
  type PhaseSpans,
  type RunPhase,
  fmtPhaseClock,
  fmtPhaseDuration,
  formationPhaseIndex,
  formationPhaseState,
  formationPhasesFor,
  phaseDurationMs,
} from './formationVisualState';

export type FormationRibbonNode = {
  device: string;
  working: boolean;
};

/** The fill of the CURRENT step: the accent while the run is live; the outcome tone once it ended. */
export type FormationActiveTone = 'accent' | 'ok' | 'err' | 'stopped';

function shortDeviceName(device: string): string {
  return device.match(/^([^-]+)/)?.[1] ?? device;
}

/** A node's ramp slot — identity only; the same slot the FLEET zone's dot uses. */
function nodeSlot(index: number): NodeIndex {
  return ((index % 6) + 1) as NodeIndex;
}

/** An IDLE node keeps its hue as a hollow ring; a working node is the filled dot. Literal class names so
 *  Tailwind's source scan generates every ring. */
const NODE_RING: Record<NodeIndex, string> = {
  1: 'border-2 border-lz-node-1',
  2: 'border-2 border-lz-node-2',
  3: 'border-2 border-lz-node-3',
  4: 'border-2 border-lz-node-4',
  5: 'border-2 border-lz-node-5',
  6: 'border-2 border-lz-node-6',
};

/** The stepper's three registers (DESIGN.md "States"): done = a check on a quiet hairline outline, current
 *  = the ONE solid fill, todo = a stronger outline in meta ink. Skipped keeps the stopped slate; held is a
 *  stopped outline with NO fill, so nothing asserts work while every node is deliberately idle. */
const STEP_STATE: Record<FormationPhaseState | 'held', string> = {
  complete: 'border border-lz-border text-lz-ink-2',
  active: '',
  upcoming: 'border border-lz-border-strong text-lz-ink-3',
  skipped: 'border border-lz-border text-lz-stopped',
  held: cx('border border-lz-stopped text-lz-stopped', WEIGHT.semibold),
};

/** The step's clock under its chip reads in the chip's own ink — solid, never a fade. */
const CLOCK_INK: Record<FormationPhaseState, string> = {
  complete: 'text-lz-ink-2',
  active: '',
  upcoming: 'text-lz-ink-3',
  skipped: TONE_TEXT.stopped,
};

/**
 * The run's route and its real fleet in ONE band: a horizontal STEPPER of the engine's phases — which is
 * live, which are behind it — with the working nodes under the live one. `phase` is the engine's own phase
 * key (see formationVisualState) — a null phase lights nothing, which is the honest rendering of a held run.
 */
export function FormationRibbon({
  phase,
  nodes,
  activeTone = 'accent',
  evidence,
  spans,
  now,
  held = false,
}: {
  phase: RunPhase | null;
  nodes: FormationRibbonNode[];
  activeTone?: FormationActiveTone;
  evidence?: FormationEvidence;
  /** Each phase's time on the clock from the event timestamps (foldRunPhase) — rendered under every
   *  step and stated in its tooltip with the clock range (VA-138: "synthesis took 35 min" was a
   *  12-minute synthesis and a 23-minute split lane under one chip). */
  spans?: PhaseSpans;
  /** The live clock for the active phase's open segment. Absent — a finished, killed or silent run —
   *  the newest event timestamp stands in, so a dead run's step never keeps counting. */
  now?: number;
  /** Engine-truth hold (run_paused with no later run_unpaused). The active step renders in a distinct
   *  held style — stopped outline, no fill — so nothing asserts work while every node is deliberately
   *  idle, WITHOUT un-completing the phases behind it. The phase used to be nulled for this, which
   *  returned every step to 'upcoming': pausing erased the run's whole history. */
  held?: boolean;
}) {
  // The steps are THIS run's: the live pipeline plus any retired phase (review/contracts — deleted
  // from the engine) that the run's own events prove it ran. An archived run keeps its historical
  // steps; a new run is never offered a stage the engine cannot reach.
  const steps = formationPhasesFor(evidence);
  const activeIndex = formationPhaseIndex(phase, steps);

  return (
    <div
      className="border-t border-lz-border px-3 pb-2"
      data-testid="formation-ribbon"
      data-active-phase={phase ?? 'none'}
    >
      {/* The FLEET zone names the nodes a few rows below; the ribbon keeps the dots, which carry WHICH
          node is lit, and states the count once, here. */}
      <SectionHeader
        title="Formation"
        className="w-full"
        right={
          <span className={cx('text-lz-meta text-lz-ink-3', TNUM)} aria-live="polite">
            {nodes.length} node{nodes.length === 1 ? '' : 's'}
          </span>
        }
      />
      {/* The rail WRAPS rather than truncating: ten labels at ~90px each fit one row on a 900px panel and
          fold onto a second row below that — a label is never cut to "Synthe…" (measured on every
          900px-wide panel before this). */}
      <ol className="flex flex-wrap items-start gap-x-2 gap-y-2" aria-label="Run phases">
        {steps.map((step, index) => {
          const state = formationPhaseState(phase, index, evidence, steps);
          // A HELD active step claims position, never work. Prior steps keep their evidence-based
          // complete/skipped states untouched.
          const heldActive = held && state === 'active';
          const register = heldActive
            ? STEP_STATE.held
            : state === 'active'
              ? cx(TONE_FILL[activeTone], WEIGHT.semibold)
              : STEP_STATE[state];
          const clockInk = heldActive
            ? TONE_TEXT.stopped
            : state === 'active'
              ? TONE_TEXT[activeTone]
              : CLOCK_INK[state];
          const span = spans?.phases[step.key];
          const durationMs = step.key === 'done' ? null : phaseDurationMs(spans, step.key, now);
          const duration = durationMs != null ? fmtPhaseDuration(durationMs) : null;
          return (
            // The chip is the li's FIRST child and its clock the sibling under it — the order the
            // VA-138 fixture reads. The connector to the previous step sits in the column gap.
            <li
              key={step.key}
              data-state={state}
              data-held={heldActive || undefined}
              className="relative flex flex-col items-center gap-1"
            >
              <Tooltip>
                <TooltipTrigger asChild>
                  <div
                    className={cx(
                      'flex h-7 items-center gap-1 whitespace-nowrap px-2.5 text-lz-meta [&_svg]:size-3',
                      RADIUS.control,
                      register
                    )}
                  >
                    {state === 'complete' ? <Check strokeWidth={3} aria-hidden /> : null}
                    <span>
                      {step.label}
                      {state === 'skipped' ? ' — skipped' : heldActive ? ' — held' : ''}
                    </span>
                  </div>
                </TooltipTrigger>
                <TooltipContent>
                  {duration && span ? (
                    <span className={cx('block font-mono text-lz-mono', WEIGHT.semibold, TNUM)}>
                      {step.label} · {duration}
                      {state === 'active' && !heldActive ? ' so far' : ''} · {fmtPhaseClock(span, now)}
                    </span>
                  ) : null}
                  {step.tip}
                </TooltipContent>
              </Tooltip>
              {/* THE STEP'S OWN CLOCK, under its chip: mono, tabular, in the chip's ink so the row reads
                  as a timeline — never inside the chip, where the label would truncate first. */}
              <div
                className={cx('min-h-[0.875rem] text-center font-mono text-lz-mono', WEIGHT.semibold, TNUM, clockInk)}
                data-testid="formation-duration"
                data-duration-phase={step.key}
              >
                {duration ?? ''}
              </div>
              {/* The fleet, under the live step only. Identity is a solid hue DOT (the letter and name
                  stay in the aria-label and title). State is the MARK, never a fade: a working node is a
                  filled dot, an idle one a hollow ring in its full hue. */}
              <div
                className="flex h-2.5 items-center justify-center gap-1"
                data-formation-phase={step.key}
              >
                {index === activeIndex
                  ? nodes.map((node, nodeIndex) => {
                      const letter = String.fromCharCode(65 + (nodeIndex % 26));
                      const nodeState = node.working ? 'working' : 'idle';
                      const slot = nodeSlot(nodeIndex);
                      return (
                        <span
                          key={node.device}
                          role="img"
                          aria-label={`Node ${letter}, ${shortDeviceName(node.device)}, ${nodeState}`}
                          title={`${shortDeviceName(node.device)} · ${nodeState}`}
                          className={cx(
                            'inline-block size-2.5 shrink-0',
                            RADIUS.pill,
                            node.working ? NODE_DOT[slot] : NODE_RING[slot]
                          )}
                          data-testid="formation-node"
                          data-node-state={nodeState}
                        />
                      );
                    })
                  : null}
              </div>
              {index > 0 ? (
                <span
                  aria-hidden
                  className="absolute left-0 top-3.5 h-px w-2 -translate-x-full bg-lz-border-strong"
                />
              ) : null}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

export default FormationRibbon;
