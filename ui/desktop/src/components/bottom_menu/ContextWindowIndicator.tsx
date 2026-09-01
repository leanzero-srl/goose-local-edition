import BottomMenuAlertPopover from './BottomMenuAlertPopover';
import { Alert } from '../alerts';
import { TNUM, TONE_TEXT, cx } from '../lz';

interface ContextWindowIndicatorProps {
  totalTokens: number;
  tokenLimit: number;
  alerts: Alert[];
}

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
}: ContextWindowIndicatorProps) {
  if (!tokenLimit) return null;

  const percentage = Math.round((totalTokens / tokenLimit) * 100);
  const colorClass = getProgressColor(percentage);

  return (
    <div className="flex items-center h-full">
      <BottomMenuAlertPopover alerts={alerts}>
        <span className={cx('text-lz-meta', TNUM, colorClass)}>
          {formatTokenCount(totalTokens)} / {formatTokenCount(tokenLimit)}
        </span>
      </BottomMenuAlertPopover>
    </div>
  );
}
