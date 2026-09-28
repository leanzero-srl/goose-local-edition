import { useId } from 'react';
import { Loader2 } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Checkbox, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';

/**
 * The one remove dialog of the Nodes page (a node, a strategy). Q-259: the live 3.0.65 dialog
 * showed the engine's first answer as a red "Not removed", printed the live-chat sentence twice
 * (the refusal's words, then the checkbox's), and its Remove button re-sent the same refused call
 * until an unexplained box was ticked. The rules this component carries:
 *
 * - A consequence the person must accept is a CONFIRMATION, never an error: its box says what
 *   ticking it does, beside the box, once. Consequences known from the config show when the dialog
 *   opens; one only the engine can count (live chats) shows the moment it answers.
 * - Remove is disabled while a box is unticked, and the reason is written next to it.
 * - Only a refusal no box answers is an error: red "Not removed" and the engine's own words, after
 *   the attempt that produced it.
 */

const i18n = defineMessages({
  cancel: { id: 'nodes.cancel', defaultMessage: 'Cancel' },
  removeConfirm: { id: 'nodes.removeConfirm', defaultMessage: 'Remove' },
  refused: { id: 'nodes.refused', defaultMessage: 'Not removed' },
  blocked: {
    id: 'nodes.removeBlocked',
    defaultMessage:
      '{count, plural, one {Tick the box above to remove it} other {Tick the # boxes above to remove it}}',
  },
});

export interface RemoveConfirmation {
  key: string;
  /** What ticking the box does, in words beside the box. */
  label: string;
  /** Why the removal needs it. */
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  testId: string;
}

export interface RemoveRefusal {
  code: string;
  message: string;
}

export interface RemoveConfirmDialogProps {
  title: string;
  body: string;
  confirmations: RemoveConfirmation[];
  /** Refusals no confirmation answers, from the last attempt: shown verbatim, in red. */
  refusals: RemoveRefusal[];
  busy: boolean;
  onConfirm: () => void;
  onClose: () => void;
  /** `node-remove` → `node-remove-dialog`, `-confirmations`, `-refusals`, `-blocked`, `-confirm`. */
  testIdPrefix: string;
}

export function RemoveConfirmDialog({
  title,
  body,
  confirmations,
  refusals,
  busy,
  onConfirm,
  onClose,
  testIdPrefix,
}: RemoveConfirmDialogProps) {
  const intl = useIntl();
  const blockedId = useId();
  const unticked = confirmations.filter((c) => !c.checked).length;
  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx('flex w-[30rem] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div data-testid={`${testIdPrefix}-dialog`} className="flex flex-col gap-2">
        <OverlayDialogTitle asChild>
          <h2 className={cx('break-words', TYPE.h2)}>{title}</h2>
        </OverlayDialogTitle>
        <p className={TYPE.body}>{body}</p>
      </div>
      {confirmations.length > 0 && (
        <div className="flex flex-col gap-2" data-testid={`${testIdPrefix}-confirmations`}>
          {confirmations.map((c) => (
            <Checkbox
              key={c.key}
              variant="card"
              checked={c.checked}
              onChange={c.onChange}
              label={c.label}
              description={c.description}
              testId={c.testId}
            />
          ))}
        </div>
      )}
      {refusals.length > 0 && (
        <div role="alert" className="flex flex-col gap-1" data-testid={`${testIdPrefix}-refusals`}>
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.refused)}
          </span>
          {refusals.map((r) => (
            <p key={r.code + r.message} className={cx('break-words', TYPE.body)}>
              {r.message}
            </p>
          ))}
        </div>
      )}
      <div className="flex flex-wrap items-center justify-end gap-2">
        {unticked > 0 && (
          <span
            id={blockedId}
            className={cx('mr-auto', TYPE.meta, WEIGHT.semibold)}
            data-testid={`${testIdPrefix}-blocked`}
          >
            {intl.formatMessage(i18n.blocked, { count: unticked })}
          </span>
        )}
        <Button variant="ghost" onClick={onClose}>
          {intl.formatMessage(i18n.cancel)}
        </Button>
        <Button
          variant="destructive"
          disabled={busy || unticked > 0}
          aria-describedby={unticked > 0 ? blockedId : undefined}
          icon={busy ? <Loader2 className="animate-spin" /> : undefined}
          onClick={onConfirm}
          data-testid={`${testIdPrefix}-confirm`}
        >
          {intl.formatMessage(i18n.removeConfirm)}
        </Button>
      </div>
    </OverlayDialog>
  );
}
