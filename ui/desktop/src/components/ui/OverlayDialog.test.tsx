import { useState } from 'react';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { OverlayDialog, OverlayDialogTitle } from './OverlayDialog';

/**
 * Q-21: Escape closes the panel, focus is trapped inside it, and — measured on the packaged app —
 * focus must come BACK to the button that opened it: Radix returns focus to a Dialog.Trigger only,
 * and these panels open from ordinary buttons, so focus fell to <body>.
 */
function Harness({ unmountOnClose }: { unmountOnClose: boolean }) {
  const [open, setOpen] = useState(false);
  const panel = (
    <OverlayDialog open={open} onClose={() => setOpen(false)}>
      <OverlayDialogTitle>Panel</OverlayDialogTitle>
      <button>inside</button>
    </OverlayDialog>
  );
  return (
    <>
      <button onClick={() => setOpen(true)}>opener</button>
      {unmountOnClose ? open && panel : panel}
    </>
  );
}

describe('OverlayDialog gives focus back to its opener (Q-21)', () => {
  it.each([
    ['kept mounted', false],
    ['unmounted on close (how the composer renders Report a problem)', true],
  ])('%s', async (_name, unmountOnClose) => {
    render(<Harness unmountOnClose={unmountOnClose} />);
    const opener = screen.getByRole('button', { name: 'opener' });
    opener.focus();
    fireEvent.click(opener);
    const dialog = await screen.findByRole('dialog', { name: 'Panel' });
    expect(dialog.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(dialog, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(opener));
  });
});
