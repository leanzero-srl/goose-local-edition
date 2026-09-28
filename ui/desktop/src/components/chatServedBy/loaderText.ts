import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type {
  MeasuredLoad,
  NodeDisplaced,
  NodeRefusal,
  NodeSwap,
  NodeWait,
} from '../../utils/nodeSwap';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { loadPhaseWord } from '../nodes/loadPhaseWord';

const i18n = defineMessages({
  // DESIGN-NODES-AND-STRATEGIES.md §8.7, as the table words them.
  turnLoading: {
    id: 'nodes.turnLoading',
    defaultMessage: 'Loading {node} for this chat: {phase}',
  },
  turnWaiting: {
    id: 'nodes.turnWaiting',
    defaultMessage:
      'Waiting for {way} to finish {count, plural, one {# reply} other {# replies}}, then loading {node} ({duration})',
  },
  turnFirstLoad: {
    id: 'nodes.turnFirstLoad',
    defaultMessage: 'First load of {node}, not measured yet',
  },
  displacedNotice: {
    id: 'nodes.displacedNotice',
    defaultMessage:
      '{node} was stopped for {other} in chat "{chat}". Your next message loads it back ({duration}).',
  },
  // A Start on a node's card asked for {other}: there is no chat to name.
  displacedNoticeStarted: {
    id: 'nodes.displacedNoticeStarted',
    defaultMessage:
      '{node} was stopped to start {other}. Your next message loads it back ({duration}).',
  },
  displacedFailed: {
    id: 'nodes.displacedFailed',
    defaultMessage:
      '{node} was stopped for {other}, which failed to load: {words}. Your next message loads {node} back.',
  },
  refusedKept: {
    id: 'nodes.refusedKept',
    defaultMessage: "Can't load {node}: {kept} is kept loaded on {mac}.",
  },
  refusedBuild: {
    id: 'nodes.refusedBuild',
    defaultMessage:
      "Can't load {node}: a swarm build is using {way}. It frees when the build ends.",
  },
  refusedFit: {
    id: 'nodes.refusedFit',
    defaultMessage: "Can't load {node} on {mac}: {verdict}",
  },
  loadFailed: { id: 'nodes.loadFailed', defaultMessage: '{node} failed to load: {words}' },
  // The ledger's words for a way the loader stopped (Q-254): what the stopped way reads, on every
  // surface, while the loader loads the node — never "Failed" or "No model is mounted".
  swappingTo: { id: 'nodes.swappingTo', defaultMessage: 'Swapping to {node}' },
  // {duration} in the lines above: the node's measured load, or that it has none yet (design §6.4
  // step 10: no estimate in place of a measurement).
  durationAbout: { id: 'nodes.durationAbout', defaultMessage: 'about {duration}' },
  durationUnmeasured: {
    id: 'nodes.durationUnmeasured',
    defaultMessage: 'first load not measured yet',
  },
  loadMedian: {
    id: 'nodes.turnLoadMedian',
    defaultMessage:
      '{count, plural, one {Loads in about {duration} · # load measured} other {Loads in about {duration} · median of # loads}}',
  },
});

/**
 * The node loader's state as ONE chat sees it (chatServedBy.ts `chatLoaderOf`):
 *  - `loading`: the loader is loading `swap.target`; `forThisChat` when this chat's turn is in flight
 *    and the node is one its model runs on (`node:`/`strategy:`) — the turn waits on this load;
 *  - `waiting`: this chat's turn is queued in the loader for one of its nodes;
 *  - `refused`: the loader refused the last demand for the node this chat's next turn goes to.
 */
export type ChatLoader =
  | { kind: 'loading'; swap: NodeSwap; forThisChat: boolean }
  | { kind: 'waiting'; wait: NodeWait }
  | { kind: 'refused'; refusal: NodeRefusal };

