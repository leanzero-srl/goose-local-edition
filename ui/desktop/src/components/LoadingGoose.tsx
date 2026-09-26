import { useState } from 'react';
import type { FormingStatus } from '@aaif/goose-sdk';
import { ChevronDown } from 'lucide-react';
import GooseLogo from './GooseLogo';
import AnimatedIcons from './AnimatedIcons';
import FlyingBird from './FlyingBird';
import { ChatState } from '../types/chatState';
import { defineMessages, useIntl } from '../i18n';
import { MOTION, SURFACE, TNUM, TYPE, WEIGHT, cx } from './lz';

interface LoadingGooseProps {
  message?: string;
  chatState?: ChatState;
  /** A response still forming tool calls: listed behind the line's disclosure (Q-151). */
  forming?: FormingStatus | null;
}

const i18n = defineMessages({
  loadingConversation: {
    id: 'loadingGoose.loadingConversation',
    defaultMessage: 'loading conversation...',
  },
  thinking: {
    id: 'loadingGoose.thinking',
    defaultMessage: 'goose is thinking…',
  },
  streaming: {
    id: 'loadingGoose.streaming',
    defaultMessage: 'goose is working on it…',
  },
  waiting: {
    id: 'loadingGoose.waiting',
    defaultMessage: 'goose is waiting…',
  },
  compacting: {
    id: 'loadingGoose.compacting',
    defaultMessage: 'goose is compacting the conversation...',
  },
  idle: {
    id: 'loadingGoose.idle',
    defaultMessage: 'goose is working on it…',
  },
  restartingAgent: {
    id: 'loadingGoose.restartingAgent',
    defaultMessage: 'restarting session...',
  },
  showForming: {
    id: 'loadingGoose.showForming',
    defaultMessage: 'Show what is forming',
  },
  hideForming: {
    id: 'loadingGoose.hideForming',
    defaultMessage: 'Hide',
  },
  formingCallsHeading: {
    id: 'loadingGoose.formingCallsHeading',
    defaultMessage: '{count, plural, one {# tool call forming} other {# tool calls forming}}',
  },
  formingReceived: {
    id: 'loadingGoose.formingReceived',
    defaultMessage:
      'Received so far: {arguments} chars of arguments{reasoning, select, none {} other {, {reasoning} chars of reasoning}}',
  },
  formingCallChars: {
    id: 'loadingGoose.formingCallChars',
    defaultMessage: '{chars} chars',
  },
  formingTextHeading: {
    id: 'loadingGoose.formingTextHeading',
    defaultMessage: 'Text written beside the calls ({chars} chars) — not part of the answer',
  },
});

const STATE_ICONS: Record<ChatState, React.ReactNode> = {
  [ChatState.LoadingConversation]: <AnimatedIcons className="flex-shrink-0" cycleInterval={600} />,
  [ChatState.Thinking]: <AnimatedIcons className="flex-shrink-0" cycleInterval={600} />,
  [ChatState.Streaming]: <FlyingBird className="flex-shrink-0" cycleInterval={150} />,
  [ChatState.WaitingForUserInput]: (
    <AnimatedIcons className="flex-shrink-0" cycleInterval={600} variant="waiting" />
  ),
  [ChatState.Compacting]: <AnimatedIcons className="flex-shrink-0" cycleInterval={600} />,
  [ChatState.Idle]: <GooseLogo size="small" hover={false} />,
  [ChatState.RestartingAgent]: <AnimatedIcons className="flex-shrink-0" cycleInterval={600} />,
};

const STATE_MESSAGE_KEYS: Record<ChatState, keyof typeof i18n> = {
  [ChatState.LoadingConversation]: 'loadingConversation',
  [ChatState.Thinking]: 'thinking',
  [ChatState.Streaming]: 'streaming',
  [ChatState.WaitingForUserInput]: 'waiting',
  [ChatState.Compacting]: 'compacting',
  [ChatState.Idle]: 'idle',
  [ChatState.RestartingAgent]: 'restartingAgent',
};

const LoadingGoose = ({
  message,
  chatState = ChatState.Idle,
  forming = null,
}: LoadingGooseProps) => {
  const intl = useIntl();
  const [open, setOpen] = useState(false);
  const displayMessage = message || intl.formatMessage(i18n[STATE_MESSAGE_KEYS[chatState]]);
  const icon = STATE_ICONS[chatState];
  const hasForming = forming != null && forming.calls.length > 0;

  return (
    <div className="relative w-full animate-fade-slide-up">
      {hasForming && open && <FormingPanel forming={forming} />}
      <div
        data-testid="loading-indicator"
        className="flex items-center gap-2 text-xs text-text-primary py-2"
      >
        {icon}
        <span data-testid="loading-indicator-message">{displayMessage}</span>
        {hasForming && (
          // The line sits in a pointer-events-none overlay (BaseChat): the toggle opts back in.
          <button
            type="button"
            aria-expanded={open}
            data-testid="loading-indicator-forming-toggle"
            onClick={() => setOpen((o) => !o)}
            className={cx(
              'pointer-events-auto inline-flex items-center gap-1 underline text-lz-meta text-lz-accent hover:text-lz-ink',
              WEIGHT.semibold,
              MOTION
            )}
          >
            {intl.formatMessage(open ? i18n.hideForming : i18n.showForming)}
            <ChevronDown aria-hidden className={cx('size-3.5', open && 'rotate-180')} />
          </button>
        )}
      </div>
    </div>
  );
};

function FormingPanel({ forming }: { forming: FormingStatus }) {
  const intl = useIntl();
  const count = (n: number) => intl.formatNumber(n);
  return (
    <div
      data-testid="loading-indicator-forming"
      className={cx(
        SURFACE.overlay,
        'pointer-events-auto absolute bottom-full left-0 mb-1 flex max-h-[50vh] w-[min(640px,calc(100vw-4rem))] flex-col gap-2 overflow-y-auto p-3'
      )}
    >
      <p className={cx(TYPE.body, WEIGHT.semibold)}>
        {intl.formatMessage(i18n.formingCallsHeading, { count: forming.calls.length })}
      </p>
      <p className={cx(TYPE.meta, TNUM)}>
        {intl.formatMessage(i18n.formingReceived, {
          arguments: count(forming.argumentChars),
          reasoning: forming.reasoningChars > 0 ? count(forming.reasoningChars) : 'none',
        })}
      </p>
      <ol data-testid="loading-indicator-forming-calls" className="flex flex-col">
        {forming.calls.map((call, i) => (
          <li
            key={i}
            title={call.name}
            className={cx('flex items-baseline justify-between gap-3 py-0.5', TYPE.body)}
          >
            <span className="min-w-0 truncate">
              <span className={cx(TYPE.meta, TNUM, 'mr-2')}>{i + 1}.</span>
              {call.title}
            </span>
            <span className={cx(TYPE.meta, TNUM, 'shrink-0')}>
              {intl.formatMessage(i18n.formingCallChars, { chars: count(call.argumentChars) })}
            </span>
          </li>
        ))}
      </ol>
      {forming.text.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className={cx(TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.formingTextHeading, {
              chars: count(Array.from(forming.text).length),
            })}
          </p>
          <pre
            data-testid="loading-indicator-forming-text"
            className={cx(
              SURFACE.inset,
              'whitespace-pre-wrap break-words rounded-lz-control p-2 font-mono text-lz-mono text-lz-ink'
            )}
          >
            {forming.text}
          </pre>
        </div>
      )}
    </div>
  );
}

export default LoadingGoose;
