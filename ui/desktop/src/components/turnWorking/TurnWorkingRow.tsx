import type { ReactNode } from 'react';
import { LoaderCircle } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { formatElapsed, formatRate } from '../leanzero-swarm/mlxLiveStats';
import { readBarOf } from '../leanzero-swarm/engineFigures';
import {
  PromptReadBar,
  freshReadText,
  promptCacheWords,
  promptReadMessages,
  soFarText,
} from '../leanzero-swarm/PromptReadBar';
import { PHASE_FILL, RADIUS, TNUM, TONE_FILL, TYPE, cx } from '../lz';
import { useNow } from '../sessionActivity/ActivityPills';
import { elapsedLabel, useActivityOf } from '../sessionActivity/sessionActivityStore';
import { useTurnReadOf } from './turnReadStore';

const i18n = defineMessages({
  reading: { id: 'turnWorking.reading', defaultMessage: 'Reading your prompt' },
  readOf: {
    id: 'turnWorking.readOf',
    defaultMessage: '{done} of {total} tokens read',
  },
  readSize: {
    id: 'turnWorking.readSize',
    defaultMessage: '{tokens} prompt tokens, reading for {elapsed}',
  },
  rate: { id: 'turnWorking.rate', defaultMessage: '{rate} tok/s' },
  left: { id: 'turnWorking.left', defaultMessage: 'about {left} left at this rate' },
  progress: { id: 'turnWorking.progress', defaultMessage: 'Prompt read so far' },
  waiting: { id: 'turnWorking.waiting', defaultMessage: 'Waiting for the model’s first words' },
  waitingFor: {
    id: 'turnWorking.waitingFor',
    defaultMessage: 'Waiting for the model’s first words · {elapsed}',
  },
});

const PILL = 'inline-flex h-6 items-center gap-1.5 px-2 text-lz-meta font-lz-semibold';

/**
 * Under the user's message while the turn has produced nothing yet (Q-301: a 40.5K-token prompt
 * was read for 2 m 48 s with the transcript blank — only the chip and the sidebar card showed
 * work). While its prompt is read, the figures the card leads with for it — the split the prefix
 * cache made of it (Q-337: what is already in memory, what is new), how far into the new part, the
 * rate it is read at now, the time left at that rate over the new tokens only — from the
 * composer's one derivation (`promptRead`, published by session); otherwise how long the turn has
 * waited. It goes the moment the model's first words (or thoughts, or a tool call) arrive.
 */
export function TurnWorkingRow({ sessionId }: { sessionId: string }) {
  const intl = useIntl();
  const read = useTurnReadOf(sessionId);
  const { runningSince } = useActivityOf(sessionId);
  const now = useNow();
  const compact = (n: number) =>
    intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });

  if (!read) {
    return (
      <div data-testid="turn-working-row" data-stage="waiting" className="py-2">
        <span className={cx(PILL, RADIUS.pill, TONE_FILL.secondary, TNUM)}>
          <LoaderCircle aria-hidden className="size-3.5 animate-spin" />
          {runningSince
            ? intl.formatMessage(i18n.waitingFor, { elapsed: elapsedLabel(runningSince, now) })
            : intl.formatMessage(i18n.waiting)}
        </span>
      </div>
    );
  }

  const { cache, progress } = read;
  // Into the new part when the cache supplied some; otherwise into the whole prompt.
  const howFar =
    cache && cache.cached > 0
      ? freshReadText(intl, cache)
      : progress
        ? intl.formatMessage(i18n.readOf, {
            done: compact(progress.done),
            total: compact(progress.total),
          })
        : null;
  const figures: ReactNode[] = [
    cache ? promptCacheWords(intl, cache, 'surface', false) : null,
    howFar,
    // The single engine reports no position: its read is said by the time it has taken.
    cache && !progress ? soFarText(intl, read.elapsedS) : null,
    !cache && !progress && read.tokens != null
      ? intl.formatMessage(i18n.readSize, {
          tokens: compact(read.tokens),
          elapsed: formatElapsed(read.elapsedS),
        })
      : null,
    read.tps != null
      ? intl.formatMessage(i18n.rate, { rate: formatRate(read.tps, intl.locale) })
      : null,
    read.leftS != null ? intl.formatMessage(i18n.left, { left: formatElapsed(read.leftS) }) : null,
  ].filter((part) => part != null);
  const bar = readBarOf(progress, cache);

  return (
    <div
      data-testid="turn-working-row"
      data-stage="reading"
      className="flex max-w-md flex-col gap-2 py-2"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={cx(PILL, RADIUS.pill, PHASE_FILL.reading)}>
          <LoaderCircle aria-hidden className="size-3.5 animate-spin" />
          {intl.formatMessage(i18n.reading)}
        </span>
        {figures.length > 0 && (
          <span data-testid="turn-working-figures" className={cx(TYPE.meta, TNUM)}>
            {figures.map((part, i) => (
              <span key={i}>
                {i > 0 && ' · '}
                {part}
              </span>
            ))}
          </span>
        )}
      </div>
      {bar && (
        <PromptReadBar
          bar={bar}
          cache={cache}
          paint="surface"
          label={intl.formatMessage(i18n.progress)}
          height="h-2.5"
          testId="turn-working-progress"
        />
      )}
      {cache && cache.cached > 0 && (
        <span data-testid="turn-working-why" className={TYPE.meta}>
          {intl.formatMessage(promptReadMessages.why)}
        </span>
      )}
    </div>
  );
}
