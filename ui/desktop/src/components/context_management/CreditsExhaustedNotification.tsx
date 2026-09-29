import React from 'react';
import { AlertTriangle, ExternalLink } from 'lucide-react';
import type { Message, SystemNotificationContent } from '../../types/message';
import { WEB_PROTOCOLS } from '../../utils/urlSecurity';
import { defineMessages, useIntl } from '../../i18n';
import { CALLOUT, TONE_TEXT, cx } from '../lz/tokens';

const i18n = defineMessages({
  insufficientCredits: {
    id: 'creditsExhaustedNotification.insufficientCredits',
    defaultMessage: 'Insufficient Credits',
  },
  addCredits: {
    id: 'creditsExhaustedNotification.addCredits',
    defaultMessage: 'Add credits',
  },
});

interface CreditsExhaustedNotificationProps {
  notification: SystemNotificationContent;
}

function getValidatedTopUpUrl(data: unknown): string | null {
  if (!data || typeof data !== 'object') {
    return null;
  }

  const rawUrl = (data as Record<string, unknown>).top_up_url;
  if (typeof rawUrl !== 'string') {
    return null;
  }

  const url = rawUrl.trim();
  if (!url) {
    return null;
  }

  try {
    const parsedUrl = new URL(url);
    if (!WEB_PROTOCOLS.includes(parsedUrl.protocol)) {
      return null;
    }
    return parsedUrl.toString();
  } catch {
    return null;
  }
}

export const CreditsExhaustedNotification: React.FC<CreditsExhaustedNotificationProps> = ({
  notification,
}) => {
  const intl = useIntl();
  const topUpUrl = getValidatedTopUpUrl(notification.data);

  const handleTopUp = () => {
    if (topUpUrl) {
      window.electron.openExternal(topUpUrl);
    }
  };

  return (
    <div className={cx('rounded-lg p-4 my-2', CALLOUT.warn)}>
      <div className="flex items-start gap-3">
        <AlertTriangle className={cx('h-4 w-4 mt-0.5 shrink-0', TONE_TEXT.warn)} />
        <div className="flex-1">
          <div className="text-sm font-semibold">
            {intl.formatMessage(i18n.insufficientCredits)}
          </div>
          <div className="text-sm mt-1">{notification.msg}</div>
          {topUpUrl && (
            <button
              onClick={handleTopUp}
              className="mt-3 inline-flex items-center gap-2 rounded-md bg-lz-accent hover:bg-lz-accent-hover text-lz-accent-ink text-sm font-medium px-4 py-2 transition-colors"
            >
              {intl.formatMessage(i18n.addCredits)}
              <ExternalLink className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};

export function getCreditsExhaustedNotification(
  message: Message
): SystemNotificationContent | undefined {
  return message.content.find(
    (content): content is SystemNotificationContent & { type: 'systemNotification' } =>
      content.type === 'systemNotification' && content.notificationType === 'creditsExhausted'
  );
}
