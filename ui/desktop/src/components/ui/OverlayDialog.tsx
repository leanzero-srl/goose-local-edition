import * as React from 'react';
import * as DialogPrimitive from '@radix-ui/react-dialog';
import { Dialog, DialogOverlay, DialogPortal } from './dialog';
import { cn } from '../../utils';

/**
 * The app's dialog primitive (Radix, ui/dialog.tsx) for a panel that draws its OWN chrome: Escape
 * and a click on the backdrop close it, focus is trapped inside and returns to the opener, and the
 * panel is announced as a modal dialog. Q-21: the Report a problem and Set up overlays were
 * hand-rolled fixed divs — Escape did nothing and focus walked out behind them.
 *
 * `panelClassName` is the panel's own look (size, surface); it is centred over a `bg-black/50`
 * backdrop. Put the heading in `OverlayDialogTitle` so the dialog is named by it.
 */
export function OverlayDialog({
  open,
  onClose,
  panelClassName,
  layerClassName = 'z-50',
  children,
  ...rest
}: {
  open: boolean;
  onClose: () => void;
  panelClassName?: string;
  /** The stacking layer of backdrop and panel (a wizard opened from another sits above it). */
  layerClassName?: string;
  children: React.ReactNode;
} & Omit<React.ComponentProps<typeof DialogPrimitive.Content>, 'children' | 'className'>) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogPortal>
        <DialogOverlay className={layerClassName} />
        <DialogPrimitive.Content
          aria-describedby={undefined}
          {...rest}
          className={cn(
            'fixed left-1/2 top-1/2 max-w-[calc(100%-2rem)] -translate-x-1/2 -translate-y-1/2 focus:outline-none',
            layerClassName,
            panelClassName
          )}
        >
          {children}
        </DialogPrimitive.Content>
      </DialogPortal>
    </Dialog>
  );
}

export const OverlayDialogTitle = DialogPrimitive.Title;
