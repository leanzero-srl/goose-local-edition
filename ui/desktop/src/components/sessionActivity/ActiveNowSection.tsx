import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { defineMessages, useIntl } from '../../i18n';
import { FOCUS, MOTION, RADIUS, SURFACE, SectionHeader, TYPE, WEIGHT, cx } from '../lz';
import { displaySessionListName } from '../../sessions';
import { NeedsYouPill, RunningPill } from './ActivityPills';
import { activeSessions, sessionHref, useSessionActivity } from './sessionActivityStore';

const i18n = defineMessages({
  title: { id: 'activeNowSection.title', defaultMessage: 'Active now' },
  unnamed: { id: 'activeNowSection.unnamed', defaultMessage: 'Untitled session' },
  startedAt: { id: 'activeNowSection.startedAt', defaultMessage: 'started {time}' },
});

function folderOf(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? '';
}

/**
 * The sidebar's top block: every session running a turn or waiting on the person, before any
 * folder, so none of them hides behind a collapsed project or "Show more". Each row carries its
 * folder and start time as a second line, so two same-title sessions are never confused.
 */
export default function ActiveNowSection({ className }: { className?: string }) {
  const intl = useIntl();
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams] = useSearchParams();
  const openSessionId =
    location.pathname === '/pair' ? searchParams.get('resumeSessionId') : null;
  const rows = activeSessions(useSessionActivity());
  if (rows.length === 0) return null;

  return (
    <section data-testid="active-now-section" className={cx('flex flex-col px-2', className)}>
      <SectionHeader title={intl.formatMessage(i18n.title)} count={rows.length} className="px-2" />
      <div className="flex flex-col gap-px">
        {rows.map((row) => {
          const name = row.sessionName
            ? displaySessionListName(row.sessionName)
            : intl.formatMessage(i18n.unnamed);
          const started = row.runningSince
            ? intl.formatMessage(i18n.startedAt, {
                time: intl.formatTime(Date.parse(row.runningSince), {
                  hour: '2-digit',
                  minute: '2-digit',
                }),
              })
            : '';
          const detail = [folderOf(row.workingDir), row.needsYou > 0 ? row.headline : started]
            .filter(Boolean)
            .join(' · ');
          const current = row.sessionId === openSessionId;
          return (
            <button
              key={row.sessionId}
              type="button"
              data-testid={`active-now-row-${row.sessionId}`}
              data-state={[row.needsYou > 0 && 'needs-you', row.runningSince && 'running']
                .filter(Boolean)
                .join(' ')}
              aria-busy={row.runningSince ? true : undefined}
              aria-current={current ? 'true' : undefined}
              title={`${name}${detail ? ` — ${detail}` : ''}`}
              onClick={() => navigate(sessionHref(row.sessionId))}
              className={cx(
                // Two lines (name + folder/question), so not the tree's fixed-height dense row.
                'flex w-full flex-col items-stretch gap-0.5 px-2 py-1.5 text-left',
                RADIUS.control,
                MOTION,
                FOCUS,
                current ? SURFACE.selectedRing : SURFACE.hover
              )}
            >
              <span className="flex min-w-0 items-center gap-1.5">
                <span className={cx('min-w-0 flex-1 truncate text-lz-body text-lz-ink', WEIGHT.semibold)}>
                  {name}
                </span>
                {row.needsYou > 0 && <NeedsYouPill count={row.needsYou} />}
                {row.runningSince && <RunningPill since={row.runningSince} />}
              </span>
              {detail && <span className={cx('truncate', TYPE.meta)}>{detail}</span>}
            </button>
          );
        })}
      </div>
    </section>
  );
}
