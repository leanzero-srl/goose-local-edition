import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import type { ChatBusyIn } from './chatServedBy';

const i18n = defineMessages({
  busyIn: {
    id: 'busyIn.headline',
    defaultMessage:
      '{named, select, yes {Busy in ‘{name}’} other {Busy in another chat}}{elapsed, select, none {} other { · {elapsed}}}',
  },
  sendWaits: {
    id: 'busyIn.sendWaits',
    defaultMessage: 'A message sent now waits until the engine has room for it.',
  },
  sendShares: {
    id: 'busyIn.sendShares',
    defaultMessage:
      'A message sent now shares the engine with that answer — it runs slower, or waits if there is no room.',
  },
});

/** "Busy in ‘Jira Migration Kickoff Notes’ · 39m 15s" — the chip's word and the bar's headline (Q-152). */
export function busyInHeadline(intl: IntlShape, busy: ChatBusyIn): string {
  return intl.formatMessage(i18n.busyIn, {
    named: busy.sessionName ? 'yes' : 'no',
    name: busy.sessionName ?? '',
    elapsed: busy.elapsedS != null ? formatElapsed(busy.elapsedS) : 'none',
  });
}

/**
 * What a send does now, from what the engine reports: it holds a request WAITING — a new one waits
 * too; otherwise the engine batches, so a new request runs beside the answer or is held for room
 * (Rapid-MLX batches, Q-40; the split's rank 0 holds a request its batch has no KV room for).
 */
export function busyInSendText(intl: IntlShape, busy: ChatBusyIn): string {
  return intl.formatMessage(busy.waits ? i18n.sendWaits : i18n.sendShares);
}
