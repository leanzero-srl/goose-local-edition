import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import type { TurnWait } from './chatServedBy';

const i18n = defineMessages({
  leaving: {
    id: 'turnWait.leaving',
    defaultMessage:
      'Queued behind {rows, plural, one {# stopped request} other {# stopped requests}} still leaving the engine{since, select, none {} other { · stopped {since} ago}}',
  },
  room: {
    id: 'turnWait.room',
    defaultMessage: 'Queued until the running requests finish — no memory room to join them',
  },
});

/**
 * Why this chat's queued turn waits, in the chip's status line (Q-238): "Queued behind 3 stopped
 * requests still leaving the engine · stopped 4s ago" — every figure the split's rank 0 reported.
 */
export function turnWaitText(intl: IntlShape, wait: TurnWait): string {
  if (wait.kind === 'room') return intl.formatMessage(i18n.room);
  return intl.formatMessage(i18n.leaving, {
    rows: wait.rows,
    since: wait.sinceStopS != null ? formatElapsed(wait.sinceStopS) : 'none',
  });
}
