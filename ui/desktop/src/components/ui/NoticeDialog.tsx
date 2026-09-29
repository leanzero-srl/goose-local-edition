import React from 'react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from './dialog';
import { Button } from './button';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  ok: {
    id: 'noticeDialog.ok',
    defaultMessage: 'OK',
  },
});

/**
 * An informational in-app dialog with a single acknowledge button — the replacement for a native
 * message box that only ever offered "OK".
 */
export function NoticeDialog({
  isOpen,
  title,
  message,
  detail,
  onClose,
}: {
  isOpen: boolean;
  title: string;
  message: string;
  detail?: React.ReactNode;
  onClose: () => void;
}) {
  const intl = useIntl();
  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-[425px] max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{message}</DialogDescription>
        </DialogHeader>
        {detail && (
          <div className="overflow-y-auto min-h-0 text-sm text-text-secondary break-all">
            {detail}
          </div>
        )}
        <DialogFooter className="pt-2 shrink-0">
          <Button onClick={onClose}>{intl.formatMessage(i18n.ok)}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
