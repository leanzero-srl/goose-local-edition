import React from 'react';
import { Cloud, Cpu } from 'lucide-react';
import { Button } from '../ui/button';
import { useEdition } from '../../contexts/EditionContext';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  edition: { id: 'editionSelector.edition', defaultMessage: 'Edition' },
  standard: { id: 'editionSelector.standard', defaultMessage: 'Standard' },
  local: { id: 'editionSelector.local', defaultMessage: 'Local' },
});

interface EditionSelectorProps {
  className?: string;
  hideTitle?: boolean;
  horizontal?: boolean;
}

/**
 * Goose vs Goose Local Edition selector — a segmented control mirroring ThemeSelector. Active option is a
 * solid inverse fill (not a faded tint); no left rail. Reselectable any time.
 */
const EditionSelector: React.FC<EditionSelectorProps> = ({
  className = '',
  hideTitle = false,
  horizontal = false,
}) => {
  const intl = useIntl();
  const { edition, setEdition } = useEdition();

  const buttonClass = (active: boolean) =>
    `flex items-center justify-center gap-1 p-2 rounded-md border transition-colors text-xs ${
      active
        ? 'bg-background-inverse text-text-inverse border-text-inverse hover:!bg-background-inverse hover:!text-text-inverse'
        : 'border-border-primary hover:!bg-background-secondary text-text-secondary hover:text-text-primary'
    }`;

  return (
    <div className={`${!horizontal ? 'px-1 py-2 space-y-2' : ''} ${className}`}>
      {!hideTitle && (
        <div className="text-xs text-text-primary px-3">{intl.formatMessage(i18n.edition)}</div>
      )}
      <div
        className={`${horizontal ? 'flex' : 'grid grid-cols-2'} gap-1 ${!horizontal ? 'px-3' : ''}`}
      >
        <Button
          data-testid="edition-standard-button"
          onClick={() => setEdition('standard')}
          className={buttonClass(edition === 'standard')}
          variant="ghost"
          size="sm"
        >
          <Cloud className="h-3 w-3" />
          <span>{intl.formatMessage(i18n.standard)}</span>
        </Button>

        <Button
          data-testid="edition-local-button"
          onClick={() => setEdition('local')}
          className={buttonClass(edition === 'local')}
          variant="ghost"
          size="sm"
        >
          <Cpu className="h-3 w-3" />
          <span>{intl.formatMessage(i18n.local)}</span>
        </Button>
      </div>
    </div>
  );
};

export default EditionSelector;
