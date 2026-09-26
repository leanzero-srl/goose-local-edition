import { useSyncExternalStore, type ReactNode } from 'react';
import { Hand, TriangleAlert } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { PHASE_FILL, RADIUS, TNUM, TONE_FILL, cx } from '../lz';
import { elapsedLabel, sessionStates, useActivityOf } from './sessionActivityStore';

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
  needsYouCount: { id: 'sessionActivity.needsYouCount', defaultMessage: 'Needs you · {count}' },
  needsYouLabel: {
    id: 'sessionActivity.needsYouLabel',
    defaultMessage:
      '{count, plural, one {Waiting for your answer} other {# questions waiting for your answer}}',
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

/**
 * The row attributes every session list carries, from the one store: `data-state` (space-separated
 * when two hold, e.g. "needs-you running") and `aria-busy` while a turn runs.
 */
export function useSessionStateAttrs(sessionId: string): {
  'data-state': string;
  'aria-busy': true | undefined;
} {
  const activity = useActivityOf(sessionId);
  return {
    'data-state': sessionStates(activity).join(' '),
    'aria-busy': activity.runningSince ? true : undefined,
  };
}

/**
 * What a session row shows about its session RIGHT NOW, from the one activity store: needs-you,
 * running or failed pills, or — when the session is idle — `idle` (the row's usual "27m ago").
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
      {states.includes('failed') && <FailedPill reason={activity.failedReason} />}
    </span>
  );
}
