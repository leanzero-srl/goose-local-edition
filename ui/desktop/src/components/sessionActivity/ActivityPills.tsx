import { useSyncExternalStore, type ReactNode } from 'react';
import { CircleStop, Hand, Repeat, TriangleAlert } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { PHASE_FILL, RADIUS, TNUM, TONE_FILL, cx, type Tone } from '../lz';
import { clockTime, parseTime } from '../loops/model';
import { backgroundWorkLabel, backgroundWorkShort } from './backgroundWorkText';
import {
  elapsedLabel,
  sessionStates,
  useActivityOf,
  type BackgroundWorkKind,
  type LoopStatus,
} from './sessionActivityStore';
import { stoppedTurnText } from './stoppedTurnText';

const i18n = defineMessages({
  running: { id: 'sessionActivity.running', defaultMessage: 'Running · {elapsed}' },
  runningLabel: {
    id: 'sessionActivity.runningLabel',
    defaultMessage: 'A turn is running now, for {elapsed}',
  },
  needsYou: { id: 'sessionActivity.needsYou', defaultMessage: 'Needs you' },
  failed: { id: 'sessionActivity.failed', defaultMessage: 'Failed' },
  failedLabel: {
    id: 'sessionActivity.failedLabel',
    defaultMessage: 'The last turn failed: {reason}',
  },
  failedLabelBare: {
    id: 'sessionActivity.failedLabelBare',
    defaultMessage: 'The last turn failed',
  },
  stopped: { id: 'sessionActivity.stopped', defaultMessage: 'Stopped' },
  needsYouCount: { id: 'sessionActivity.needsYouCount', defaultMessage: 'Needs you · {count}' },
  needsYouLabel: {
    id: 'sessionActivity.needsYouLabel',
    defaultMessage:
      '{count, plural, one {Waiting for your answer} other {# questions waiting for your answer}}',
  },
  backgroundLabel: {
    id: 'sessionActivity.backgroundLabel',
    defaultMessage: 'goose is still working for this chat: {work}',
  },
  looping: { id: 'sessionActivity.looping', defaultMessage: 'Looping' },
  loopNext: { id: 'sessionActivity.loopNext', defaultMessage: 'Next {time}' },
  loopPaused: { id: 'sessionActivity.loopPaused', defaultMessage: 'Paused' },
  loopWaitingYou: { id: 'sessionActivity.loopWaitingYou', defaultMessage: 'Waiting for you' },
  loopUnreadable: { id: 'sessionActivity.loopUnreadable', defaultMessage: 'Loop unreadable' },
  loopLabelNext: {
    id: 'sessionActivity.loopLabelNext',
    defaultMessage: "This chat's loop runs its next tick at {time}",
  },
  loopLabelNextMissing: {
    id: 'sessionActivity.loopLabelNextMissing',
    defaultMessage: "This chat's loop is waiting, but goose sent no time for its next tick",
  },
  loopLabelNextUnreadable: {
    id: 'sessionActivity.loopLabelNextUnreadable',
    defaultMessage: "This chat's loop is waiting; its next tick time could not be read: {error}",
  },
  loopLabelRunning: {
    id: 'sessionActivity.loopLabelRunning',
    defaultMessage: "This chat's loop is running a tick",
  },
  loopLabelChecking: {
    id: 'sessionActivity.loopLabelChecking',
    defaultMessage: "This chat's loop is running its check after a tick",
  },
  loopLabelWaitingTurn: {
    id: 'sessionActivity.loopLabelWaitingTurn',
    defaultMessage: "A tick of this chat's loop is due; it starts when the turn before it ends",
  },
  loopLabelWaitingYou: {
    id: 'sessionActivity.loopLabelWaitingYou',
    defaultMessage: "The last tick of this chat's loop didn't say when to come back",
  },
  loopLabelNeedsYou: {
    id: 'sessionActivity.loopLabelNeedsYou',
    defaultMessage: "This chat's loop waits until your answer to its tick has run",
  },
  loopLabelPaused: {
    id: 'sessionActivity.loopLabelPaused',
    defaultMessage: "This chat's loop is paused; open the chat to see why and resume it",
  },
  loopLabelElsewhere: {
    id: 'sessionActivity.loopLabelElsewhere',
    defaultMessage: "This chat's loop runs in another goose window",
  },
});

const PILL =
  'inline-flex h-5 shrink-0 items-center gap-1 whitespace-nowrap px-1.5 text-lz-meta font-lz-semibold';

let nowListeners = new Set<() => void>();
let nowTimer: ReturnType<typeof setInterval> | undefined;
let now = Date.now();
let frozen = false;

function subscribeNow(listener: () => void): () => void {
  nowListeners.add(listener);
  if (!nowTimer && !frozen) {
    nowTimer = setInterval(() => {
      now = Date.now();
      for (const l of nowListeners) l();
    }, 1000);
  }
  return () => {
    nowListeners.delete(listener);
    if (nowListeners.size === 0 && nowTimer) {
      clearInterval(nowTimer);
      nowTimer = undefined;
    }
  };
}

