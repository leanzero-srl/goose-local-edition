import { useEffect, useState, type ReactNode } from 'react';
import type { CompactionStatus } from '@aaif/goose-sdk';
import {
  AlertTriangle,
  Archive,
  Check,
  LoaderCircle,
  MessageCircleQuestion,
  X,
} from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, RADIUS, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, PHASE_DOT, cx } from '../lz';
import { readBarOf } from '../leanzero-swarm/engineFigures';
import { formatElapsed, formatRate } from '../leanzero-swarm/mlxLiveStats';
import { PromptReadBar, promptCacheWords, freshReadText } from '../leanzero-swarm/PromptReadBar';
import { useTurnReadOf } from '../turnWorking/turnReadStore';
import { compactCommand } from '../../acp/compaction';
import {
  announceCompactionSteerChanged,
  requestOpenContextRail,
} from '../contextRail/contextRailRequest';
import { NoteDialog } from './NoteEditor';
import { useCompactionSteer } from './useCompactionSteer';

const i18n = defineMessages({
  compacting: { id: 'compaction.card.compacting', defaultMessage: 'Compacting the conversation' },
  reading: { id: 'compaction.card.reading', defaultMessage: 'Reading the conversation' },
  readingTokens: { id: 'compaction.card.readingTokens', defaultMessage: '{tokens} tokens' },
  writing: { id: 'compaction.card.writing', defaultMessage: 'Writing the summary' },
  written: { id: 'compaction.card.written', defaultMessage: '{tokens} tokens written' },
  rate: { id: 'compaction.card.rate', defaultMessage: '{rate} tok/s' },
  part: { id: 'compaction.card.part', defaultMessage: 'part {done} of {total}' },
  parts: { id: 'compaction.card.parts', defaultMessage: 'Parts of the summary' },
  done: { id: 'compaction.card.done', defaultMessage: 'Conversation compacted' },
  beforeAfter: {
    id: 'compaction.card.beforeAfter',
    defaultMessage: '{before} → {after} tokens',
  },
  whatWasKept: { id: 'compaction.card.whatWasKept', defaultMessage: 'What was kept' },
  concern: {
    id: 'compaction.card.concern',
    defaultMessage: 'Compacted following your note. goose noted: “{concern}”',
  },
  missing: {
    id: 'compaction.card.missing',
    defaultMessage: 'goose didn’t say whether your note was clear — it was followed as written.',
  },
  notSent: {
    id: 'compaction.card.notSent',
    defaultMessage:
      'The conversation was too long to send your note with it — goose kept your note word for word.',
  },
  noteFollowed: {
    id: 'compaction.card.noteFollowed',
    defaultMessage: 'Compacted following your note.',
  },
  question: { id: 'compaction.card.question', defaultMessage: 'goose asks about your note' },
  questionSaid: { id: 'compaction.card.questionSaid', defaultMessage: '“{question}”' },
  yourNote: { id: 'compaction.card.yourNote', defaultMessage: 'Your note: “{note}”' },
  asWritten: { id: 'compaction.card.asWritten', defaultMessage: 'Compact as written' },
  editNote: { id: 'compaction.card.editNote', defaultMessage: 'Edit note' },
  editTitle: {
    id: 'compaction.card.editTitle',
    defaultMessage: 'Your note for this compaction',
  },
  saveAndCompact: { id: 'compaction.card.saveAndCompact', defaultMessage: 'Save and compact' },
  cancel: { id: 'compaction.card.cancel', defaultMessage: 'Cancel' },
  cancelled: {
    id: 'compaction.card.cancelled',
    defaultMessage: 'Not compacted — the conversation is as it was.',
  },
  failed: { id: 'compaction.card.failed', defaultMessage: 'Compaction failed' },
  failedSaid: {
    id: 'compaction.card.failedSaid',
    defaultMessage: '{error}. Your conversation is unchanged.',
  },
  tryAgain: { id: 'compaction.card.tryAgain', defaultMessage: 'Try again' },
  manual: { id: 'compaction.card.manual', defaultMessage: 'You asked' },
  auto: { id: 'compaction.card.auto', defaultMessage: 'Automatic' },
  recovery: { id: 'compaction.card.recovery', defaultMessage: 'The chat was too long' },
});

const compactNumber = (intl: ReturnType<typeof useIntl>, n: number) =>
  intl.formatNumber(n, { notation: 'compact', maximumFractionDigits: 1 });

function Figures({ parts }: { parts: ReactNode[] }) {
  const shown = parts.filter((part) => part != null && part !== false);
  if (shown.length === 0) return null;
  return (
    <span data-testid="compaction-card-figures" className={cx(TYPE.meta, TNUM, 'text-lz-ink-2')}>
      {shown.map((part, i) => (
        <span key={i}>
          {i > 0 && ' · '}
          {part}
        </span>
      ))}
    </span>
  );
}

