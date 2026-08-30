import React from 'react';
import { Check } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import {
  CHIP_RADIUS,
  EYEBROW_CLASS,
  FORMATION_INK,
  FORMATION_RAMP,
  SWARM_STATUS,
  type FormationEvidence,
  type RunPhase,
  formationPhaseIndex,
  formationPhaseState,
  formationPhasesFor,
} from './formationVisualState';

export type FormationRibbonNode = {
  device: string;
  working: boolean;
};


function shortDeviceName(device: string): string {
  return device.match(/^([^-]+)/)?.[1] ?? device;
}

/**
 * The run's route and its real fleet in ONE band: which of the engine's phases is live, which are
 * behind it, and which nodes are working under the live one. `phase` is the engine's own phase key (see
 * formationVisualState) — a null phase lights nothing, which is the honest rendering of a held run.
 */
export function FormationRibbon({
  phase,
  nodes,
  activeColor = SWARM_STATUS.action,
  metrics,
  evidence,
  held = false,
}: {
  phase: RunPhase | null;
  nodes: FormationRibbonNode[];
  activeColor?: string;
  metrics?: React.ReactNode;
  evidence?: FormationEvidence;
  /** Engine-truth hold (run_paused with no later run_unpaused). The active chip renders in a distinct
   *  held style — stopped-grey border, no fill — so nothing asserts work while every node is
   *  deliberately idle, WITHOUT un-completing the phases behind it. The phase used to be nulled for
   *  this, which returned every chip to 'upcoming': pausing erased the run's whole history. */
  held?: boolean;
}) {
  // The steps are THIS run's: the live pipeline plus any retired phase (research/contracts — deleted
  // from the engine) that the run's own events prove it ran. An archived run keeps its historical
  // chips; a new run is never offered a stage the engine cannot reach. One column per step —
  // Tailwind's `grid-cols-N` is a static class, so the template is derived from the list.
  const steps = formationPhasesFor(evidence);
  const phaseColumns = `repeat(${steps.length}, minmax(0, 1fr))`;
  const activeIndex = formationPhaseIndex(phase, steps);

  return (
    <div
      className="overflow-x-auto border-t border-border-primary bg-background-primary px-3 py-2"
      data-testid="formation-ribbon"
      data-active-phase={phase ?? 'none'}
    >
      <div className="min-w-[900px]">
        <div className="mb-2 flex min-h-5 items-center justify-between gap-3">
          <span className={`${EYEBROW_CLASS} text-text-secondary`}>Formation</span>
          {metrics}
        </div>
        <ol
          className="grid gap-1"
          style={{ gridTemplateColumns: phaseColumns }}
          aria-label="Run phases"
        >
          {steps.map((step, index) => {
            const state = formationPhaseState(phase, index, evidence, steps);
            const color =
              state === 'active'
                ? activeColor
                : state === 'complete'
                  ? SWARM_STATUS.done
                  : state === 'skipped'
                    ? SWARM_STATUS.stopped
                    : 'var(--color-text-secondary)';
            // A HELD active chip claims position, never work: stopped-grey border and ink, no fill.
            // Prior chips keep their evidence-based complete/skipped states untouched.
            const heldActive = held && state === 'active';
            return (
              <li key={step.key} data-state={state} data-held={heldActive || undefined} className="min-w-0">
                <Tooltip>
                  <TooltipTrigger asChild>
                    <div
                      className="flex h-7 items-center justify-center gap-1 border px-2 text-xs font-semibold"
                      style={{
                        borderRadius: CHIP_RADIUS,
                        borderColor: heldActive
                          ? SWARM_STATUS.stopped
                          : state === 'upcoming'
                            ? 'var(--color-border-primary)'
                            : (color as string),
                        backgroundColor:
                          state === 'active' && !heldActive ? activeColor : 'transparent',
                        color: heldActive
                          ? SWARM_STATUS.stopped
                          : state === 'active'
                            ? '#ffffff'
                            : (color as string),
                      }}
                    >
                      {state === 'complete' ? <Check className="h-3 w-3" strokeWidth={3} /> : null}
                      <span className="truncate">
                        {step.label}
                        {state === 'skipped' ? ' — skipped' : heldActive ? ' — held' : ''}
                      </span>
                    </div>
                  </TooltipTrigger>
                  <TooltipContent>{step.tip}</TooltipContent>
                </Tooltip>
              </li>
            );
          })}
        </ol>

        <div
          className="mt-1 grid min-h-8 gap-1"
          style={{ gridTemplateColumns: phaseColumns }}
          aria-label="Fleet formation"
        >
          {steps.map((step, index) => (
            <div
              key={step.key}
              className="flex min-w-0 items-center justify-center gap-1"
              data-formation-phase={step.key}
            >
              {index === activeIndex
                ? nodes.map((node, nodeIndex) => {
                    const letter = String.fromCharCode(65 + (nodeIndex % 26));
                    const state = node.working ? 'working' : 'idle';
                    return (
                      <span
                        key={node.device}
                        aria-label={`Node ${letter}, ${shortDeviceName(node.device)}, ${state}`}
                        title={`${shortDeviceName(node.device)} · ${state}`}
                        className="inline-flex h-5 w-5 shrink-0 items-center justify-center border-2 font-mono text-xs font-bold"
                        style={{
                          borderRadius: CHIP_RADIUS,
                          backgroundColor: FORMATION_RAMP[nodeIndex % FORMATION_RAMP.length],
                          color: FORMATION_INK[nodeIndex % FORMATION_INK.length],
                          // An idle node keeps its full-strength identity fill and loses only the running
                          // OUTLINE — dimming the fill would make the fleet read as absent rather than idle.
                          borderColor: node.working
                            ? SWARM_STATUS.running
                            : 'var(--color-border-primary)',
                        }}
                        data-testid="formation-node"
                        data-node-state={state}
                      >
                        {letter}
                      </span>
                    );
                  })
                : null}
            </div>
          ))}
        </div>
        <p className="text-center text-xs text-text-secondary" aria-live="polite">
          {/* The FLEET zone header already reads "N nodes · N working" a few rows below, and the node
              chips above already show which are lit. Three renderings of one fact. The ribbon keeps the
              chips, which carry WHICH node; the count belongs where the nodes are named. */}
          {nodes.length} node{nodes.length === 1 ? '' : 's'}
        </p>
      </div>
    </div>
  );
}

export default FormationRibbon;
