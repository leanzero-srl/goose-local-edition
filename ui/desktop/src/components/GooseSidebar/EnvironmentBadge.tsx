import React from 'react';
import { Tooltip, TooltipContent, TooltipTrigger } from '../ui/Tooltip';
import { Chip, cx } from '../lz';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  dev: {
    id: 'environmentBadge.dev',
    defaultMessage: 'Dev',
  },
});

interface EnvironmentBadgeProps {
  className?: string;
}

/** The dev-build marker: a warn-tone Chip (a state colour with meaning), never a hand-written orange. */
const EnvironmentBadge: React.FC<EnvironmentBadgeProps> = ({ className = '' }) => {
  const intl = useIntl();
  const isDevelopment = import.meta.env.DEV;

  if (!isDevelopment) {
    return null;
  }

  const tooltipText = intl.formatMessage(i18n.dev);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          className={cx('relative cursor-default no-drag', className)}
          data-testid="environment-badge"
          aria-label={tooltipText}
        >
          <Chip tone="warn">{tooltipText}</Chip>
        </div>
      </TooltipTrigger>
      <TooltipContent side="bottom">{tooltipText}</TooltipContent>
    </Tooltip>
  );
};

export default EnvironmentBadge;
