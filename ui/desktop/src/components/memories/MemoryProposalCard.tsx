import { useState } from 'react';
import { Brain, BookOpen } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import type { MemoryProposalDto } from '../../acp/proposals';
import { ChatState } from '../../types/chatState';
import { Button, FOCUS, MOTION, RADIUS, SURFACE, TYPE, cx } from '../lz';
import { useMemoryProposals } from './useMemoryProposals';

const i18n = defineMessages({
  saveMemory: { id: 'memoryProposal.saveMemory', defaultMessage: 'Save this as a memory?' },
  saveKnowledge: {
    id: 'memoryProposal.saveKnowledge',
    defaultMessage: 'Save this as a knowledge piece?',
  },
  positive: { id: 'memoryProposal.positive', defaultMessage: 'Positive' },
  negative: { id: 'memoryProposal.negative', defaultMessage: 'Negative' },
  why: { id: 'memoryProposal.why', defaultMessage: 'Why: {why}' },
  sources: { id: 'memoryProposal.sources', defaultMessage: 'Sources: {sources}' },
  save: { id: 'memoryProposal.save', defaultMessage: 'Save' },
  no: { id: 'memoryProposal.no', defaultMessage: 'No' },
  edit: { id: 'memoryProposal.edit', defaultMessage: 'Edit' },
  expired: { id: 'memoryProposal.expired', defaultMessage: 'Expired — not saved' },
  dismiss: { id: 'memoryProposal.dismiss', defaultMessage: 'Dismiss' },
  scope: { id: 'memoryProposal.scope', defaultMessage: '{category} · {scope}' },
  scopeGlobal: { id: 'memoryProposal.scopeGlobal', defaultMessage: 'global' },
  scopeLocal: { id: 'memoryProposal.scopeLocal', defaultMessage: 'this project' },
  fromProject: {
    id: 'memoryProposal.fromProject',
    defaultMessage: 'From a chat in this project · {when}',
  },
});

// The same solid hues MemoriesView paints its type chips with: user/memory teal, feedback amber.
// White ink on both; no rails, no tints.
export const POLARITY_FILL: Record<'positive' | 'negative', string> = {
  positive: '#0f766e',
  negative: '#b45309',
};

interface CardProps {
  proposal: MemoryProposalDto;
  /**
   * Filed for the whole project, not this chat (Q-82: every knowledge piece before the fix, or one
   * filed with no session) — the card says so and when, so it never reads as this chat's own.
   */
  fromProject?: boolean;
  busy: boolean;
  onAnswer: (decision: 'save' | 'decline', text?: string) => void;
  onDismiss: () => void;
}

export function ProposalCard({
  proposal,
  fromProject = false,
  busy,
  onAnswer,
  onDismiss,
}: CardProps) {
  const intl = useIntl();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(proposal.text);
  const expired = proposal.state === 'expired';
  const knowledge = proposal.kind === 'knowledge';
  const polarity = proposal.polarity ?? null;

  return (
    <div
      data-testid="memory-proposal-card"
      data-proposal-id={proposal.id}
      className={cx(SURFACE.card, 'mb-3 px-4 py-3')}
    >
      <div className="flex items-center gap-2">
        {knowledge ? (
          <BookOpen className="size-4 text-lz-ink-2" aria-hidden />
        ) : (
          <Brain className="size-4 text-lz-ink-2" aria-hidden />
        )}
        <span className={cx(TYPE.body, 'font-lz-semibold')}>
          {intl.formatMessage(knowledge ? i18n.saveKnowledge : i18n.saveMemory)}
        </span>
        {polarity && (
          <span
            data-testid="memory-proposal-polarity"
            className={cx(
              RADIUS.pill,
              'px-2 py-0.5 text-[11px] font-lz-semibold uppercase text-white'
            )}
            style={{ backgroundColor: POLARITY_FILL[polarity] }}
          >
            {intl.formatMessage(polarity === 'positive' ? i18n.positive : i18n.negative)}
          </span>
        )}
        <span className={cx(TYPE.meta, 'ml-auto')}>
          {intl.formatMessage(i18n.scope, {
            category: proposal.category,
            scope: intl.formatMessage(proposal.isGlobal ? i18n.scopeGlobal : i18n.scopeLocal),
          })}
        </span>
      </div>
      {fromProject && (
        <p data-testid="memory-proposal-origin" className={cx(TYPE.meta, 'mt-1 font-lz-semibold')}>
          {intl.formatMessage(i18n.fromProject, {
            when: intl.formatDate(proposal.createdAt * 1000, {
              month: 'short',
              day: 'numeric',
              hour: '2-digit',
              minute: '2-digit',
            }),
          })}
        </p>
      )}

      {editing && !expired ? (
        <textarea
          data-testid="memory-proposal-text"
          className={cx(
            'mt-2 w-full resize-y border border-lz-border-strong bg-lz-surface px-2 py-1 text-lz-body text-lz-ink',
            RADIUS.control,
            FOCUS,
            MOTION
          )}
          rows={3}
          maxLength={350}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      ) : (
        <p
          data-testid="memory-proposal-text"
          className={cx(TYPE.body, 'mt-2 whitespace-pre-wrap', expired && 'text-lz-ink-3')}
        >
          {text}
        </p>
      )}

      {proposal.why && (
        <p className={cx(TYPE.meta, 'mt-1')}>
          {intl.formatMessage(i18n.why, { why: proposal.why })}
        </p>
      )}
      {proposal.sources.length > 0 && (
        <p className={cx(TYPE.meta, 'mt-1')}>
          {intl.formatMessage(i18n.sources, { sources: proposal.sources.join(', ') })}
        </p>
      )}

      <div className="mt-3 flex items-center gap-2">
        {expired ? (
          <>
            <span className={cx(TYPE.meta, 'font-lz-semibold')}>
              {intl.formatMessage(i18n.expired)}
            </span>
            <Button variant="ghost" size="sm" className="ml-auto" onClick={onDismiss}>
              {intl.formatMessage(i18n.dismiss)}
            </Button>
          </>
        ) : (
          <>
            <Button
              variant="primary"
              size="sm"
              disabled={busy || text.trim().length === 0}
              onClick={() => onAnswer('save', editing ? text : undefined)}
              data-testid="memory-proposal-save"
            >
              {intl.formatMessage(i18n.save)}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={busy}
              onClick={() => onAnswer('decline')}
              data-testid="memory-proposal-no"
            >
              {intl.formatMessage(i18n.no)}
            </Button>
            {!editing && (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy}
                onClick={() => setEditing(true)}
                data-testid="memory-proposal-edit"
              >
                {intl.formatMessage(i18n.edit)}
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

interface CardsProps {
  sessionId: string | undefined;
  chatState: ChatState;
  className?: string;
}

/** Under the last message: every open proposal for the session, newest last. */
export default function MemoryProposalCards({ sessionId, chatState, className }: CardsProps) {
  const { proposals, busyId, error, answer, dismissExpired } = useMemoryProposals(
    sessionId,
    chatState
  );
  if (proposals.length === 0) return null;
  return (
    <div className={className} data-testid="memory-proposals">
      {proposals.map((p) => (
        <ProposalCard
          key={p.id}
          proposal={p}
          fromProject={p.key !== sessionId}
          busy={busyId === p.id}
          onAnswer={(decision, text) => void answer(p, decision, text)}
          onDismiss={() => dismissExpired(p.id)}
        />
      ))}
      {error && <p className={cx(TYPE.meta, 'text-lz-err')}>{error}</p>}
    </div>
  );
}
