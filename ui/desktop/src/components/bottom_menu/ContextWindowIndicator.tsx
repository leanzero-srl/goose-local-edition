import BottomMenuAlertPopover from './BottomMenuAlertPopover';
import { Alert } from '../alerts';
import { Chip, TNUM, TONE_TEXT, cx } from '../lz';
import { defineMessages, useIntl } from '../../i18n';

interface ContextWindowIndicatorProps {
  totalTokens: number;
  tokenLimit: number;
  alerts: Alert[];
  /**
   * Tokens the engine has written for the turn in flight, from its live request (Q-153): the
   * context grows by them while goose reports usage only when the turn ends. 0 = none measured.
   */
  liveTokens?: number;
}

const i18n = defineMessages({
  liveTitle: {
    id: 'contextWindowIndicator.liveTitle',
    defaultMessage:
      '{context} tokens of context + {live} being written now, of a {limit}-token window',
  },
});

const formatTokenCount = (count: number): string => {
  if (count >= 1_000_000) return `${Math.round(count / 1_000_000)}M`;
  if (count >= 1_000) return `${Math.round(count / 1_000)}k`;
  return count.toString();
};

const getProgressColor = (percentage: number): string => {
  if (percentage <= 75) return 'text-lz-ink-3';
  if (percentage <= 90) return TONE_TEXT.warn;
  return TONE_TEXT.err;
};

export function ContextWindowIndicator({
  totalTokens,
  tokenLimit,
  alerts,
  liveTokens = 0,
}: ContextWindowIndicatorProps) {
  const intl = useIntl();
  if (!tokenLimit) return null;

  const live = liveTokens > 0 ? liveTokens : 0;
  const percentage = Math.round(((totalTokens + live) / tokenLimit) * 100);
  const colorClass = getProgressColor(percentage);

  return (
    <Chip>
      <div className="flex items-center h-full">
        <BottomMenuAlertPopover alerts={alerts}>
          <span
            data-testid="context-window-indicator"
            title={
              live > 0
                ? intl.formatMessage(i18n.liveTitle, {
                    context: intl.formatNumber(totalTokens),
                    live: intl.formatNumber(live),
                    limit: intl.formatNumber(tokenLimit),
                  })
                : undefined
            }
            className={cx('text-lz-meta', TNUM, colorClass)}
          >
            {formatTokenCount(totalTokens)}
            {live > 0 && (
              <span data-testid="context-window-live" className="text-lz-accent font-lz-semibold">
                {' + '}
                {formatTokenCount(live)}
              </span>
            )}{' '}
            / {formatTokenCount(tokenLimit)}
          </span>
        </BottomMenuAlertPopover>
      </div>
    </Chip>
  );
}
