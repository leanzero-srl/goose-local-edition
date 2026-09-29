import type { IntlShape } from 'react-intl';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, RADIUS, SURFACE, SectionHeader, TYPE, WEIGHT, cx } from '../lz';
import { displaySessionListName } from '../../sessions';
import { NeedsYouPill, NotePill, RunningPill } from './ActivityPills';
import { noteWords } from '../notes/noteWords';
import {
  activeSessions,
  sessionHref,
  useSessionActivity,
  type ActiveSession,
} from './sessionActivityStore';
import { projectLabel, useProjectNames } from '../../utils/projectNames';

const i18n = defineMessages({
  title: { id: 'activeNowSection.title', defaultMessage: 'Active now' },
  unnamed: { id: 'activeNowSection.unnamed', defaultMessage: 'Untitled session' },
  startedAt: { id: 'activeNowSection.startedAt', defaultMessage: 'started {time}' },
  waitingSince: {
    id: 'activeNowSection.waitingSince',
    defaultMessage: 'waiting for your answer since {time}',
  },
  waiting: { id: 'activeNowSection.waiting', defaultMessage: 'waiting for your answer' },
});

const clock = (intl: IntlShape, iso: string) =>
  intl.formatTime(Date.parse(iso), { hour: '2-digit', minute: '2-digit' });

/**
 * THE one derivation of an Active now row's second line (Q-484): the folder, then what the chat is
 * doing — waiting for the person's answer (since when the oldest question was asked) or running a
 * turn (since when it started) — then any notes waiting. Never the question's own words: they are
 * the chat's content, a raw excerpt with paths in it, and the card in the chat shows them whole.
 */
export function activeRowDetail(intl: IntlShape, row: ActiveSession, project: string): string {
  const doing =
    row.needsYou > 0
      ? row.waitingSince
        ? intl.formatMessage(i18n.waitingSince, { time: clock(intl, row.waitingSince) })
        : intl.formatMessage(i18n.waiting)
      : row.runningSince
        ? intl.formatMessage(i18n.startedAt, { time: clock(intl, row.runningSince) })
        : '';
  const notes =
    row.notesWaiting > 0 ? intl.formatMessage(noteWords.waiting, { count: row.notesWaiting }) : '';
  return [project, doing, notes].filter(Boolean).join(' · ');
}

export default function ActiveNowSection({ className }: { className?: string }) {
  const intl = useIntl();
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const openSessionId = location.pathname === '/pair' ? searchParams.get('resumeSessionId') : null;
  const rows = activeSessions(useSessionActivity());
  const projectName = useProjectNames(rows.map((row) => row.workingDir).filter(Boolean));
  if (rows.length === 0) return null;

  return (
    <section data-testid="active-now-section" className={cx('flex flex-col px-2', className)}>
      <SectionHeader title={intl.formatMessage(i18n.title)} count={rows.length} className="px-2" />
      {/* Q-483: rows are two-line cards, so they get a real gap — the tree's 1px gap let one row's
          ring sit on its neighbour. */}
      <div data-testid="active-now-rows" className="flex flex-col gap-1.5">
        {rows.map((row) => {
          const name = row.sessionName
            ? displaySessionListName(row.sessionName)
            : intl.formatMessage(i18n.unnamed);
          const detail = activeRowDetail(
            intl,
            row,
            row.workingDir ? projectLabel(projectName(row.workingDir)) : ''
          );
          const current = row.sessionId === openSessionId;
          return (
            <button
              key={row.sessionId}
              type="button"
              data-testid={`active-now-row-${row.sessionId}`}
              data-state={[
                row.needsYou > 0 && 'needs-you',
                row.runningSince && 'running',
                row.notesWaiting > 0 && 'note',
              ]
                .filter(Boolean)
                .join(' ')}
              aria-busy={row.runningSince ? true : undefined}
              aria-current={current ? 'true' : undefined}
              title={`${name}${detail ? ` — ${detail}` : ''}`}
              onClick={() => navigate(sessionHref(row.sessionId))}
              className={cx(
                // Two lines (name + folder/state), so not the tree's fixed-height dense row.
                'flex w-full flex-col items-stretch gap-1 px-2.5 py-2 text-left',
                RADIUS.control,
                MOTION,
                FOCUS,
                current ? SURFACE.selectedRing : SURFACE.hover
              )}
            >
              {/* The title keeps at least 10rem and truncates before the badges; where the row is
                  narrower than that plus the badges, the badges wrap under the title instead of
                  squeezing it. */}
              <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
                <span
                  data-testid="active-now-title"
                  className={cx(
                    'min-w-0 grow basis-40 truncate text-lz-body text-lz-ink',
                    WEIGHT.semibold
                  )}
                >
                  {name}
                </span>
                <span data-testid="active-now-badges" className="flex shrink-0 items-center gap-1">
                  {row.notesWaiting > 0 && (
                    <NotePill count={row.notesWaiting} from={row.noteFrom} />
                  )}
                  {row.needsYou > 0 && <NeedsYouPill count={row.needsYou} />}
                  {row.runningSince && <RunningPill since={row.runningSince} />}
                </span>
              </span>
              {detail && (
                <span data-testid="active-now-detail" className={cx('truncate', TYPE.meta)}>
                  {detail}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </section>
  );
}
