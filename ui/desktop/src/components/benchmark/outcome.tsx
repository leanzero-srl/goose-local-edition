import { Chip, RADIUS, TNUM, cx, type Tone } from '../lz';
import type { BenchSession, SessionOutcome } from './bridge';

/** The session's start stamp, short; null when the stamp is unreadable. */
export function fmtWhen(when: string | number | undefined | null): string | null {
  if (when == null || when === '') return null;
  const t = typeof when === 'number' ? when : Date.parse(when);
  if (Number.isNaN(t)) return null;
  return new Date(t).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export const OUTCOME_TONE: Record<SessionOutcome, Tone> = {
  running: 'accent',
  finished: 'ok',
  did_not_finish: 'err',
  did_not_start: 'stopped',
};
export const OUTCOME_WORDS: Record<SessionOutcome, string> = {
  running: 'Running',
  finished: 'Finished',
  did_not_finish: 'Did not finish',
  did_not_start: 'Did not start',
};

export function OutcomeChip({ session }: { session: BenchSession }) {
  return (
    <Chip
      tone={OUTCOME_TONE[session.outcome]}
      icon={
        session.outcome === 'running' ? (
          // DESIGN.md motion: the live dot SCALES (animate-lz-live), it never fades.
          <span className={cx('inline-block size-1.5 animate-lz-live bg-white', RADIUS.pill)} />
        ) : undefined
      }
    >
      {OUTCOME_WORDS[session.outcome]}
      {session.outcome === 'finished' && (
        <span className={TNUM}>
          {session.score != null ? ` · ${(session.score * 100).toFixed(1)}%` : ' · score missing'}
        </span>
      )}
    </Chip>
  );
}
