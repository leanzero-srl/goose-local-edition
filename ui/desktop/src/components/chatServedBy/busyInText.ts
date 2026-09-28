import type { IntlShape } from 'react-intl';
import type { BackgroundWorkKind } from '@aaif/goose-sdk';
import { defineMessages } from '../../i18n';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import type { ChatBusyIn } from './chatServedBy';
import { backgroundWorkLabel } from '../sessionActivity/backgroundWorkText';
import { listedTitleOf } from '../sessionActivity/sessionActivityStore';

const i18n = defineMessages({
  busyIn: {
    id: 'busyIn.headline',
    defaultMessage:
      '{named, select, yes {Busy in ‘{name}’} other {Busy in another chat}}{elapsed, select, none {} other { · {elapsed}}}',
  },
  busyForWork: {
    id: 'busyIn.backgroundHeadline',
    defaultMessage: 'Busy for ‘{name}’: {work}{elapsed, select, none {} other { · {elapsed}}}',
  },
  sendYields: {
    id: 'busyIn.sendYields',
    defaultMessage:
      'A message sent now goes first: goose sets this check aside and runs it again after.',
  },
  sendWaits: {
    id: 'busyIn.sendWaits',
    defaultMessage: 'A message sent now waits until the engine has room for it.',
  },
  sendGoesTo: {
    id: 'busyIn.sendGoesTo',
    defaultMessage: 'A message sent now goes to {node} — it does not wait for that answer.',
  },
  sendLoadsAfter: {
    id: 'busyIn.sendLoadsAfter',
    defaultMessage: 'A message sent now waits for that answer to finish, then loads {node}.',
  },
  sendShares: {
    id: 'busyIn.sendShares',
    defaultMessage:
      'A message sent now shares the engine with that answer — it runs slower, or waits if there is no room.',
  },
});

/**
 * "Busy in ‘Jira Migration Kickoff Notes · 5’ · 39m 15s" — the chip's word and the bar's headline
 * (Q-152); "Busy for ‘…’: Checking the reply · 3s" when goose's own call for that chat is all it
 * runs there (Q-185). The name is the one its sidebar row shows.
 */
export function busyInHeadline(intl: IntlShape, busy: ChatBusyIn): string {
  const name = busy.sessionName ? listedTitleOf(busy.sessionId, busy.sessionName) : '';
  const elapsed = busy.elapsedS != null ? formatElapsed(busy.elapsedS) : 'none';
  if (busy.work && name) {
    return intl.formatMessage(i18n.busyForWork, {
      name,
      work: backgroundWorkLabel(intl, busy.work),
      elapsed,
    });
  }
  return intl.formatMessage(i18n.busyIn, { named: name ? 'yes' : 'no', name, elapsed });
}

/**
 * What a send does now: where the chat's own chain sends it when that is not the busy way
 * (busyNext.ts); otherwise, from what the engine reports: it holds a request WAITING — a new one waits
 * too; otherwise the engine batches, so a new request runs beside the answer or is held for room
 * (Rapid-MLX batches, Q-40; the split's rank 0 holds a request its batch has no KV room for).
 */
export function busyInSendText(intl: IntlShape, busy: ChatBusyIn): string {
  // The chat's own chain decides first (Q-431): a turn that goes elsewhere, or loads its node
  // after the busy answer, never shares the engine with it.
  if (busy.next?.kind === 'goesTo') {
    return intl.formatMessage(i18n.sendGoesTo, { node: busy.next.node });
  }
  if (busy.next?.kind === 'loadsAfter') {
    return intl.formatMessage(i18n.sendLoadsAfter, { node: busy.next.node });
  }
  if (busy.work && YIELDS_TO_A_TURN.has(busy.work)) return intl.formatMessage(i18n.sendYields);
  return intl.formatMessage(busy.waits ? i18n.sendWaits : i18n.sendShares);
}

/**
 * The end-of-turn checks step aside for a user's turn (Q-132, crates/goose/src/turn_priority.rs:
 * a turn that starts drops the check's call; it is asked again once no turn runs).
 */
const YIELDS_TO_A_TURN: ReadonlySet<BackgroundWorkKind> = new Set(['factCheck', 'memoryReview']);