/** "Swapping to {node}" — the words every surface says a way the loader stopped with. */
export function swappingText(intl: IntlShape, node: string): string {
  return intl.formatMessage(i18n.swappingTo, { node });
}

/** A node's load time in the §8.7 lines: "about 1m 38s", or that no load is measured yet. */
export function loadDurationText(intl: IntlShape, load: MeasuredLoad | null | undefined): string {
  return load
    ? intl.formatMessage(i18n.durationAbout, { duration: formatElapsed(load.medianMs / 1000) })
    : intl.formatMessage(i18n.durationUnmeasured);
}

/** The goosed sentence as a headline: only its first letter raised. */
function headline(words: string): string {
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** A refusal in §8.7's words when it names one of theirs, else in the loader's own. */
export function refusalText(intl: IntlShape, refusal: NodeRefusal): string {
  const node = refusal.target.name;
  const facts = refusal.facts;
  switch (facts?.kind) {
    case 'keptLoaded':
      return intl.formatMessage(i18n.refusedKept, { node, kept: facts.kept, mac: facts.mac });
    case 'heldByBuild':
      return intl.formatMessage(i18n.refusedBuild, { node, way: facts.way });
    case 'fit':
      return intl.formatMessage(i18n.refusedFit, { node, mac: facts.mac, verdict: facts.verdict });
    case 'loadFailed':
      return intl.formatMessage(i18n.loadFailed, { node, words: facts.words });
    case undefined:
      return headline(refusal.reason);
  }
}

/**
 * The loader line — the composer bar's headline, the chip's word and its menu line read this one
 * text: "Loading {node} for this chat: {phase}" once the way names its phase, "Swapping to {node}"
 * before it does or for a chat the load is not for; a wait for replies in §8.7's words, any other
 * wait in the loader's own; a refusal in §8.7's words when it is one of theirs.
 */
export function loaderText(intl: IntlShape, loader: ChatLoader): string {
  switch (loader.kind) {
    case 'waiting': {
      const { wait } = loader;
      if (!wait.replies) return headline(wait.reason);
      return intl.formatMessage(i18n.turnWaiting, {
        way: wait.replies.way,
        count: wait.replies.count,
        node: wait.target.name,
        duration: loadDurationText(intl, wait.load),
      });
    }
    case 'refused':
      return refusalText(intl, loader.refusal);
    case 'loading': {
      const { swap, forThisChat } = loader;
      if (forThisChat && swap.phase != null) {
        return intl.formatMessage(i18n.turnLoading, {
          node: swap.target.name,
          phase: loadPhaseWord(intl, swap.phase),
        });
      }
      return swappingText(intl, swap.target.name);
    }
  }
}

/**
 * The line under a load this chat's turn waits on: its measured time ("Loads in about 48s · median
 * of 3 loads"), or §8.7's `nodes.turnFirstLoad` when none is measured. null = not this chat's load.
 */
export function loaderDetail(intl: IntlShape, loader: ChatLoader): string | null {
  if (loader.kind !== 'loading' || !loader.forThisChat) return null;
  const { load, target } = loader.swap;
  if (!load) return intl.formatMessage(i18n.turnFirstLoad, { node: target.name });
  return intl.formatMessage(i18n.loadMedian, {
    count: load.count,
    duration: formatElapsed(load.medianMs / 1000),
  });
}

/** The displaced chat's notice (§8.7 `nodes.displacedNotice` / `nodes.displacedFailed`). */
export function displacedText(intl: IntlShape, displaced: NodeDisplaced): string {
  const node = displaced.node.name;
  const other = displaced.other.name;
  if (displaced.failed != null) {
    return intl.formatMessage(i18n.displacedFailed, { node, other, words: displaced.failed });
  }
  const duration = loadDurationText(intl, displaced.load);
  return displaced.chat != null
    ? intl.formatMessage(i18n.displacedNotice, { node, other, chat: displaced.chat, duration })
    : intl.formatMessage(i18n.displacedNoticeStarted, { node, other, duration });
}
