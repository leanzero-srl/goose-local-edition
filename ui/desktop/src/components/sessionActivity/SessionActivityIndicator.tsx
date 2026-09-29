import { useNavigate } from 'react-router-dom';
import { Hand, Mail } from 'lucide-react';
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
import { noteWords } from '../notes/noteWords';
import { projectLabel, useProjectNames } from '../../utils/projectNames';
import { needsYouCountLabel } from './needsYouWords';
import {
  activeSessions,
  elapsedLabel,
  sessionHref,
  useSessionActivity,
  type ActiveSession,
} from './sessionActivityStore';

const i18n = defineMessages({
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

/**
 * Q-324: at a narrow window (460 px measured) the pill's words took the top bar and left the chat
 * title "J…". Below the app's narrow-layout width (the one TickRow and SessionRail fold at) the pill
 * keeps its icon and the count only; the words stay its accessible name.
 */
const WORDS = 'max-[560px]:hidden';
const COUNT_ONLY = 'hidden max-[560px]:inline';

interface GroupProps {
  testId: string;
  label: string;
  count: number;
  menuLabel: string;
  fill: string;
  icon: React.ReactNode;
  sessions: ActiveSession[];
  detail: (session: ActiveSession) => string;
}

/** One solid pill per state. One session: the pill jumps straight there. More: it lists them. */
function ActivityGroup({
  testId,
  label,
  count,
  menuLabel,
  fill,
  icon,
  sessions,
  detail,
}: GroupProps) {
  const intl = useIntl();
  const navigate = useNavigate();
  const projectName = useProjectNames(sessions.map((s) => s.workingDir).filter(Boolean));
  const nameOf = (s: ActiveSession) =>
    s.sessionName ? displaySessionListName(s.sessionName) : intl.formatMessage(i18n.unnamed);

  const face = (
    <>
      {icon}
      <span data-testid={`${testId}-words`} className={WORDS}>
        {label}
      </span>
      <span data-testid={`${testId}-count`} aria-hidden className={COUNT_ONLY}>
        {count}
      </span>
    </>
  );

  if (sessions.length === 1) {
    const only = sessions[0];
    return (
      <button
        type="button"
        data-testid={testId}
        title={`${nameOf(only)} — ${detail(only)}`}
        aria-label={label}
        onClick={() => navigate(sessionHref(only.sessionId))}
        className={cx(PILL_BUTTON, fill)}
      >
        {face}
      </button>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          data-testid={testId}
          aria-label={label}
          className={cx(PILL_BUTTON, fill)}
        >
          {face}
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
              {[s.workingDir ? projectLabel(projectName(s.workingDir)) : '', detail(s)]
                .filter(Boolean)
                .join(' · ')}
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
  const noted = active.filter((s) => s.notesWaiting > 0);
  if (waiting.length === 0 && running.length === 0 && noted.length === 0) return null;
  const waitingCount = waiting.reduce((n, s) => n + s.needsYou, 0);
  const notesCount = noted.reduce((n, s) => n + s.notesWaiting, 0);

  return (
    <div data-testid="session-activity-indicator" className="flex items-center gap-1.5">
      {waiting.length > 0 && (
        <ActivityGroup
          testId="indicator-needs-you"
          label={needsYouCountLabel(intl, { questions: waitingCount, chats: waiting.length })}
          count={waitingCount}
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
          count={running.length}
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
      {noted.length > 0 && (
        <ActivityGroup
          testId="indicator-notes"
          label={intl.formatMessage(noteWords.waiting, { count: notesCount })}
          count={notesCount}
          menuLabel={intl.formatMessage(noteWords.waitingMenu)}
          fill={TONE_FILL.secondary}
          icon={<Mail aria-hidden />}
          sessions={noted}
          detail={(s) =>
            s.noteFrom ? intl.formatMessage(noteWords.waitingFrom, { from: s.noteFrom }) : ''
          }
        />
      )}
    </div>
  );
}
