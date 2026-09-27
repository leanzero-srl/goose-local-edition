import React from 'react';
import { useIntl } from '../../i18n';
import type { Message, SystemNotificationContent } from '../../types/message';
import { TYPE, cx } from '../lz/tokens';
import { StoppedPill } from '../sessionActivity/ActivityPills';
import { stoppedTurnOf, stoppedTurnText } from '../sessionActivity/stoppedTurnText';

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

export const SystemNotificationInline: React.FC<SystemNotificationInlineProps> = ({
  notification,
}) => {
  const stopped = stoppedTurnOf(notification.data);
  if (stopped) {
    return <StoppedTurnLine elapsedMs={stopped.elapsedMs} outputTokens={stopped.outputTokens} />;
  }
  return <div className="text-xs text-gray-400 py-2 text-left">{notification.msg}</div>;
};

export function getInlineSystemNotification(
  message: Message
): SystemNotificationContent | undefined {
  return message.content.find(
    (content): content is SystemNotificationContent & { type: 'systemNotification' } =>
      content.type === 'systemNotification' && content.notificationType === 'inlineMessage'
  );
}
