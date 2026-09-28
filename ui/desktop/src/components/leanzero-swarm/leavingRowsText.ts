import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { formatElapsed, type LeavingRows } from './mlxLiveStats';

/** Exported for a surface that keeps a word table per stage (the glance's `STAGE_WORD`). */
export const leavingRowsMessages = defineMessages({
  stage: { id: 'leavingRows.stage', defaultMessage: 'Stopped · leaving' },
  figure: {
    id: 'leavingRows.figure',
    defaultMessage:
      '{rows, plural, one {stopped request} other {stopped requests}} still leaving the engine{since, select, none {} other { · stopped {since} ago}}',
  },
  stoppedAgo: { id: 'leavingRows.stoppedAgo', defaultMessage: 'stopped {since} ago' },
});

/**
 * The words for rows whose answers already ended but still hold the engine's batch (Q-231
 * `leaving`), shared by the Engine tile and the engine glance so both say them alike (Q-246): never
 * "Reading", which read as work beside the turn they hold up.
 */
export function leavingStageWord(intl: IntlShape): string {
  return intl.formatMessage(leavingRowsMessages.stage);
}

/** The figure: how many rows, and how long ago the one held longest was stopped. */
export function leavingFigureText(
  intl: IntlShape,
  fact: LeavingRows
): { value: string; label: string } {
  return {
    value: intl.formatNumber(fact.rows),
    label: intl.formatMessage(leavingRowsMessages.figure, {
      rows: fact.rows,
      since: fact.sinceStopS != null ? formatElapsed(fact.sinceStopS) : 'none',
    }),
  };
}

/** One row's stop, on the engine's own clock: "stopped 3s ago". */
export function stoppedAgoText(intl: IntlShape, sinceStopS: number): string {
  return intl.formatMessage(leavingRowsMessages.stoppedAgo, { since: formatElapsed(sinceStopS) });
}
