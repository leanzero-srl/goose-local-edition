import { useState } from 'react';
import { MemoryStick } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, type ButtonVariant } from '../lz';
import { useEditionIsLocal } from '../../contexts/EditionContext';
import { FreeMemoryDialog } from './FreeMemoryDialog';
import { useMemfreeEnabled } from './useMemfreeEnabled';

const i18n = defineMessages({
  open: { id: 'freeMemoryEntry.open', defaultMessage: 'Free up memory' },
});

/**
 * The "Free up memory" button and the dialog it opens, for one Mac. Shown only in the local edition
 * and while GOOSE_MEMFREE_ENABLED is on. `target` is the peer's Link node id, null for this Mac.
 */
export function FreeMemoryEntry({
  target,
  macName,
  fitModelId,
  variant = 'secondary',
  testId = 'free-memory-open',
}: {
  target: string | null;
  macName: string;
  fitModelId?: string | null;
  variant?: ButtonVariant;
  testId?: string;
}) {
  const intl = useIntl();
  const isLocal = useEditionIsLocal();
  const enabled = useMemfreeEnabled();
  const [open, setOpen] = useState(false);
  if (!isLocal || !enabled) return null;
  return (
    <>
      <Button
        variant={variant}
        size="sm"
        icon={<MemoryStick />}
        data-testid={testId}
        onClick={() => setOpen(true)}
      >
        {intl.formatMessage(i18n.open)}
      </Button>
      {open && (
        <FreeMemoryDialog
          target={target ?? undefined}
          macName={macName}
          fitModelId={fitModelId}
          onClose={() => setOpen(false)}
        />
      )}
    </>
  );
}
