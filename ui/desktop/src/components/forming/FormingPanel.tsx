import type { CSSProperties } from 'react';
import type { FormingStatus } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import { SURFACE, TNUM, TYPE, WEIGHT, cx } from '../lz';

const i18n = defineMessages({
  callsHeading: {
    id: 'formingPanel.callsHeading',
    defaultMessage: '{count, plural, one {# tool call forming} other {# tool calls forming}}',
  },
  received: {
    id: 'formingPanel.received',
    defaultMessage:
      'Received so far: {arguments} chars of arguments{reasoning, select, none {} other {, {reasoning} chars of reasoning}}',
  },
  callChars: {
    id: 'formingPanel.callChars',
    defaultMessage: '{chars} chars',
  },
  textHeading: {
    id: 'formingPanel.textHeading',
    defaultMessage: 'Text written beside the calls ({chars} chars) — not part of the answer',
  },
  repeatsOfOne: {
    id: 'formingPanel.repeatsOfOne',
    defaultMessage:
      '{count, plural, one {# of them is identical to an earlier {tool} call} other {# of them are identical to an earlier {tool} call}}',
  },
  repeatsOfMany: {
    id: 'formingPanel.repeatsOfMany',
    defaultMessage:
      '{count, plural, one {# of them is identical to an earlier call} other {# of them are identical to earlier calls}}',
  },
});

/**
 * What a turn's response is still forming (Q-151): every tool call by the name the chat gives it,
 * what has arrived of their arguments, and the text written beside them. Opened from the engine card
 * for the chat it serves (EngineGlanceCard); the caller places it.
 */
export function FormingPanel({
  forming,
  className,
  style,
}: {
  forming: FormingStatus;
  className?: string;
  style?: CSSProperties;
}) {
  const intl = useIntl();
  const count = (n: number) => intl.formatNumber(n);
  return (
    <div
      data-testid="forming-panel"
      style={style}
      className={cx(
        SURFACE.overlay,
        'flex flex-col gap-2 overflow-y-auto p-3 text-lz-ink',
        className
      )}
    >
      <p className={cx(TYPE.body, WEIGHT.semibold)}>
        {intl.formatMessage(i18n.callsHeading, { count: forming.calls.length })}
      </p>
      <p className={cx(TYPE.meta, TNUM)}>
        {intl.formatMessage(i18n.received, {
          arguments: count(forming.argumentChars),
          reasoning: forming.reasoningChars > 0 ? count(forming.reasoningChars) : 'none',
        })}
      </p>
      {(forming.repeatedCalls ?? 0) > 0 && (
        <p data-testid="forming-panel-repeats" className={cx(TYPE.meta, WEIGHT.semibold, TNUM)}>
          {forming.repeatedTitle
            ? intl.formatMessage(i18n.repeatsOfOne, {
                count: forming.repeatedCalls,
                tool: forming.repeatedTitle,
              })
            : intl.formatMessage(i18n.repeatsOfMany, { count: forming.repeatedCalls })}
        </p>
      )}
      <ol data-testid="forming-panel-calls" className="flex flex-col">
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
              {intl.formatMessage(i18n.callChars, { chars: count(call.argumentChars) })}
            </span>
          </li>
        ))}
      </ol>
      {forming.text.length > 0 && (
        <div className="flex flex-col gap-1">
          <p className={cx(TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.textHeading, {
              chars: count(Array.from(forming.text).length),
            })}
          </p>
          <pre
            data-testid="forming-panel-text"
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
