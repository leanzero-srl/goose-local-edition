import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { MlxModeSummary } from './mlxDistributed';

/**
 * The one sentence that says which engine this Mac runs — on the state tile, the Engine tab's
 * section row and the Distributed section — so the three never disagree.
 */
const i18n = defineMessages({
  single: { id: 'mlxMode.single', defaultMessage: 'Single · this Mac' },
  singlePeer: { id: 'mlxMode.singlePeer', defaultMessage: 'Single engine on {host}' },
  remote: { id: 'mlxMode.remote', defaultMessage: 'Serving from {peer}' },
  distributed: {
    id: 'mlxMode.distributed',
    defaultMessage: 'Distributed · {count, plural, one {# node} other {# nodes}} · {backend}',
  },
  distributedNoBackend: {
    id: 'mlxMode.distributedNoBackend',
    defaultMessage: 'Distributed · {count, plural, one {# node} other {# nodes}}',
  },
  hosting: {
    id: 'mlxMode.hosting',
    defaultMessage: "Rank {rank} of {requester}'s distributed engine · {model} · {backend}",
  },
  hostingNoBackend: {
    id: 'mlxMode.hostingNoBackend',
    defaultMessage: "Rank {rank} of {requester}'s distributed engine · {model}",
  },
});

/** The distributed run's and each rank's state words (the backend's own vocabulary). */
const STATE_WORDS = defineMessages({
  stopped: { id: 'mlxMode.state.stopped', defaultMessage: 'Stopped' },
  preflight: { id: 'mlxMode.state.preflight', defaultMessage: 'Preflight' },
  starting: { id: 'mlxMode.state.starting', defaultMessage: 'Starting' },
  loading: { id: 'mlxMode.state.loading', defaultMessage: 'Loading' },
  ready: { id: 'mlxMode.state.ready', defaultMessage: 'Ready' },
  serving: { id: 'mlxMode.state.serving', defaultMessage: 'Serving' },
  failed: { id: 'mlxMode.state.failed', defaultMessage: 'Failed' },
  stopping: { id: 'mlxMode.state.stopping', defaultMessage: 'Stopping' },
});

/** A state the backend added after this build is shown as sent, never mapped onto a known word. */
export function distributedStateWord(intl: IntlShape, state: string): string {
  const message = (STATE_WORDS as Record<string, (typeof STATE_WORDS)['stopped'] | undefined>)[
    state
  ];
  return message ? intl.formatMessage(message) : state;
}

/**
 * `peerHost` = a linked device is selected in "Manage on": the tile then shows THAT device's single
 * engine, and the distributed status (supervised by this Mac) says nothing about it.
 */
export function formatMlxMode(
  intl: IntlShape,
  summary: MlxModeSummary,
  peerHost: string | null
): string {
  if (peerHost != null) return intl.formatMessage(i18n.singlePeer, { host: peerHost });
  if (summary.mode === 'single') return intl.formatMessage(i18n.single);
  if (summary.mode === 'hosting') {
    const values = {
      rank: summary.rank,
      requester: summary.requester,
      model: summary.modelId.split('/').pop() || summary.modelId,
    };
    return summary.backend
      ? intl.formatMessage(i18n.hosting, { ...values, backend: summary.backend })
      : intl.formatMessage(i18n.hostingNoBackend, values);
  }
  const count = summary.nodeNames.length;
  return summary.backend
    ? intl.formatMessage(i18n.distributed, { count, backend: summary.backend })
    : intl.formatMessage(i18n.distributedNoBackend, { count });
}

/** The mode while this Mac's chat is served by the single engine on a linked Mac. */
export function formatRemoteMode(intl: IntlShape, peerName: string): string {
  return intl.formatMessage(i18n.remote, { peer: peerName });
}