/** One shared one-second clock for every live elapsed on screen. */
export function useNow(): number {
  return useSyncExternalStore(subscribeNow, () => now);
}

/** Tests pin the clock: the live one-second tick stays off until the next module load. */
export function resetNowForTests(at: number): void {
  now = at;
  frozen = true;
  nowListeners = new Set();
  if (nowTimer) clearInterval(nowTimer);
  nowTimer = undefined;
}

/** Solid green with a live dot: a turn is in flight right now. */
export function RunningPill({ since, className }: { since: string; className?: string }) {
  const intl = useIntl();
  const elapsed = elapsedLabel(since, useNow());
  return (
    <span
      data-testid="session-running-pill"
      title={intl.formatMessage(i18n.runningLabel, { elapsed })}
      aria-label={intl.formatMessage(i18n.runningLabel, { elapsed })}
      className={cx(PILL, RADIUS.pill, PHASE_FILL.writing, TNUM, className)}
    >
      <span aria-hidden className="size-1.5 animate-lz-live rounded-full bg-current" />
      {intl.formatMessage(i18n.running, { elapsed })}
    </span>
  );
}

/**
 * Solid secondary with a still dot: no turn runs, but goose is still working for the session — the
 * fact check after the reply, a title (Q-185). Quieter than Running, never the idle "4m ago".
 */
export function BackgroundPill({
  kind,
  className,
}: {
  kind: BackgroundWorkKind;
  className?: string;
}) {
  const intl = useIntl();
  const label = intl.formatMessage(i18n.backgroundLabel, {
    work: backgroundWorkLabel(intl, kind),
  });
  return (
    <span
      data-testid="session-background-pill"
      data-work={kind}
      title={label}
      aria-label={label}
      className={cx(PILL, RADIUS.pill, TONE_FILL.secondary, className)}
    >
      <span aria-hidden className="size-1.5 rounded-full bg-current" />
      {backgroundWorkShort(intl, kind)}
    </span>
  );
}

interface LoopPillFace {
  text: string;
  label: string;
  tone: Tone;
}

/** The viewer's own UTC offset at that moment, so "Next 22:40" is the time on their clock. */
function localClock(ms: number): ReturnType<typeof clockTime> {
  return clockTime(ms, -new Date(ms).getTimezoneOffset());
}

function loopPillFace(
  intl: IntlShape,
  status: LoopStatus | undefined,
  nextTickAt: string | undefined,
  error: string | undefined
): LoopPillFace | null {
  const looping = (label: string): LoopPillFace => ({
    text: intl.formatMessage(i18n.looping),
    label,
    tone: 'accent',
  });
  if (error !== undefined) {
    return {
      text: intl.formatMessage(i18n.loopUnreadable),
      // The engine's own sentence ("The loop record could not be read: …"), verbatim.
      label: error,
      tone: 'err',
    };
  }
  switch (status) {
    case undefined:
    case 'ended':
      return null;
    case 'waiting': {
      if (nextTickAt === undefined) {
        return looping(intl.formatMessage(i18n.loopLabelNextMissing));
      }
      const parsed = parseTime(nextTickAt);
      const time = parsed.ok ? localClock(parsed.value) : parsed;
      if (!time.ok) {
        return looping(intl.formatMessage(i18n.loopLabelNextUnreadable, { error: time.error }));
      }
      return {
        text: intl.formatMessage(i18n.loopNext, { time: time.value }),
        label: intl.formatMessage(i18n.loopLabelNext, { time: time.value }),
        tone: 'accent',
      };
    }
    case 'running':
      return looping(intl.formatMessage(i18n.loopLabelRunning));
    case 'checking':
      return looping(intl.formatMessage(i18n.loopLabelChecking));
    case 'waiting_turn':
      return looping(intl.formatMessage(i18n.loopLabelWaitingTurn));
    case 'needs_you':
      return looping(intl.formatMessage(i18n.loopLabelNeedsYou));
    case 'elsewhere':
      return looping(intl.formatMessage(i18n.loopLabelElsewhere));
    case 'waiting_you':
      return {
        text: intl.formatMessage(i18n.loopWaitingYou),
        label: intl.formatMessage(i18n.loopLabelWaitingYou),
        tone: 'warn',
      };
    case 'paused':
      return {
        text: intl.formatMessage(i18n.loopPaused),
        label: intl.formatMessage(i18n.loopLabelPaused),
        tone: 'stopped',
      };
  }
}

/**
 * The chat's loop between its ticks (session loops §8.6), in the Background pill's grammar: a
 * solid fill, an icon, one or two words. Accent "Looping" / "Next 22:40"; stopped-fill "Paused";
 * warn "Waiting for you" when a self-paced tick named no delay (not a question, so not Active
 * now); err "Loop unreadable" when the record could not be read. Nothing for an ended loop.
 */
