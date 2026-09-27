import type { IntlShape } from 'react-intl';
import type { BackgroundWorkKind } from '@aaif/goose-sdk';
import { defineMessages } from '../../i18n';

/**
 * Q-185: the words for goose's own model calls FOR a session — the fact check after the reply, the
 * title, a tool label — tagged with their kind where goose makes them (crates/goose/src/
 * background_work.rs). The session lists, the chat, the Engine card and the cut guard all name the
 * work from here, so the fact check is never "the answer being written" on one surface and idle on
 * another.
 */
const label = defineMessages({
  factCheck: { id: 'backgroundWork.factCheck', defaultMessage: 'Checking the reply' },
  memoryReview: {
    id: 'backgroundWork.memoryReview',
    defaultMessage: 'Reviewing the turn for memories',
  },
  title: { id: 'backgroundWork.title', defaultMessage: 'Naming the chat' },
  toolLabel: { id: 'backgroundWork.toolLabel', defaultMessage: 'Labeling tool calls' },
  compaction: { id: 'backgroundWork.compaction', defaultMessage: 'Compacting the conversation' },
  toolDigest: { id: 'backgroundWork.toolDigest', defaultMessage: 'Summarizing a tool result' },
  permissionCheck: {
    id: 'backgroundWork.permissionCheck',
    defaultMessage: 'Checking a tool’s permission',
  },
  safetyCheck: { id: 'backgroundWork.safetyCheck', defaultMessage: 'Inspecting a tool call' },
  sessionSummary: {
    id: 'backgroundWork.sessionSummary',
    defaultMessage: 'Summarizing the conversation',
  },
  recipe: { id: 'backgroundWork.recipe', defaultMessage: 'Writing a recipe' },
});

/** The one word a session row can spare beside its name. */
const short = defineMessages({
  factCheck: { id: 'backgroundWork.short.factCheck', defaultMessage: 'Checking' },
  memoryReview: { id: 'backgroundWork.short.memoryReview', defaultMessage: 'Reviewing' },
  title: { id: 'backgroundWork.short.title', defaultMessage: 'Naming' },
  toolLabel: { id: 'backgroundWork.short.toolLabel', defaultMessage: 'Labeling' },
  compaction: { id: 'backgroundWork.short.compaction', defaultMessage: 'Compacting' },
  toolDigest: { id: 'backgroundWork.short.toolDigest', defaultMessage: 'Summarizing' },
  permissionCheck: { id: 'backgroundWork.short.permissionCheck', defaultMessage: 'Checking' },
  safetyCheck: { id: 'backgroundWork.short.safetyCheck', defaultMessage: 'Inspecting' },
  sessionSummary: { id: 'backgroundWork.short.sessionSummary', defaultMessage: 'Summarizing' },
  recipe: { id: 'backgroundWork.short.recipe', defaultMessage: 'Writing' },
});

const named = defineMessages({
  named: { id: 'backgroundWork.named', defaultMessage: '{work} · {name}' },
});

/** "Checking the reply". */
export function backgroundWorkLabel(intl: IntlShape, kind: BackgroundWorkKind): string {
  return intl.formatMessage(label[kind]);
}

/** "Checking" — the sidebar pill. */
export function backgroundWorkShort(intl: IntlShape, kind: BackgroundWorkKind): string {
  return intl.formatMessage(short[kind]);
}

/** "Checking the reply · Jira Migration Kickoff Notes · 5". */
export function backgroundWorkFor(intl: IntlShape, kind: BackgroundWorkKind, name: string): string {
  return intl.formatMessage(named.named, { work: backgroundWorkLabel(intl, kind), name });
}
