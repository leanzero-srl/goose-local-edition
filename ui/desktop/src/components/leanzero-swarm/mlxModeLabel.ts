import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { MlxModeSummary } from './mlxDistributed';
import { modelShortName } from '../../utils/modelShortName';

/**
 * The one sentence that says which engine this Mac runs — on the state tile, the Engine tab's
 * section row and the split's section — so the three never disagree. The split is called one
 * thing everywhere a person reads it: "Split across N Macs", "the split" (Q-155).
 */
const i18n = defineMessages({
  single: { id: 'mlxMode.single', defaultMessage: 'Single · this Mac' },
  singlePeer: { id: 'mlxMode.singlePeer', defaultMessage: 'Single engine on {host}' },
  remote: { id: 'mlxMode.remote', defaultMessage: 'Serving from {peer}' },
  distributed: {
    id: 'mlxMode.distributed',
    defaultMessage: 'Split across {count, plural, one {# Mac} other {# Macs}} · {backend}',
  },
  distributedNoBackend: {
    id: 'mlxMode.distributedNoBackend',
    defaultMessage: 'Split across {count, plural, one {# Mac} other {# Macs}}',
  },
  hosting: {
    id: 'mlxMode.hosting',
    defaultMessage: "Rank {rank} of {requester}'s split · {model} · {backend}",
  },
  hostingNoBackend: {
    id: 'mlxMode.hostingNoBackend',
    defaultMessage: "Rank {rank} of {requester}'s split · {model}",
  },
  overThunderbolt: { id: 'mlxMode.overThunderbolt', defaultMessage: 'over Thunderbolt' },
  overNetwork: { id: 'mlxMode.overNetwork', defaultMessage: 'over the network' },
});

/**
 * The link a split runs over, in plain words: "over Thunderbolt", never "JACCL" on a surface a
 * person reads (Q-174); the transport's name stays in the split's details. An id this build does
 * not know is shown as sent.
 */
export function linkText(intl: IntlShape, backend: string | null | undefined): string | null {
  if (!backend) return null;
  if (backend === 'jaccl') return intl.formatMessage(i18n.overThunderbolt);
  if (backend === 'ring') return intl.formatMessage(i18n.overNetwork);
  return backend;
}

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
  const backend = linkText(intl, summary.backend);
  if (summary.mode === 'hosting') {
    const values = {
      rank: summary.rank,
      requester: summary.requester,
      model: modelShortName(summary.modelId),
    };
    return backend
      ? intl.formatMessage(i18n.hosting, { ...values, backend })
      : intl.formatMessage(i18n.hostingNoBackend, values);
  }
  const count = summary.nodeNames.length;
  return backend
    ? intl.formatMessage(i18n.distributed, { count, backend })
    : intl.formatMessage(i18n.distributedNoBackend, { count });
}

/** The mode while this Mac's chat is served by the single engine on a linked Mac. */
export function formatRemoteMode(intl: IntlShape, peerName: string): string {
  return intl.formatMessage(i18n.remote, { peer: peerName });
}
