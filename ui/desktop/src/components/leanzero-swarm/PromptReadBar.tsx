import type { ReactNode } from 'react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { PHASE_DOT, RADIUS, cx } from '../lz';
import { formatElapsed } from './mlxLiveStats';
import type { PromptCache, ReadBar } from './engineFigures';

/**
 * A prompt read as every surface draws it (Q-337): ONE bar in two parts — what the prefix cache
 * supplied (already in the engine's memory, full at once, teal) and, after it, the new tokens read
 * so far (the reading colour) — and the words that say the same split. The figures are
 * engineFigures.ts's (`promptCacheOf`, `readBarOf`); nothing here derives one.
 *
 * `paint`: `surface` on a page or card (the chat's working row); `fill` on a solid engine-phase fill
 * (the Engine tile, the sidebar glance), where the read part is the fill's own white ink.
 */
export type ReadPaint = 'surface' | 'fill';

export const promptReadMessages = defineMessages({
  split: {
    id: 'promptRead.split',
    defaultMessage:
      '{total} tokens · <c>{cached} from cache</c> · <n>{fresh} new</n> — reading the new part',
  },
  splitShort: {
    id: 'promptRead.splitShort',
    defaultMessage: '<c>{cached} from cache</c> · <n>{fresh} new</n> — reading the new part',
  },
  cold: {
    id: 'promptRead.cold',
    defaultMessage: '{total} tokens · nothing cached — reading all of it',
  },
  coldShort: { id: 'promptRead.coldShort', defaultMessage: 'nothing cached — reading all of it' },
  freshOf: { id: 'promptRead.freshOf', defaultMessage: '{done} of {fresh} new read' },
  soFar: { id: 'promptRead.soFar', defaultMessage: '{elapsed} so far' },
  why: {
    id: 'promptRead.why',
    defaultMessage:
      'goose keeps the start of this conversation in the engine’s memory; only the new part is read again',
  },
  barText: {
    id: 'promptRead.barText',
    defaultMessage: '{cached} from cache, {done} of {fresh} new read',
  },
});

const SEGMENT: Record<ReadPaint, { track: string; cached: string; read: string }> = {
  surface: {
    track: 'bg-lz-surface border border-lz-border-strong',
    cached: 'bg-lz-cache',
    read: PHASE_DOT.reading,
  },
  fill: { track: 'border border-current', cached: 'bg-lz-cache-on-fill', read: 'bg-current' },
};

function compact(intl: IntlShape, n: number): string {
  return intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });
}

/**
 * How much of the prompt is in the engine, cached and read, as a whole percent — rounded DOWN: a
 * 114,948-token read 576 tokens into its 1,124 new ones is 99.5% in and must not say "100%".
 */
export function filledPercent(bar: ReadBar): number {
  return Math.floor(Math.min(1, bar.cached + bar.read) * 100);
}

function percent(fraction: number): string {
  return `${(Math.min(1, Math.max(0, fraction)) * 100).toFixed(2)}%`;
}

export function PromptReadBar({
  bar,
  cache,
  paint,
  label,
  height,
  testId,
}: {
  bar: ReadBar;
  cache: PromptCache | null;
  paint: ReadPaint;
  label: string;
  height: string;
  testId: string;
}) {
  const intl = useIntl();
  const paints = SEGMENT[paint];
  const filled = filledPercent(bar);
  const valueText =
    cache && cache.cached > 0 && cache.freshDone != null
      ? intl.formatMessage(promptReadMessages.barText, {
          cached: compact(intl, cache.cached),
          done: compact(intl, Math.min(cache.fresh, cache.freshDone)),
          fresh: compact(intl, cache.fresh),
        })
      : undefined;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={filled}
      aria-valuetext={valueText}
      data-testid={testId}
      data-cached={cache ? cache.cached : undefined}
      className={cx('flex w-full overflow-hidden', height, RADIUS.pill, paints.track)}
    >
      {bar.cached > 0 && (
        <div
          data-testid="prompt-read-cached"
          className={cx('h-full shrink-0', paints.cached)}
          style={{ width: percent(bar.cached) }}
        />
      )}
      <div
        data-testid="prompt-read-new"
        className={cx('h-full shrink-0', paints.read)}
        style={{ width: percent(bar.read) }}
      />
    </div>
  );
}

/** A small solid dot in a bar part's colour, before the words that name that part: the legend. */
function Swatch({ paint, part }: { paint: ReadPaint; part: 'cached' | 'read' }) {
  return (
    <span
      aria-hidden
      data-testid={`prompt-read-swatch-${part}`}
      className={cx('mr-1 inline-block size-2.5 align-baseline', RADIUS.pill, SEGMENT[paint][part])}
    />
  );
}

/**
 * The split in words: "114.9K tokens · 113.8K from cache · 1.1K new — reading the new part" (each
 * part after its swatch), "114.9K tokens · nothing cached — reading all of it" when the cache
 * supplied none. `short` leaves the size out where the surface already leads with it.
 */
export function promptCacheWords(
  intl: IntlShape,
  cache: PromptCache,
  paint: ReadPaint,
  short: boolean
): ReactNode {
  const total = compact(intl, cache.total);
  if (cache.cached === 0) {
    return intl.formatMessage(short ? promptReadMessages.coldShort : promptReadMessages.cold, {
      total,
    });
  }
  return intl.formatMessage(short ? promptReadMessages.splitShort : promptReadMessages.split, {
    total,
    cached: compact(intl, cache.cached),
    fresh: compact(intl, cache.fresh),
    c: (chunks: ReactNode[]) => (
      <span key="c" className="whitespace-nowrap">
        <Swatch paint={paint} part="cached" />
        {chunks}
      </span>
    ),
    n: (chunks: ReactNode[]) => (
      <span key="n" className="whitespace-nowrap">
        <Swatch paint={paint} part="read" />
        {chunks}
      </span>
    ),
  });
}

/** "576 of 1.1K new read" — how far into the new part, when the engine reports a position. */
export function freshReadText(intl: IntlShape, cache: PromptCache): string | null {
  if (cache.cached === 0 || cache.freshDone == null) return null;
  return intl.formatMessage(promptReadMessages.freshOf, {
    done: compact(intl, Math.min(cache.fresh, cache.freshDone)),
    fresh: compact(intl, cache.fresh),
  });
}

/** "3s so far" — a read the engine reports no position for (the single engine). */
export function soFarText(intl: IntlShape, elapsedS: number): string {
  return intl.formatMessage(promptReadMessages.soFar, { elapsed: formatElapsed(elapsedS) });
}