/**
 * The compaction point in the chat (Q-357 VISUAL): one card that says what goose is doing to the
 * conversation and then what it did. While it runs, the engine's own read of the conversation — the
 * split the prefix cache made of it (Q-337) — then the summary as it is written: tokens, rate and
 * the parts (the section headings the model has written). When it ends: before → after and the
 * time, what the model said about the person's note, and a way into what was kept. A question
 * about the note waits for the person; a failure says the conversation is unchanged.
 */
export function CompactionCard({
  sessionId,
  status,
  live,
  onSend,
}: {
  sessionId: string;
  status: CompactionStatus;
  /** The card of a compaction running in this window (the engine's read is shown only then). */
  live: boolean;
  onSend: (text: string) => void;
}) {
  const intl = useIntl();
  const read = useTurnReadOf(live ? sessionId : null);
  const [dismissed, setDismissed] = useState(false);
  const [editing, setEditing] = useState(false);
  const steer = useCompactionSteer(status.stage === 'question' ? sessionId : null);

  useEffect(() => {
    if (status.stage === 'done') announceCompactionSteerChanged(sessionId);
  }, [status.stage, sessionId]);

  const parts = status.parts ?? [];
  const triggerLabel = intl.formatMessage(
    status.trigger === 'manual'
      ? i18n.manual
      : status.trigger === 'auto'
        ? i18n.auto
        : i18n.recovery
  );

  if (status.stage === 'reading' || status.stage === 'writing') {
    const writing = status.stage === 'writing';
    const cache = !writing && read ? read.cache : null;
    const bar = !writing && read ? readBarOf(read.progress, cache) : null;
    const rate =
      writing && status.writtenTokens != null && status.writingMs && status.writingMs > 0
        ? status.writtenTokens / (status.writingMs / 1000)
        : null;
    const figures: ReactNode[] = writing
      ? [
          status.writtenTokens != null
            ? intl.formatMessage(i18n.written, {
                tokens: compactNumber(intl, status.writtenTokens),
              })
            : null,
          rate != null
            ? intl.formatMessage(i18n.rate, { rate: formatRate(rate, intl.locale) })
            : null,
          parts.length > 0
            ? intl.formatMessage(i18n.part, {
                done: parts.length,
                total: status.partsTotal,
              })
            : null,
          formatElapsed(status.elapsedMs / 1000),
        ]
      : [
          cache
            ? promptCacheWords(intl, cache, 'surface', false)
            : status.tokensBefore != null
              ? intl.formatMessage(i18n.readingTokens, {
                  tokens: compactNumber(intl, status.tokensBefore),
                })
              : null,
          cache ? freshReadText(intl, cache) : null,
          formatElapsed(status.elapsedMs / 1000),
        ];
    return (
      <section
        data-testid="compaction-card"
        data-stage={status.stage}
        aria-live="polite"
        className={cx('flex max-w-xl flex-col gap-2 p-3', SURFACE.card)}
      >
        <div className="flex flex-wrap items-center gap-2">
          <Chip
            phase={writing ? 'writing' : 'reading'}
            icon={<LoaderCircle className="animate-spin" />}
          >
            {intl.formatMessage(writing ? i18n.writing : i18n.reading)}
          </Chip>
          <span className={cx(TYPE.body, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.compacting)}
          </span>
          <Chip>{triggerLabel}</Chip>
        </div>
        <Figures parts={figures} />
        {!writing && bar && (
          <PromptReadBar
            bar={bar}
            cache={cache}
            paint="surface"
            label={intl.formatMessage(i18n.reading)}
            height="h-2.5"
            testId="compaction-card-read"
          />
        )}
        {writing && (
          <div
            role="progressbar"
            aria-label={intl.formatMessage(i18n.parts)}
            aria-valuemin={0}
            aria-valuemax={status.partsTotal}
            aria-valuenow={parts.length}
            data-testid="compaction-card-parts"
            className="flex gap-1"
          >
            {Array.from({ length: Math.max(status.partsTotal, parts.length) }, (_, i) => (
              <div
                key={i}
                data-done={i < parts.length}
                className={cx(
                  'h-2.5 flex-1',
                  RADIUS.pill,
                  i < parts.length
                    ? PHASE_DOT.writing
                    : 'border border-lz-border-strong bg-lz-surface'
                )}
              />
            ))}
          </div>
        )}
        {writing && parts.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {parts.map((part, i) => (
              <Chip key={`${i}-${part}`} phase="writing" icon={<Check />}>
                {part}
              </Chip>
            ))}
          </div>
        )}
      </section>
    );
  }

  if (status.stage === 'done') {
    const verdict =
      status.noteVerdict === 'concern' && status.said
        ? intl.formatMessage(i18n.concern, { concern: status.said })
        : status.noteVerdict === 'missing'
          ? intl.formatMessage(i18n.missing)
          : status.noteVerdict === 'notSent'
            ? intl.formatMessage(i18n.notSent)
            : status.noteVerdict === 'ok'
              ? intl.formatMessage(i18n.noteFollowed)
              : null;
    return (
      <section
        data-testid="compaction-card"
        data-stage="done"
        className={cx('flex max-w-xl flex-col gap-2 p-3', SURFACE.card)}
      >
        <div className="flex flex-wrap items-center gap-2">
          <Chip tone="ok" icon={<Archive />}>
            {intl.formatMessage(i18n.done)}
          </Chip>
          <Figures
            parts={[
              status.tokensBefore != null && status.tokensAfter != null
                ? intl.formatMessage(i18n.beforeAfter, {
                    before: compactNumber(intl, status.tokensBefore),
                    after: compactNumber(intl, status.tokensAfter),
                  })
                : null,
              formatElapsed(status.elapsedMs / 1000),
              triggerLabel,
            ]}
          />
          <Button
            size="sm"
            variant="secondary"
            className="ml-auto"
            data-testid="compaction-card-kept"
            onClick={() => requestOpenContextRail(sessionId)}
          >
            {intl.formatMessage(i18n.whatWasKept)}
          </Button>
        </div>
        {verdict && (
          <p
            data-testid="compaction-card-verdict"
            className={cx(
              TYPE.body,
              status.noteVerdict === 'ok' ? 'text-lz-ink-2' : cx(TONE_TEXT.warn, WEIGHT.semibold)
            )}
          >
            {verdict}
          </p>
        )}
        {status.warning && (
          <p className={cx(TYPE.meta, TONE_TEXT.warn)} data-testid="compaction-card-warning">
            {status.warning}
          </p>
        )}
      </section>
    );
  }

  if (status.stage === 'question') {
    if (dismissed) {
      return (
        <p data-testid="compaction-card" data-stage="cancelled" className={TYPE.bodyMuted}>
          {intl.formatMessage(i18n.cancelled)}
        </p>
      );
    }
    const note =
      status.note ?? (steer.state.kind === 'ready' ? (steer.state.steer.note ?? '') : '');
    return (
      <section
        data-testid="compaction-card"
        data-stage="question"
        className={cx('flex max-w-xl flex-col gap-2 p-3', SURFACE.card)}
      >
        <div className="flex flex-wrap items-center gap-2">
          <Chip tone="warn" icon={<MessageCircleQuestion />}>
            {intl.formatMessage(i18n.question)}
          </Chip>
        </div>
        <p className={cx(TYPE.body, WEIGHT.semibold)} data-testid="compaction-card-question">
          {intl.formatMessage(i18n.questionSaid, { question: status.said ?? '' })}
        </p>
        {note && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.yourNote, { note })}</p>}
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="primary"
            data-testid="compaction-card-as-written"
            onClick={async () => {
              if (await steer.save({ followAsWritten: true })) onSend(compactCommand());
            }}
          >
            {intl.formatMessage(i18n.asWritten)}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            data-testid="compaction-card-edit-note"
            onClick={() => setEditing(true)}
          >
            {intl.formatMessage(i18n.editNote)}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            icon={<X />}
            data-testid="compaction-card-cancel"
            onClick={() => setDismissed(true)}
          >
            {intl.formatMessage(i18n.cancel)}
          </Button>
        </div>
        {steer.saveError && !editing && (
          <p className={cx(TYPE.meta, TONE_TEXT.err)}>{steer.saveError}</p>
        )}
        <NoteDialog
          open={editing}
          title={intl.formatMessage(i18n.editTitle)}
          initialNote={note}
          initialStanding={
            steer.state.kind === 'ready' ? (steer.state.steer.standing ?? false) : false
          }
          saveError={steer.saveError}
          confirmLabel={intl.formatMessage(i18n.saveAndCompact)}
          onClose={() => setEditing(false)}
          onConfirm={async (edited, standing) => {
            if (await steer.save({ note: edited, standing, followAsWritten: false })) {
              setEditing(false);
              onSend(compactCommand());
            }
          }}
        />
      </section>
    );
  }

  return (
    <section
      data-testid="compaction-card"
      data-stage="failed"
      className={cx('flex max-w-xl flex-col gap-2 p-3', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Chip tone="err" icon={<AlertTriangle />}>
          {intl.formatMessage(i18n.failed)}
        </Chip>
      </div>
      <p className={TYPE.body} data-testid="compaction-card-error">
        {intl.formatMessage(i18n.failedSaid, { error: status.error ?? '' })}
      </p>
      <div>
        <Button
          size="sm"
          variant="primary"
          data-testid="compaction-card-retry"
          onClick={() => onSend(compactCommand())}
        >
          {intl.formatMessage(i18n.tryAgain)}
        </Button>
      </div>
    </section>
  );
}
