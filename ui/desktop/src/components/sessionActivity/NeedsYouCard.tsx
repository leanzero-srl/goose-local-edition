import { useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import { Check, ChevronRight, Clock, Hand, PlugZap, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import ElicitationRequest from '../ElicitationRequest';
import { Button, FOCUS, MOTION, RADIUS, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { ChatState } from '../../types/chatState';
import {
  answerMessage,
  pickOptions,
  resolveNeedsYou,
  useSessionActivity,
  type NeedsYouItemDto,
} from './sessionActivityStore';
import {
  answersWaiting,
  cancelQueuedAnswer,
  closeUnsentAnswer,
  deliverQueuedAnswers,
  enqueueAnswer,
  useAnswerQueue,
  type QueuedAnswer,
} from './needsYouAnswerQueue';
import { useNeedsYouFold } from './needsYouFold';

const i18n = defineMessages({
  title: { id: 'needsYouCard.title', defaultMessage: 'Needs you' },
  extensionTitle: {
    id: 'needsYouCard.extensionTitle',
    defaultMessage: 'An extension needs information',
  },
  position: { id: 'needsYouCard.position', defaultMessage: '{index} of {total}' },
  recommended: { id: 'needsYouCard.recommended', defaultMessage: 'Recommended' },
  options: { id: 'needsYouCard.options', defaultMessage: 'Or pick' },
  placeholder: {
    id: 'needsYouCard.placeholder',
    defaultMessage: 'Or type your own answer…',
  },
  answer: { id: 'needsYouCard.answer', defaultMessage: 'Answer' },
  dismiss: { id: 'needsYouCard.dismiss', defaultMessage: 'Dismiss' },
  busyQueues: {
    id: 'needsYouCard.busyQueues',
    defaultMessage:
      'goose is working — an answer you give now waits and is sent when this turn ends.',
  },
  queued: { id: 'needsYouCard.queued', defaultMessage: 'Queued · answers when this turn ends' },
  queuedChip: { id: 'needsYouCard.queuedChip', defaultMessage: 'Queued' },
  sending: { id: 'needsYouCard.sending', defaultMessage: 'Sending your answer…' },
  cancelQueued: { id: 'needsYouCard.cancelQueued', defaultMessage: 'Cancel' },
  stack: {
    id: 'needsYouCard.stack',
    defaultMessage: 'Needs you · {count, plural, one {# question} other {# questions}}',
  },
  stackQueued: { id: 'needsYouCard.stackQueued', defaultMessage: '{count} queued' },
  unsentClosed: {
    id: 'needsYouCard.unsentClosed',
    defaultMessage: 'Not sent — this question was closed before your queued answer could go.',
  },
  unsentFailed: {
    id: 'needsYouCard.unsentFailed',
    defaultMessage: 'Not sent — your queued answer could not be saved: {error}',
  },
  unsentAnswer: { id: 'needsYouCard.unsentAnswer', defaultMessage: 'Your answer: {answer}' },
  closeNotice: { id: 'needsYouCard.closeNotice', defaultMessage: 'Close this notice' },
  failed: { id: 'needsYouCard.failed', defaultMessage: 'Could not save that: {error}' },
});

/** Answer surfaces are solid: amber header band, full amber border — never a rail, never a wash. */
const CARD = 'overflow-hidden border-2 border-lz-warn-solid bg-lz-surface';
/**
 * Q-340, the clipped action row: in the height-capped list an overflow-hidden card's automatic
 * min-height is 0, so the cards SHRANK and cut their own bottoms. A folded card never shrinks; an
 * open card may, but only its question part scrolls — the band and the answer footer (text box,
 * Answer, Dismiss) never shrink, so the action row is always whole on screen.
 */
const cardLayout = (folded: boolean) => (folded ? 'shrink-0' : 'flex min-h-0 flex-col');
const BAND_TONE = TONE_FILL.warn;
/** A chip that sits ON the amber band: solid surface, dark ink. */
const BAND_CHIP = cx(
  'inline-flex shrink-0 items-center gap-1 bg-lz-surface px-1.5 py-0.5 text-lz-meta text-lz-ink [&_svg]:size-3',
  WEIGHT.semibold,
  RADIUS.control
);

interface FoldBandProps {
  folded: boolean;
  onToggle: () => void;
  bodyId: string;
  icon: ReactNode;
  title: string;
  /** Shown beside the title only while folded — the card's one line. */
  summary: string;
  queued: boolean;
  position: string | null;
}

/**
 * The card's header IS its fold toggle (a real button, aria-expanded, Enter/Space): folded, the
 * card is this one line — the hand, "Needs you", the question cut to the line, "1 of 2".
 */
function FoldBand({
  folded,
  onToggle,
  bodyId,
  icon,
  title,
  summary,
  queued,
  position,
}: FoldBandProps) {
  const intl = useIntl();
  return (
    <button
      type="button"
      data-testid="needs-you-fold"
      aria-expanded={!folded}
      aria-controls={bodyId}
      onClick={onToggle}
      className={cx(
        'flex w-full min-w-0 shrink-0 items-center gap-2 px-3 py-1.5 text-left [&>svg]:size-4 [&>svg]:shrink-0',
        BAND_TONE,
        FOCUS,
        MOTION
      )}
    >
      <ChevronRight aria-hidden className={cx(!folded && 'rotate-90')} />
      {icon}
      <span className={cx('shrink-0 text-lz-body', WEIGHT.semibold)}>{title}</span>
      {folded ? (
        <span data-testid="needs-you-fold-summary" className="min-w-0 flex-1 truncate text-lz-body">
          {summary}
        </span>
      ) : (
        <span className="flex-1" />
      )}
      {queued && (
        <span data-testid="needs-you-fold-queued" className={BAND_CHIP}>
          <Clock aria-hidden />
          {intl.formatMessage(i18n.queuedChip)}
        </span>
      )}
      {position && <span className={cx('shrink-0 text-lz-meta', TNUM)}>{position}</span>}
    </button>
  );
}

interface QuestionCardProps {
  item: NeedsYouItemDto;
  index: number;
  total: number;
  /** The chat's turn runs (or answers already wait): an answer given now is queued, not sent. */
  queues: boolean;
  /** This question's queued answer, if one waits. */
  queued: QueuedAnswer | null;
  /** Its queued answer is being resolved and sent right now. */
  sending: boolean;
  folded: boolean;
  onToggleFold: () => void;
  onAnswer: (item: NeedsYouItemDto, answer: string) => Promise<void>;
  /** Takes the queued answer back; returns its words. */
  onCancelQueued: (item: NeedsYouItemDto) => string | null;
  onDismiss: (item: NeedsYouItemDto) => Promise<void>;
}

export function QuestionCard({
  item,
  index,
  total,
  queues,
  queued,
  sending: queuedSending,
  folded,
  onToggleFold,
  onAnswer,
  onCancelQueued,
  onDismiss,
}: QuestionCardProps) {
  const intl = useIntl();
  const bodyId = useId();
  const [text, setText] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = saving || queuedSending;
  const options = pickOptions(item);

  const act = async (run: () => Promise<void>) => {
    setSaving(true);
    setError(null);
    try {
      await run();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };
  const answer = (value: string) => {
    if (value.trim().length === 0) return;
    if (queues) {
      // Queueing saves nothing yet, so nothing locks: the answer shows in its own row and the box
      // is free for a change of mind.
      void onAnswer(item, value);
      setText('');
      return;
    }
    void act(() => onAnswer(item, value));
  };
  const cancelQueued = () => {
    const words = onCancelQueued(item);
    if (words !== null) setText(words);
  };

  return (
    <section
      data-testid="needs-you-card"
      data-item-id={item.id}
      data-folded={folded ? 'true' : 'false'}
      aria-label={intl.formatMessage(i18n.title)}
      className={cx(CARD, RADIUS.card, cardLayout(folded))}
    >
      <FoldBand
        folded={folded}
        onToggle={onToggleFold}
        bodyId={bodyId}
        icon={<Hand aria-hidden />}
        title={intl.formatMessage(i18n.title)}
        summary={item.question}
        queued={queued !== null}
        position={total > 1 ? intl.formatMessage(i18n.position, { index, total }) : null}
      />

      {/* Folded, the body stays mounted (a typed answer survives the fold) but hidden. */}
      <div id={bodyId} hidden={folded} className="flex min-h-0 flex-col">
        <div
          data-testid="needs-you-card-scroll"
          className="flex min-h-0 flex-col gap-2 overflow-y-auto px-4 pb-2 pt-3"
        >
          <p data-testid="needs-you-question" className={cx(TYPE.h2, 'whitespace-pre-wrap')}>
            {item.question}
          </p>
          {item.why && (
            <p data-testid="needs-you-why" className={cx(TYPE.bodyMuted, 'whitespace-pre-wrap')}>
              {item.why}
            </p>
          )}

          {queued && (
            <div
              data-testid="needs-you-queued"
              className={cx(
                'flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 [&_svg]:size-4 [&_svg]:shrink-0',
                TONE_FILL.secondary,
                RADIUS.control
              )}
            >
              <Clock aria-hidden />
              <span className={cx('min-w-0 flex-1 text-lz-body', WEIGHT.semibold)}>
                {intl.formatMessage(queuedSending ? i18n.sending : i18n.queued)}
              </span>
              <Button
                variant="secondary"
                size="sm"
                data-testid="needs-you-queued-cancel"
                disabled={queuedSending}
                onClick={cancelQueued}
              >
                {intl.formatMessage(i18n.cancelQueued)}
              </Button>
              <p
                data-testid="needs-you-queued-answer"
                className="basis-full whitespace-pre-wrap break-words text-lz-body"
              >
                {queued.answer}
              </p>
            </div>
          )}

          <div className="flex flex-col gap-1">
            <span className={TYPE.zone}>{intl.formatMessage(i18n.recommended)}</span>
            <button
              type="button"
              data-testid="needs-you-recommended"
              disabled={locked}
              onClick={() => answer(item.recommendedAnswer)}
              className={cx(
                'inline-flex max-w-full items-start gap-2 self-start border border-lz-accent bg-lz-accent px-3 py-1.5 text-left text-lz-body text-lz-accent-ink hover:border-lz-accent-hover hover:bg-lz-accent-hover [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0',
                'disabled:pointer-events-none disabled:border-lz-border disabled:bg-lz-surface-2 disabled:text-lz-ink-3',
                WEIGHT.semibold,
                RADIUS.control,
                FOCUS,
                MOTION
              )}
            >
              <Check aria-hidden />
              <span className="whitespace-pre-wrap">{item.recommendedAnswer}</span>
            </button>
          </div>

          {options.length > 0 && (
            <div className="flex flex-col gap-1">
              <span className={TYPE.zone}>{intl.formatMessage(i18n.options)}</span>
              <div className="flex flex-wrap gap-1.5">
                {options.map((option) => (
                  <button
                    key={option}
                    type="button"
                    data-testid="needs-you-option"
                    disabled={locked}
                    onClick={() => answer(option)}
                    className={cx(
                      // Q-315: an option is a sentence; at 460 px it wraps, so the chip grows in
                      // height with its text (never a fixed h-7 the text spills out of) and takes the
                      // chip radius, which holds two lines where the 999 pill clipped them.
                      'inline-flex min-h-7 max-w-full items-center whitespace-normal break-words border border-lz-border-strong bg-lz-surface px-2.5 py-1 text-left text-[12px] text-lz-ink hover:bg-lz-surface-2',
                      'disabled:pointer-events-none disabled:bg-lz-surface-2 disabled:text-lz-ink-3',
                      WEIGHT.medium,
                      RADIUS.control,
                      FOCUS,
                      MOTION
                    )}
                  >
                    {option}
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>

        <div
          data-testid="needs-you-card-footer"
          className="flex shrink-0 flex-col gap-2 border-t border-lz-border px-4 pb-3 pt-2"
        >
          <textarea
            data-testid="needs-you-input"
            rows={2}
            value={text}
            disabled={locked}
            placeholder={intl.formatMessage(i18n.placeholder)}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                answer(text);
              }
            }}
            className={cx(
              'w-full resize-y border border-lz-border-strong bg-lz-surface px-2 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
              'disabled:bg-lz-surface-2 disabled:text-lz-ink-3',
              RADIUS.control,
              FOCUS,
              MOTION
            )}
          />

          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="secondary"
              size="sm"
              data-testid="needs-you-answer"
              disabled={locked || text.trim().length === 0}
              onClick={() => answer(text)}
            >
              {intl.formatMessage(i18n.answer)}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              icon={<X />}
              data-testid="needs-you-dismiss"
              disabled={locked}
              onClick={() => void act(() => onDismiss(item))}
            >
              {intl.formatMessage(i18n.dismiss)}
            </Button>
            {queues && !queued && (
              <span data-testid="needs-you-busy" className="min-w-0 text-lz-meta text-lz-ink-3">
                {intl.formatMessage(i18n.busyQueues)}
              </span>
            )}
          </div>
          {error && (
            // The size utility alone with the err ink: TYPE.meta carries ink-3, which wins (c16f1d5f1).
            <p role="alert" className="text-lz-meta text-lz-err">
              {intl.formatMessage(i18n.failed, { error })}
            </p>
          )}
        </div>
      </div>
    </section>
  );
}

interface NeedsYouTrayProps {
  sessionId: string;
  chatState: ChatState;
  /** The chat refuses a message while a stop is being settled; queued answers wait for it too. */
  sendBlocked?: boolean;
  /** Sends the answer to the model as the person's next chat message. */
  sendAnswer: (text: string) => void;
  submitElicitationResponse?: (
    elicitationId: string,
    userData: Record<string, unknown>
  ) => Promise<boolean>;
  className?: string;
}

/**
 * Pinned above the composer — outside the scrolling conversation, so a question never scrolls away.
 * It holds this session's open `ask_user` questions and any live MCP elicitation, and stays until
 * each is answered or dismissed. Every card folds to one line and two or more fold to one bar
 * (Q-340); an answer given while the turn runs is queued and sent when it ends (Q-341).
 */
export default function NeedsYouTray({
  sessionId,
  chatState,
  sendBlocked = false,
  sendAnswer,
  submitElicitationResponse,
  className,
}: NeedsYouTrayProps) {
  const intl = useIntl();
  const stackBodyId = useId();
  const activity = useSessionActivity();
  const queue = useAnswerQueue(sessionId);
  const items = useMemo(
    () => activity.needsYou.filter((item) => item.sessionId === sessionId),
    [activity.needsYou, sessionId]
  );
  const elicitations = useMemo(
    () =>
      submitElicitationResponse
        ? activity.elicitations.filter((request) => request.sessionId === sessionId)
        : [],
    [activity.elicitations, sessionId, submitElicitationResponse]
  );
  const openIds = useMemo(() => new Set(items.map((item) => item.id)), [items]);
  const fold = useNeedsYouFold(sessionId, [
    ...items.map((item) => item.id),
    ...elicitations.map((request) => request.id),
  ]);

  const idle = chatState === ChatState.Idle && !sendBlocked;
  const queues = !idle || answersWaiting(queue);

  // The turn ended: what was queued goes now, before anything the composer holds (the order is
  // documented and tested in needsYouAnswerQueue.ts).
  const hasQueued = queue.queued.length > 0;
  useEffect(() => {
    if (idle && hasQueued) void deliverQueuedAnswers(sessionId, openIds, sendAnswer);
  }, [idle, hasQueued, sessionId, openIds, sendAnswer]);

  // An answer queued for a question that has since closed is said so at once — not only when the
  // turn ends.
  const orphans = queue.queued.filter(
    (entry) => !openIds.has(entry.itemId) && !queue.sending.includes(entry.itemId)
  );
  const cards = items.length + elicitations.length;
  if (cards === 0 && queue.unsent.length === 0 && orphans.length === 0) return null;

  const onAnswer = async (item: NeedsYouItemDto, answer: string) => {
    if (queues) {
      enqueueAnswer(item, answer);
      return;
    }
    // The item is closed on the engine first: if that fails the card stays, answer still typed.
    await resolveNeedsYou(item, 'answer', answer);
    sendAnswer(answerMessage(item.question, answer));
  };
  const onCancelQueued = (item: NeedsYouItemDto) =>
    cancelQueuedAnswer(sessionId, item.id)?.answer ?? null;
  // Dismissing sends nothing to the model, so it never waits for the turn: it closes at once.
  const onDismiss = async (item: NeedsYouItemDto) => {
    await resolveNeedsYou(item, 'dismiss');
    cancelQueuedAnswer(sessionId, item.id);
  };

  const stacked = cards > 1;
  const stackOpen = !stacked || !fold.stackFolded;
  const firstQuestion = items[0]?.question ?? elicitations[0]?.request.message ?? '';
  const queuedCount = queue.queued.length;

  const notices = [
    ...queue.unsent.map((unsent) => ({ ...unsent, key: `unsent:${unsent.itemId}` })),
    ...orphans.map((orphan) => ({
      ...orphan,
      reason: 'closed' as const,
      error: null,
      key: `orphan:${orphan.itemId}`,
    })),
  ];

  return (
    <div data-testid="needs-you-tray" className={cx('flex flex-col gap-2', className)}>
      {notices.map((notice) => (
        <div
          key={notice.key}
          role="alert"
          data-testid="needs-you-unsent"
          className={cx(
            'flex shrink-0 items-start gap-2 px-3 py-2 text-lz-body',
            TONE_FILL.err,
            RADIUS.card
          )}
        >
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className={WEIGHT.semibold}>
              {notice.reason === 'closed'
                ? intl.formatMessage(i18n.unsentClosed)
                : intl.formatMessage(i18n.unsentFailed, { error: notice.error ?? '' })}
            </span>
            <span className="truncate">{notice.question}</span>
            <span className="whitespace-pre-wrap break-words">
              {intl.formatMessage(i18n.unsentAnswer, { answer: notice.answer })}
            </span>
          </div>
          <Button
            variant="secondary"
            size="sm"
            iconOnly
            aria-label={intl.formatMessage(i18n.closeNotice)}
            data-testid="needs-you-unsent-close"
            icon={<X />}
            onClick={() =>
              notice.key.startsWith('orphan:')
                ? cancelQueuedAnswer(sessionId, notice.itemId)
                : closeUnsentAnswer(sessionId, notice.itemId)
            }
          />
        </div>
      ))}

      {stacked && (
        <button
          type="button"
          data-testid="needs-you-stack"
          aria-expanded={stackOpen}
          aria-controls={stackBodyId}
          onClick={fold.toggleStack}
          className={cx(
            'flex w-full min-w-0 shrink-0 items-center gap-2 px-3 py-1.5 text-left [&>svg]:size-4 [&>svg]:shrink-0',
            BAND_TONE,
            RADIUS.card,
            FOCUS,
            MOTION
          )}
        >
          <ChevronRight aria-hidden className={cx(stackOpen && 'rotate-90')} />
          <Hand aria-hidden />
          <span className={cx('shrink-0 text-lz-body', WEIGHT.semibold)}>
            {intl.formatMessage(i18n.stack, { count: cards })}
          </span>
          <span
            data-testid="needs-you-stack-first"
            className="min-w-0 flex-1 truncate text-lz-body"
          >
            — {firstQuestion}
          </span>
          {queuedCount > 0 && (
            <span data-testid="needs-you-stack-queued" className={BAND_CHIP}>
              <Clock aria-hidden />
              {intl.formatMessage(i18n.stackQueued, { count: queuedCount })}
            </span>
          )}
        </button>
      )}

      {cards > 0 && (
        <div
          id={stackBodyId}
          hidden={!stackOpen}
          data-testid="needs-you-list"
          // The cards never shrink (shrink-0): squeezed into the height cap they clipped their own
          // action row under the border (Q-340). The list scrolls instead.
          className="flex max-h-[45vh] flex-col gap-2 overflow-y-auto"
        >
          {items.map((item, i) => (
            <QuestionCard
              key={item.id}
              item={item}
              index={i + 1}
              total={cards}
              queues={queues}
              queued={queue.queued.find((entry) => entry.itemId === item.id) ?? null}
              sending={queue.sending.includes(item.id)}
              folded={fold.isFolded(item.id)}
              onToggleFold={() => fold.toggle(item.id)}
              onAnswer={onAnswer}
              onCancelQueued={onCancelQueued}
              onDismiss={onDismiss}
            />
          ))}
          {submitElicitationResponse &&
            elicitations.map((request, i) => (
              <ElicitationCard
                key={request.id}
                id={request.id}
                message={request.request.message}
                position={
                  cards > 1
                    ? intl.formatMessage(i18n.position, {
                        index: items.length + i + 1,
                        total: cards,
                      })
                    : null
                }
                folded={fold.isFolded(request.id)}
                onToggleFold={() => fold.toggle(request.id)}
              >
                <ElicitationRequest
                  isCancelledMessage={false}
                  isClicked={false}
                  actionRequiredContent={{
                    type: 'actionRequired',
                    data: {
                      actionType: 'elicitation',
                      id: request.id,
                      message: request.request.message,
                      requested_schema: request.request.requestedSchema,
                    },
                  }}
                  onSubmit={submitElicitationResponse}
                />
              </ElicitationCard>
            ))}
        </div>
      )}
    </div>
  );
}

interface ElicitationCardProps {
  id: string;
  message: string;
  position: string | null;
  folded: boolean;
  onToggleFold: () => void;
  children: ReactNode;
}

function ElicitationCard({
  id,
  message,
  position,
  folded,
  onToggleFold,
  children,
}: ElicitationCardProps) {
  const intl = useIntl();
  const bodyId = useId();
  return (
    <section
      data-testid="needs-you-elicitation"
      data-item-id={id}
      data-folded={folded ? 'true' : 'false'}
      className={cx(CARD, RADIUS.card, cardLayout(folded))}
    >
      <FoldBand
        folded={folded}
        onToggle={onToggleFold}
        bodyId={bodyId}
        icon={<PlugZap aria-hidden />}
        title={intl.formatMessage(i18n.extensionTitle)}
        summary={message}
        queued={false}
        position={position}
      />
      <div id={bodyId} hidden={folded} className="min-h-0 overflow-y-auto px-2 py-2">
        {children}
      </div>
    </section>
  );
}
