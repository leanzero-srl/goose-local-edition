import { Check, ChevronRight } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, RADIUS, TNUM, TONE_FILL, TYPE, WEIGHT, cx, type Tone } from '../lz';
import { shortModelName } from '../noNodeNotice/mlxMount';
import type { MlxTab } from '../../utils/navigationUtils';

const i18n = defineMessages({
  aria: { id: 'mlxSetup.aria', defaultMessage: 'Setting up LeanZero MLX' },
  step1: { id: 'mlxSetup.step1', defaultMessage: 'Your Macs' },
  step1Done: {
    id: 'mlxSetup.step1Done',
    defaultMessage: '{count, plural, =1 {# Mac} other {# connected}}',
  },
  step1Next: { id: 'mlxSetup.step1Next', defaultMessage: 'Connect your other Macs' },
  step2: { id: 'mlxSetup.step2', defaultMessage: 'Models' },
  step2Done: { id: 'mlxSetup.step2Done', defaultMessage: '{count} on your Macs' },
  step2Next: { id: 'mlxSetup.step2Next', defaultMessage: 'Get a model' },
  step3: { id: 'mlxSetup.step3', defaultMessage: 'Run it' },
  step3Done: { id: 'mlxSetup.step3Done', defaultMessage: '{model} running' },
  step3Next: { id: 'mlxSetup.step3Next', defaultMessage: 'Run a model' },
  step4: { id: 'mlxSetup.step4', defaultMessage: 'Nodes' },
  step4Done: {
    id: 'mlxSetup.step4Done',
    defaultMessage: '{count, plural, one {# node} other {# nodes}}',
  },
  step4Next: { id: 'mlxSetup.step4Next', defaultMessage: 'Add a node' },
  next: { id: 'mlxSetup.next', defaultMessage: 'Next' },
  compact: { id: 'mlxSetup.compact', defaultMessage: 'Step {n} of {total} · {label}' },
});

/** What the strip is computed from — each fact read by the view from a store that already exists. */
export interface SetupFacts {
  /** The agent advertises LeanZero Link: other Macs can be connected at all. */
  linkAvailable: boolean;
  /** This Mac is connected to LeanZero Link. */
  linkConnected: boolean;
  /** Macs Link reports reachable, this one included. */
  macsOnline: number;
  /** What the Models tab counts (`matrixRowCount`) — the strip and the tab badge never disagree. */
  models: number;
  /** The model a way serves right now (single, remote single or split); null = nothing answers. */
  running: string | null;
  /** Nodes in the swarm pool; null = the pool has not been read, so nothing is claimed. */
  nodes: number | null;
}

export type SetupTarget = { kind: 'mlx'; tab: MlxTab } | { kind: 'nodes' };
export type SetupStepState = 'done' | 'next' | 'later';

export interface SetupStep {
  n: 1 | 2 | 3 | 4;
  state: SetupStepState;
  target: SetupTarget;
}

const STEP_TONE: Record<SetupStepState, Tone> = { done: 'ok', next: 'accent', later: 'stopped' };

/**
 * The four steps of the flow the owner asked the UI to imply — connect your Macs, get a model, run
 * it, make it a node — each done or not from the facts. The first step not done is `next`; the ones
 * after it are `later`. A single Mac without Link is a complete step 1: there is nothing to connect.
 */
export function setupSteps(facts: SetupFacts): SetupStep[] {
  const done = [
    !facts.linkAvailable || facts.linkConnected,
    facts.models > 0,
    facts.running != null,
    facts.nodes != null && facts.nodes > 0,
  ];
  const firstOpen = done.indexOf(false);
  const targets: SetupTarget[] = [
    { kind: 'mlx', tab: facts.linkAvailable ? 'macs' : 'engine' },
    { kind: 'mlx', tab: 'models' },
    { kind: 'mlx', tab: 'engine' },
    { kind: 'nodes' },
  ];
  return targets.map((target, i) => ({
    n: (i + 1) as SetupStep['n'],
    state: done[i] ? 'done' : i === firstOpen ? 'next' : 'later',
    target,
  }));
}

export function MlxSetupStrip({
  facts,
  onOpen,
}: {
  facts: SetupFacts;
  onOpen: (target: SetupTarget) => void;
}) {
  const intl = useIntl();
  const steps = setupSteps(facts);

  const name = (n: SetupStep['n']): string =>
    intl.formatMessage([i18n.step1, i18n.step2, i18n.step3, i18n.step4][n - 1]);

  const detail = (step: SetupStep): string | null => {
    const done = step.state === 'done';
    switch (step.n) {
      case 1:
        return done
          ? intl.formatMessage(i18n.step1Done, {
              count: facts.linkAvailable ? facts.macsOnline : 1,
            })
          : intl.formatMessage(i18n.step1Next);
      case 2:
        return done
          ? intl.formatMessage(i18n.step2Done, { count: facts.models })
          : intl.formatMessage(i18n.step2Next);
      case 3:
        return done && facts.running
          ? intl.formatMessage(i18n.step3Done, { model: shortModelName(facts.running) })
          : intl.formatMessage(i18n.step3Next);
      case 4:
        if (facts.nodes == null) return null;
        return done
          ? intl.formatMessage(i18n.step4Done, { count: facts.nodes })
          : intl.formatMessage(i18n.step4Next);
    }
  };

  const chip = (step: SetupStep) => {
    const words = detail(step);
    return (
      <button
        type="button"
        data-testid={`mlx-setup-step-${step.n}`}
        data-state={step.state}
        onClick={() => onOpen(step.target)}
        className={cx(
          'inline-flex h-8 items-center gap-2 whitespace-nowrap px-3 text-[12px] hover:underline [&_svg]:size-3.5 [&_svg]:shrink-0',
          RADIUS.control,
          TONE_FILL[STEP_TONE[step.state]],
          FOCUS,
          MOTION,
          TNUM
        )}
      >
        <span className={WEIGHT.semibold}>{step.n}</span>
        <span className={WEIGHT.semibold}>{name(step.n)}</span>
        {words != null && (
          <>
            <span aria-hidden>·</span>
            <span>{words}</span>
          </>
        )}
        {step.state === 'done' && <Check aria-hidden />}
        {step.state === 'next' && (
          <span className={cx('border border-current px-1.5 text-[11px]', RADIUS.control)}>
            {intl.formatMessage(i18n.next)}
          </span>
        )}
      </button>
    );
  };

  const next = steps.find((s) => s.state === 'next');

  return (
    <nav aria-label={intl.formatMessage(i18n.aria)} data-testid="mlx-setup-strip">
      {next && (
        <div
          className="flex flex-wrap items-center gap-2 sm:hidden"
          data-testid="mlx-setup-compact"
        >
          <span className={TYPE.meta}>
            {intl.formatMessage(i18n.compact, {
              n: next.n,
              total: steps.length,
              label: name(next.n),
            })}
          </span>
          {chip(next)}
        </div>
      )}
      <ol
        className={cx('flex-wrap items-center gap-1.5', next ? 'hidden sm:flex' : 'flex')}
        data-testid="mlx-setup-steps"
      >
        {steps.map((step, i) => (
          <li key={step.n} className="flex items-center gap-1.5">
            {i > 0 && <ChevronRight aria-hidden className="size-4 shrink-0 text-lz-ink-3" />}
            {chip(step)}
          </li>
        ))}
      </ol>
    </nav>
  );
}
