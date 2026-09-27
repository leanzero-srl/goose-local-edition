import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { formatElapsed } from '../leanzero-swarm/mlxLiveStats';
import { ROLE_WORD } from './NodeChips';
import type { SentenceEntry, SentenceFacts } from './resolve';

/**
 * A role's rule read back as plain words (DESIGN-NODES-AND-STRATEGIES.md §6.3, §8.4): "Chat runs on
 * 27B · both Macs. If it can't run, on Claude Sonnet · OpenRouter. If 27B · both Macs isn't loaded,
 * it loads and your turn waits (about 1m 40s)."
 *
 * Every sentence is an ICU template — one per when-rule and chain shape, one per if-not-loaded
 * rule — composed through templates (`then`, `join`) and the locale's own list format, never by
 * concatenating English. The facts are `sentenceFacts` (resolve.ts, pinned to the router's fixture);
 * what a node IS (MLX or not) and its MEASURED load time come from the caller — a load time nobody
 * measured is said as such, never estimated.
 */

const i18n = defineMessages({
  failoverOne: { id: 'strategies.sentenceFailoverOne', defaultMessage: '{role} runs on {first}.' },
  failoverMany: {
    id: 'strategies.sentenceFailoverMany',
    defaultMessage: '{role} runs on {first}. If it can’t run, on {rest}.',
  },
  overflowOne: {
    id: 'strategies.sentenceOverflowOne',
    defaultMessage: '{role} runs on {first}; when it is busy, the work waits for it.',
  },
  overflowMany: {
    id: 'strategies.sentenceOverflowMany',
    defaultMessage: '{role} runs on {first}; when it is busy, the extra work goes to {rest}.',
  },
  shareOne: { id: 'strategies.sentenceShareOne', defaultMessage: '{role} runs on {first}.' },
  shareMany: { id: 'strategies.sentenceShareMany', defaultMessage: '{role} is shared: {parts}.' },
  sharePart: {
    id: 'strategies.sentenceSharePart',
    defaultMessage: '{node} {weight, plural, one {# part} other {# parts}}',
  },
  then: { id: 'strategies.sentenceThen', defaultMessage: '{earlier}, then {next}' },
  loadMeasured: {
    id: 'strategies.sentenceLoadMeasured',
    defaultMessage:
      'If {node} isn’t loaded, it loads and {who, select, chat {your turn waits} other {the task waits}} (about {duration}).',
  },
  loadUnmeasured: {
    id: 'strategies.sentenceLoadUnmeasured',
    defaultMessage:
      'If {node} isn’t loaded, it loads and {who, select, chat {your turn waits} other {the task waits}}; its first load is not measured yet.',
  },
  useNextMany: {
    id: 'strategies.sentenceUseNextMany',
    defaultMessage:
      'If {node} isn’t loaded, {next} takes the work meanwhile and nothing is loaded.',
  },
  useNextLast: {
    id: 'strategies.sentenceUseNextLast',
    defaultMessage:
      'If {node} isn’t loaded, no node after it can take the work, so it is refused with the reason.',
  },
  join: { id: 'strategies.sentenceJoin', defaultMessage: '{main} {clause}' },
});

/** What the sentence needs to know about a chain entry beyond its name. */
export interface SentenceNodeFacts {
  /** An MLX node that can be not-loaded (a pinned way); cloud and follows nodes never load. */
  loads: boolean;
  /** The median of its measured loads, in ms; null = not measured yet. */
  loadMs: number | null;
}

export function sentenceFor(
  intl: IntlShape,
  facts: SentenceFacts,
  about: (node: string) => SentenceNodeFacts
): string {
  const role = intl.formatMessage(ROLE_WORD[facts.role]);
  const [first, ...rest] = facts.entries;
  if (!first) return '';
  const main = mainSentence(intl, facts, role, first, rest);
  const clause = loadClause(intl, facts, about);
  return clause ? intl.formatMessage(i18n.join, { main, clause }) : main;
}

function inOrder(intl: IntlShape, entries: SentenceEntry[]): string {
  return entries
    .map((e) => e.name)
    .reduce((earlier, next) => intl.formatMessage(i18n.then, { earlier, next }));
}

function mainSentence(
  intl: IntlShape,
  facts: SentenceFacts,
  role: string,
  first: SentenceEntry,
  rest: SentenceEntry[]
): string {
  switch (facts.when) {
    case 'failover':
      return rest.length === 0
        ? intl.formatMessage(i18n.failoverOne, { role, first: first.name })
        : intl.formatMessage(i18n.failoverMany, {
            role,
            first: first.name,
            rest: inOrder(intl, rest),
          });
    case 'overflow':
      return rest.length === 0
        ? intl.formatMessage(i18n.overflowOne, { role, first: first.name })
        : intl.formatMessage(i18n.overflowMany, {
            role,
            first: first.name,
            rest: inOrder(intl, rest),
          });
    case 'share': {
      if (rest.length === 0) return intl.formatMessage(i18n.shareOne, { role, first: first.name });
      const parts = intl.formatList(
        facts.entries.map((e) =>
          intl.formatMessage(i18n.sharePart, { node: e.name, weight: e.weight })
        ),
        { type: 'unit' }
      );
      return intl.formatMessage(i18n.shareMany, { role, parts });
    }
  }
}

/** The if-not-loaded clause, said about the chain's first entry that can be not loaded. */
function loadClause(
  intl: IntlShape,
  facts: SentenceFacts,
  about: (node: string) => SentenceNodeFacts
): string | null {
  const at = facts.entries.findIndex((e) => about(e.node).loads);
  if (at < 0) return null;
  const entry = facts.entries[at];
  if (facts.ifNotLoaded === 'useNext') {
    const next = facts.entries[at + 1];
    return next
      ? intl.formatMessage(i18n.useNextMany, { node: entry.name, next: next.name })
      : intl.formatMessage(i18n.useNextLast, { node: entry.name });
  }
  const who = facts.role === 'chat' ? 'chat' : 'task';
  const loadMs = about(entry.node).loadMs;
  return loadMs == null
    ? intl.formatMessage(i18n.loadUnmeasured, { node: entry.name, who })
    : intl.formatMessage(i18n.loadMeasured, {
        node: entry.name,
        who,
        duration: formatElapsed(loadMs / 1000),
      });
}
