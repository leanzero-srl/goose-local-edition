import React from 'react';
import { Check } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import {
  FORMATION_PHASES,
  FORMATION_RAMP,
  SWARM_STATUS,
  formationPhaseState,
  phaseStepIndex,
} from './formationVisualState';

export type FormationRibbonNode = {
  device: string;
  working: boolean;
};

function shortDeviceName(device: string): string {
  return device.match(/^([^-]+)/)?.[1] ?? device;
}

export function FormationRibbon({
  phase,
  nodes,
  activeColor = SWARM_STATUS.action,
  metrics,
}: {
  phase: string;
  nodes: FormationRibbonNode[];
  activeColor?: string;
  metrics?: React.ReactNode;
}) {
  const activeIndex = phaseStepIndex(phase);
  const workingCount = nodes.filter((node) => node.working).length;

  return (
    <div
      className="overflow-x-auto border-t border-border-primary bg-background-primary px-3 py-2"
      data-testid="formation-ribbon"
      data-active-phase={FORMATION_PHASES[activeIndex].label.toLowerCase()}
    >
      <div className="min-w-[660px]">
        <div className="mb-2 flex min-h-5 items-center justify-between gap-3">
          <span className="font-mono text-xs font-bold uppercase tracking-[0.14em] text-text-secondary">
            Formation
          </span>
          {metrics}
        </div>
        <ol className="grid grid-cols-6 gap-1" aria-label="Run phases">
          {FORMATION_PHASES.map((step, index) => {
            const state = formationPhaseState(phase, index);
            return (
              <li key={step.label} data-state={state} className="min-w-0">
                <Tooltip>
                  <TooltipTrigger asChild>
                    <div
                      className="flex h-7 items-center justify-center gap-1 border px-2 text-xs font-semibold"
                      style={{
                        borderRadius: 3,
                        borderColor:
                          state === 'active'
                            ? activeColor
                            : state === 'complete'
                              ? SWARM_STATUS.done
                              : 'var(--color-border-primary)',
                        backgroundColor: state === 'active' ? activeColor : 'transparent',
                        color:
                          state === 'active'
                            ? '#ffffff'
                            : state === 'complete'
                              ? SWARM_STATUS.done
                              : 'var(--color-text-secondary)',
                      }}
                    >
                      {state === 'complete' ? <Check className="h-3 w-3" strokeWidth={3} /> : null}
                      <span className="truncate">{step.label}</span>
                    </div>
                  </TooltipTrigger>
                  <TooltipContent>{step.tip}</TooltipContent>
                </Tooltip>
              </li>
            );
          })}
        </ol>

        <div className="mt-1 grid min-h-8 grid-cols-6 gap-1" aria-label="Fleet formation">
          {FORMATION_PHASES.map((step, index) => (
            <div
              key={step.label}
              className="flex min-w-0 items-center justify-center gap-1"
              data-formation-phase={step.label.toLowerCase()}
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
                        className="inline-flex h-5 w-5 shrink-0 items-center justify-center border-2 font-mono text-xs font-bold text-[#0b0b0b]"
                        style={{
                          borderRadius: 3,
                          backgroundColor: FORMATION_RAMP[nodeIndex % FORMATION_RAMP.length],
                          borderColor: node.working
                            ? SWARM_STATUS.running
                            : 'var(--color-border-tertiary)',
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
          {workingCount} working · {nodes.length - workingCount} idle
        </p>
      </div>
    </div>
  );
}

export default FormationRibbon;
