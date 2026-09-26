import { useNavigate } from 'react-router-dom';
import { Hand } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '../ui/dropdown-menu';
import { FOCUS, MOTION, PHASE_FILL, RADIUS, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { displaySessionListName } from '../../sessions';
import { useNow } from './ActivityPills';
import {
  activeSessions,
  elapsedLabel,
  sessionHref,
  useSessionActivity,
  type ActiveSession,
} from './sessionActivityStore';

const i18n = defineMessages({
  needsYou: {
    id: 'sessionActivityIndicator.needsYou',
    defaultMessage: '{count, plural, one {# needs you} other {# need you}}',
  },
  running: {
    id: 'sessionActivityIndicator.running',
    defaultMessage: '{count} running',
  },
  needsYouMenu: { id: 'sessionActivityIndicator.needsYouMenu', defaultMessage: 'Waiting for you' },
  runningMenu: { id: 'sessionActivityIndicator.runningMenu', defaultMessage: 'Running now' },
  unnamed: { id: 'sessionActivityIndicator.unnamed', defaultMessage: 'Untitled session' },
  runningFor: { id: 'sessionActivityIndicator.runningFor', defaultMessage: 'running · {elapsed}' },
});

const PILL_BUTTON = cx(
  'no-drag inline-flex h-7 items-center gap-1.5 px-2.5 text-[12px] [&_svg]:size-3.5',
  WEIGHT.semibold,
  TNUM,
  RADIUS.pill,
  FOCUS,
  MOTION
);

function folderOf(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? '';
}

interface GroupProps {
  testId: string;
  label: string;
  menuLabel: string;
  fill: string;
  icon: React.ReactNode;
  sessions: ActiveSession[];
  detail: (session: ActiveSession) => string;
}

/** One solid pill per state. One session: the pill jumps straight there. More: it lists them. */
function ActivityGroup({ testId, label, menuLabel, fill, icon, sessions, detail }: GroupProps) {
  const intl = useIntl();
  const navigate = useNavigate();
  const nameOf = (s: ActiveSession) =>
    s.sessionName ? displaySessionListName(s.sessionName) : intl.formatMessage(i18n.unnamed);

  if (sessions.length === 1) {
    const only = sessions[0];
    return (
      <button
        type="button"
        data-testid={testId}
        title={`${nameOf(only)} — ${detail(only)}`}
        onClick={() => navigate(sessionHref(only.sessionId))}
        className={cx(PILL_BUTTON, fill)}
      >
        {icon}
        {label}
      </button>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" data-testid={testId} className={cx(PILL_BUTTON, fill)}>
          {icon}
          {label}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-80" data-testid={`${testId}-menu`}>
        <DropdownMenuLabel className={TYPE.zone}>{menuLabel}</DropdownMenuLabel>
        {sessions.map((s) => (
          <DropdownMenuItem
            key={s.sessionId}
            data-testid={`${testId}-item`}
            onSelect={() => navigate(sessionHref(s.sessionId))}
            className="flex flex-col items-start gap-0.5"
          >
            <span className={cx('w-full truncate text-lz-body text-lz-ink', WEIGHT.semibold)}>
              {nameOf(s)}
            </span>
            <span className="w-full truncate text-lz-meta text-lz-ink-2">
              {[folderOf(s.workingDir), detail(s)].filter(Boolean).join(' · ')}
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * The app-wide count, in the top bar of every view: how many sessions wait on the person and how
 * many have a turn running, each a jump to the session. Nothing shows when nothing is active.
 */
export default function SessionActivityIndicator() {
  const intl = useIntl();
  const now = useNow();
  const active = activeSessions(useSessionActivity());
  const waiting = active.filter((s) => s.needsYou > 0);
  const running = active.filter((s) => s.runningSince);
  if (waiting.length === 0 && running.length === 0) return null;

  return (
    <div data-testid="session-activity-indicator" className="flex items-center gap-1.5">
      {waiting.length > 0 && (
        <ActivityGroup
          testId="indicator-needs-you"
          label={intl.formatMessage(i18n.needsYou, {
            count: waiting.reduce((n, s) => n + s.needsYou, 0),
          })}
          menuLabel={intl.formatMessage(i18n.needsYouMenu)}
          fill={TONE_FILL.warn}
          icon={<Hand aria-hidden />}
          sessions={waiting}
          detail={(s) => s.headline ?? ''}
        />
      )}
      {running.length > 0 && (
        <ActivityGroup
          testId="indicator-running"
          label={intl.formatMessage(i18n.running, { count: running.length })}
          menuLabel={intl.formatMessage(i18n.runningMenu)}
          fill={PHASE_FILL.writing}
          icon={<span aria-hidden className="size-2 animate-lz-live rounded-full bg-current" />}
          sessions={running}
          detail={(s) =>
            s.runningSince
              ? intl.formatMessage(i18n.runningFor, { elapsed: elapsedLabel(s.runningSince, now) })
              : ''
          }
        />
      )}
    </div>
  );
}
