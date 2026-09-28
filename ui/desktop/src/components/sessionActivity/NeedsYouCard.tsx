import { useState } from 'react';
import { Check, Hand, PlugZap, X } from 'lucide-react';
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
  busy: {
    id: 'needsYouCard.busy',
    defaultMessage: 'goose is working — you can answer when this turn ends.',
  },
  failed: { id: 'needsYouCard.failed', defaultMessage: 'Could not save that: {error}' },
});

interface QuestionCardProps {
  item: NeedsYouItemDto;
  index: number;
  total: number;
  busy: boolean;
  onAnswer: (item: NeedsYouItemDto, answer: string) => Promise<void>;
  onDismiss: (item: NeedsYouItemDto) => Promise<void>;
}

/** Answer surfaces are solid: amber header band, full amber border — never a rail, never a wash. */
const CARD = 'overflow-hidden border-2 border-lz-warn-solid bg-lz-surface';
const BAND = cx('flex items-center gap-2 px-4 py-1.5 [&_svg]:size-4', TONE_FILL.warn);

export function QuestionCard({ item, index, total, busy, onAnswer, onDismiss }: QuestionCardProps) {
  const intl = useIntl();
  const [text, setText] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const locked = busy || sending;
  const options = pickOptions(item);

  const act = async (run: () => Promise<void>) => {
    setSending(true);
    setError(null);
    try {
      await run();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSending(false);
    }
  };
  const answer = (value: string) => {
    if (value.trim().length === 0) return;
    void act(() => onAnswer(item, value));
  };

  return (
    <section
      data-testid="needs-you-card"
      data-item-id={item.id}
      aria-label={intl.formatMessage(i18n.title)}
      className={cx(CARD, RADIUS.card)}
    >
      <div className={BAND}>
        <Hand aria-hidden />
        <span className={cx('text-lz-body', WEIGHT.semibold)}>
          {intl.formatMessage(i18n.title)}
        </span>
        {total > 1 && (
          <span className={cx('ml-auto text-lz-meta', TNUM)}>
            {intl.formatMessage(i18n.position, { index, total })}
          </span>
        )}
      </div>

      <div className="flex flex-col gap-2 px-4 py-3">
        <p data-testid="needs-you-question" className={cx(TYPE.h2, 'whitespace-pre-wrap')}>
          {item.question}
        </p>
        {item.why && (
          <p data-testid="needs-you-why" className={cx(TYPE.bodyMuted, 'whitespace-pre-wrap')}>
            {item.why}
          </p>
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
          {busy && <span className={TYPE.meta}>{intl.formatMessage(i18n.busy)}</span>}
        </div>
        {error && (
          <p role="alert" className={cx(TYPE.meta, 'text-lz-err')}>
            {intl.formatMessage(i18n.failed, { error })}
          </p>
        )}
      </div>
    </section>
  );
}

interface NeedsYouTrayProps {
  sessionId: string;
  chatState: ChatState;
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
 * each is answered or dismissed.
 */
export default function NeedsYouTray({
  sessionId,
  chatState,
  sendAnswer,
  submitElicitationResponse,
  className,
}: NeedsYouTrayProps) {
  const intl = useIntl();
  const activity = useSessionActivity();
  const items = activity.needsYou.filter((item) => item.sessionId === sessionId);
  const elicitations = activity.elicitations.filter((request) => request.sessionId === sessionId);
  if (items.length === 0 && elicitations.length === 0) return null;

  const busy = chatState !== ChatState.Idle;
  const onAnswer = async (item: NeedsYouItemDto, answer: string) => {
    // The item is closed on the engine first: if that fails the card stays, answer still typed.
    await resolveNeedsYou(item, 'answer', answer);
    sendAnswer(answerMessage(item.question, answer));
  };
  const onDismiss = (item: NeedsYouItemDto) => resolveNeedsYou(item, 'dismiss');

  return (
    <div
      data-testid="needs-you-tray"
      className={cx('flex max-h-[45vh] flex-col gap-2 overflow-y-auto', className)}
    >
      {items.map((item, i) => (
        <QuestionCard
          key={item.id}
          item={item}
          index={i + 1}
          total={items.length}
          busy={busy}
          onAnswer={onAnswer}
          onDismiss={onDismiss}
        />
      ))}
      {submitElicitationResponse &&
        elicitations.map((request) => (
          <section
            key={request.id}
            data-testid="needs-you-elicitation"
            className={cx(CARD, RADIUS.card)}
          >
            <div className={BAND}>
              <PlugZap aria-hidden />
              <span className={cx('text-lz-body', WEIGHT.semibold)}>
                {intl.formatMessage(i18n.extensionTitle)}
              </span>
            </div>
            <div className="px-2 py-2">
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
            </div>
          </section>
        ))}
    </div>
  );
}
