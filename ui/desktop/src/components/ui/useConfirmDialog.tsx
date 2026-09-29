import React, { useCallback, useEffect, useRef, useState } from 'react';
import { ConfirmationModal } from './ConfirmationModal';

export interface ConfirmOptions {
  title: string;
  message: string;
  detail?: React.ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  confirmVariant?: React.ComponentProps<typeof ConfirmationModal>['confirmVariant'];
}

interface PendingConfirm extends ConfirmOptions {
  resolve: (confirmed: boolean) => void;
}

/**
 * The in-app replacement for `window.confirm` / a yes-no native message box: `confirm()` opens the
 * app's ConfirmationModal and resolves true on confirm, false on cancel (button, Escape, overlay).
 * Requests made while one is open queue behind it, and anything still open when the owner unmounts
 * resolves false, so an awaiting caller always takes its cancel path.
 */
export function useConfirmDialog() {
  const queueRef = useRef<PendingConfirm[]>([]);
  const [isOpen, setIsOpen] = useState(false);
  // The last request shown stays rendered after it settles so the closing animation keeps its text.
  const [options, setOptions] = useState<ConfirmOptions | null>(null);

  const confirm = useCallback(
    (request: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        queueRef.current.push({ ...request, resolve });
        if (queueRef.current.length === 1) {
          setOptions(request);
          setIsOpen(true);
        }
      }),
    []
  );

  const settle = useCallback((confirmed: boolean) => {
    const head = queueRef.current.shift();
    if (!head) return;
    head.resolve(confirmed);
    const next = queueRef.current[0];
    if (next) {
      setOptions(next);
    } else {
      setIsOpen(false);
    }
  }, []);

  useEffect(() => {
    const queue = queueRef.current;
    return () => {
      queue.splice(0).forEach((pending) => pending.resolve(false));
    };
  }, []);

  const dialog = (
    <ConfirmationModal
      isOpen={isOpen}
      title={options?.title ?? ''}
      message={options?.message ?? ''}
      detail={options?.detail}
      confirmLabel={options?.confirmLabel}
      cancelLabel={options?.cancelLabel}
      confirmVariant={options?.confirmVariant}
      onConfirm={() => settle(true)}
      onCancel={() => settle(false)}
    />
  );

  return { confirm, isOpen, options, settle, dialog };
}
