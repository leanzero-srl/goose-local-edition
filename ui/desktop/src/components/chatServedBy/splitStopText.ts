import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { gb1 } from '../leanzero-swarm/mlxDistributed';
import type { SplitStop } from './splitStop';

const i18n = defineMessages({
  reason: {
    id: 'splitStop.reason',
    defaultMessage:
      '{cause, select, memory {{mac} ran out of memory} frozen {{mac} stopped responding} other {{mac}’s part of the model stopped}}',
  },
  reasonUnnamed: {
    id: 'splitStop.reasonUnnamed',
    defaultMessage:
      '{cause, select, memory {one of your Macs ran out of memory} frozen {the Macs stopped making progress} other {one part of the model stopped}}',
  },
  headline: {
    id: 'splitStop.headline',
    defaultMessage: 'The split across your Macs stopped — {reason}',
  },
  memory: {
    id: 'splitStop.memory',
    defaultMessage: '{mac} had {available} GB of {total} GB free when it stopped.',
  },
});

/** Why the split stopped, in words ("Work’s Mac Studio ran out of memory"). */
export function splitStopReason(intl: IntlShape, stop: SplitStop): string {
  return stop.mac
    ? intl.formatMessage(i18n.reason, { cause: stop.cause, mac: stop.mac })
    : intl.formatMessage(i18n.reasonUnnamed, { cause: stop.cause });
}

/** "The split across your Macs stopped — Work’s Mac Studio ran out of memory". */
export function splitStopHeadline(intl: IntlShape, stop: SplitStop): string {
  return intl.formatMessage(i18n.headline, { reason: splitStopReason(intl, stop) });
}

/** The watchdog's last sample on that Mac, when the stop was for memory and it was read. */
export function splitStopMemory(intl: IntlShape, stop: SplitStop): string | null {
  if (stop.cause !== 'memory' || !stop.mac || stop.availableGb == null || stop.totalGb == null) {
    return null;
  }
  return intl.formatMessage(i18n.memory, {
    mac: stop.mac,
    available: gb1(stop.availableGb),
    total: gb1(stop.totalGb),
  });
}
