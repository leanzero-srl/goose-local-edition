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
  reasonCancelled: {
    id: 'leavingRows.reasonCancelled',
    defaultMessage: 'Cancelled by its caller',
  },
  reasonToolRepeat: {
    id: 'leavingRows.reasonToolRepeat',
    defaultMessage: 'Stopped by the engine: it repeated the same tool call',
  },
  reasonTextCycle: {
    id: 'leavingRows.reasonTextCycle',
    defaultMessage: 'Stopped by the engine: its answer went in circles',
  },
  reasonOther: { id: 'leavingRows.reasonOther', defaultMessage: 'Stopped: {reason}' },
});

/**
 * Why a row's answer was stopped, from rank 0's named stop (Q-231 `stopped.reason`, the
 * `last_engine_stop` shape): the three the engine names today in words, any other by its name.
 */
export function stopReasonText(intl: IntlShape, reason: string): string {
  switch (reason) {
    case 'cancelled_by_client':
      return intl.formatMessage(leavingRowsMessages.reasonCancelled);
    case 'tool_call_repeated':
      return intl.formatMessage(leavingRowsMessages.reasonToolRepeat);
    case 'text_cycle':
      return intl.formatMessage(leavingRowsMessages.reasonTextCycle);
    default:
      return intl.formatMessage(leavingRowsMessages.reasonOther, { reason });
  }
}

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
