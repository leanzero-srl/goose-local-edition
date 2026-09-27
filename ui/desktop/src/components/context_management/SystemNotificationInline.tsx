import React from 'react';
import { AlertTriangle } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Chip } from '../lz';
import type { Message, SystemNotificationContent } from '../../types/message';
import { TYPE, cx } from '../lz/tokens';
import { StoppedPill } from '../sessionActivity/ActivityPills';
import { stoppedTurnOf, stoppedTurnText } from '../sessionActivity/stoppedTurnText';

const i18n = defineMessages({
  check: { id: 'systemNotificationInline.check', defaultMessage: 'goose check' },
});

/** The engine's prefix for a finding about the reply (crates/goose/src/claim_check.rs CHECK_PREFIX). */
export const CHECK_PREFIX = 'goose check:';

interface SystemNotificationInlineProps {
  notification: SystemNotificationContent;
}

/** Q-169: where a stopped answer would have been — "You stopped this answer after 6 min · 1.9k tokens". */
function StoppedTurnLine({
  elapsedMs,
  outputTokens,
}: {
  elapsedMs: number;
  outputTokens?: number;
}) {
  const intl = useIntl();
  return (
    <div data-testid="stopped-turn-line" className="flex items-center gap-2 py-2 text-left">
      <StoppedPill elapsedMs={elapsedMs} outputTokens={outputTokens} />
      <span className={cx(TYPE.body, 'font-lz-semibold')}>
        {stoppedTurnText(intl, elapsedMs, outputTokens)}
      </span>
    </div>
  );
}

/**
 * goose's own line in the transcript. Read as body copy (ink-2), never the faded 12px grey it was
 * (Q-173). A goose check — the reply contradicting itself or the turn's tool results — leads with a
 * SOLID warning chip, so a finding never reads like a quiet status line.
 */
export const SystemNotificationInline: React.FC<SystemNotificationInlineProps> = ({
  notification,
}) => {
  const intl = useIntl();
  const stopped = stoppedTurnOf(notification.data);
  if (stopped) {
    return <StoppedTurnLine elapsedMs={stopped.elapsedMs} outputTokens={stopped.outputTokens} />;
  }
  const msg = notification.msg;
  if (msg.startsWith(CHECK_PREFIX)) {
    return (
      <div
        data-testid="system-notification-check"
        className="flex items-start gap-2 py-2 text-left"
      >
        <Chip tone="warn" icon={<AlertTriangle />} className="mt-px">
          {intl.formatMessage(i18n.check)}
        </Chip>
        <span className={cx(TYPE.body, 'min-w-0 whitespace-pre-wrap')}>
          {msg.slice(CHECK_PREFIX.length).trim()}
        </span>
      </div>
    );
  }
  return (
    <div
      data-testid="system-notification-inline"
      className={cx(TYPE.bodyMuted, 'whitespace-pre-wrap py-2 text-left')}
    >
      {msg}
    </div>
  );
};

export function getInlineSystemNotification(
  message: Message
): SystemNotificationContent | undefined {
  return message.content.find(
    (content): content is SystemNotificationContent & { type: 'systemNotification' } =>
      content.type === 'systemNotification' && content.notificationType === 'inlineMessage'
  );
}