export function LoopingPill({
  status,
  nextTickAt,
  error,
  className,
}: {
  status?: LoopStatus;
  nextTickAt?: string;
  error?: string;
  className?: string;
}) {
  const intl = useIntl();
  const face = loopPillFace(intl, status, nextTickAt, error);
  if (!face) return null;
  return (
    <span
      data-testid="session-looping-pill"
      data-loop={error !== undefined ? 'unreadable' : status}
      title={face.label}
      aria-label={face.label}
      className={cx(PILL, RADIUS.pill, TONE_FILL[face.tone], TNUM, '[&_svg]:size-3', className)}
    >
      <Repeat aria-hidden />
      {face.text}
    </span>
  );
}

/** Solid amber: the session is waiting on the person. */
export function NeedsYouPill({ count, className }: { count: number; className?: string }) {
  const intl = useIntl();
  return (
    <span
      data-testid="session-needs-you-pill"
      title={intl.formatMessage(i18n.needsYouLabel, { count })}
      aria-label={intl.formatMessage(i18n.needsYouLabel, { count })}
      className={cx(PILL, RADIUS.pill, TONE_FILL.warn, TNUM, '[&_svg]:size-3', className)}
    >
      <Hand aria-hidden />
      {count > 1
        ? intl.formatMessage(i18n.needsYouCount, { count })
        : intl.formatMessage(i18n.needsYou)}
    </span>
  );
}

/** Solid red: the session's last turn failed and nothing has run since. */
export function FailedPill({ reason, className }: { reason?: string; className?: string }) {
  const intl = useIntl();
  const label = reason
    ? intl.formatMessage(i18n.failedLabel, { reason })
    : intl.formatMessage(i18n.failedLabelBare);
  return (
    <span
      data-testid="session-failed-pill"
      title={label}
      aria-label={label}
      className={cx(PILL, RADIUS.pill, TONE_FILL.err, '[&_svg]:size-3', className)}
    >
      <TriangleAlert aria-hidden />
      {intl.formatMessage(i18n.failed)}
    </span>
  );
}

/** Solid slate: the person stopped the session's last turn and nothing has run since (Q-169). */
export function StoppedPill({
  elapsedMs,
  outputTokens,
  className,
}: {
  elapsedMs: number;
  outputTokens?: number;
  className?: string;
}) {
  const intl = useIntl();
  const label = stoppedTurnText(intl, elapsedMs, outputTokens);
  return (
    <span
      data-testid="session-stopped-pill"
      title={label}
      aria-label={label}
      className={cx(PILL, RADIUS.pill, TONE_FILL.stopped, '[&_svg]:size-3', className)}
    >
      <CircleStop aria-hidden />
      {intl.formatMessage(i18n.stopped)}
    </span>
  );
}

/**
 * The row attributes every session list carries, from the one store: `data-state` (space-separated
 * when two hold, e.g. "needs-you running") and `aria-busy` while a turn runs or goose still works
 * for the session after it (Q-185).
 */
export function useSessionStateAttrs(sessionId: string): {
  'data-state': string;
  'aria-busy': true | undefined;
} {
  const activity = useActivityOf(sessionId);
  const states = sessionStates(activity);
  return {
    'data-state': states.join(' '),
    'aria-busy': states.includes('running') || states.includes('background') ? true : undefined,
  };
}

/**
 * What a session row shows about its session RIGHT NOW, from the one activity store: needs-you,
 * running, background, looping, failed or stopped pills, or — when the session is idle — `idle`
 * (the row's usual "27m ago").
 */
export function SessionActivityMarker({
  sessionId,
  idle,
  className,
}: {
  sessionId: string;
  idle?: ReactNode;
  className?: string;
}) {
  const activity = useActivityOf(sessionId);
  const states = sessionStates(activity);
  if (states[0] === 'idle') return <>{idle ?? null}</>;
  return (
    <span
      data-testid="session-activity-marker"
      className={cx('inline-flex shrink-0 items-center gap-1', className)}
    >
      {states.includes('needs-you') && <NeedsYouPill count={activity.needsYou} />}
      {activity.runningSince && <RunningPill since={activity.runningSince} />}
      {states.includes('background') && activity.background && (
        <BackgroundPill kind={activity.background} />
      )}
      {states.includes('looping') && (
        <LoopingPill
          status={activity.loopStatus}
          nextTickAt={activity.loopNextTickAt}
          error={activity.loopError}
        />
      )}
      {states.includes('failed') && <FailedPill reason={activity.failedReason} />}
      {states.includes('stopped') && activity.stoppedElapsedMs !== undefined && (
        <StoppedPill
          elapsedMs={activity.stoppedElapsedMs}
          outputTokens={activity.stoppedOutputTokens}
        />
      )}
    </span>
  );
}
