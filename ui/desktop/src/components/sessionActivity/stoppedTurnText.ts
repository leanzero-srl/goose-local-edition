import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { compactTokens } from '../leanzero-swarm/mlxLiveStats';

const i18n = defineMessages({
  withTokens: {
    id: 'stoppedTurn.withTokens',
    defaultMessage: 'You stopped this answer after {elapsed} · {tokens} tokens',
  },
  nothingWritten: {
    id: 'stoppedTurn.nothingWritten',
    defaultMessage: 'You stopped this answer after {elapsed}, before the model wrote anything',
  },
  uncounted: {
    id: 'stoppedTurn.uncounted',
    defaultMessage: 'You stopped this answer after {elapsed}',
  },
  seconds: { id: 'stoppedTurn.seconds', defaultMessage: '{seconds} s' },
  minutes: { id: 'stoppedTurn.minutes', defaultMessage: '{minutes} min' },
  hours: { id: 'stoppedTurn.hours', defaultMessage: '{hours} h {minutes} min' },
});

function elapsedWords(intl: IntlShape, elapsedMs: number): string {
  const seconds = Math.floor(elapsedMs / 1000);
  if (seconds < 60) return intl.formatMessage(i18n.seconds, { seconds });
  if (seconds < 3600)
    return intl.formatMessage(i18n.minutes, { minutes: Math.floor(seconds / 60) });
  return intl.formatMessage(i18n.hours, {
    hours: Math.floor(seconds / 3600),
    minutes: Math.floor((seconds % 3600) / 60),
  });
}

/**
 * "You stopped this answer after 6 min · 1.9k tokens" (Q-169), from the engine's measured wall time
 * and output tokens. The engine writes the same words (turn_outcome.rs `stopped_line`) for clients
 * that render none of their own.
 */
export function stoppedTurnText(
  intl: IntlShape,
  elapsedMs: number,
  outputTokens: number | undefined
): string {
  const elapsed = elapsedWords(intl, elapsedMs);
  if (outputTokens === undefined) return intl.formatMessage(i18n.uncounted, { elapsed });
  if (outputTokens === 0) return intl.formatMessage(i18n.nothingWritten, { elapsed });
  return intl.formatMessage(i18n.withTokens, { elapsed, tokens: compactTokens(outputTokens) });
}

/** The stopped turn a chat notice carries (the adapter keeps it on the notice's `data`). */
export interface StoppedTurnData {
  kind: 'turnStopped';
  elapsedMs: number;
  outputTokens?: number;
}

export function stoppedTurnOf(data: unknown): StoppedTurnData | undefined {
  if (typeof data !== 'object' || data === null) return undefined;
  const candidate = data as Partial<StoppedTurnData>;
  return candidate.kind === 'turnStopped' && typeof candidate.elapsedMs === 'number'
    ? (candidate as StoppedTurnData)
    : undefined;
}
