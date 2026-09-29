import { describe, it, expect } from 'vitest';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { useConfirmDialog, type ConfirmOptions } from './useConfirmDialog';

const ask = (title: string): ConfirmOptions => ({
  title,
  message: `${title} message`,
  confirmLabel: 'Go',
  cancelLabel: 'Stop',
});

let confirmRef: ((options: ConfirmOptions) => Promise<boolean>) | null = null;

function Host() {
  const { confirm, dialog } = useConfirmDialog();
  confirmRef = confirm;
  return dialog;
}

const renderHost = () =>
  render(
    <IntlTestWrapper>
      <Host />
    </IntlTestWrapper>
  );

describe('useConfirmDialog', () => {
  it('a second request waits behind the first and each resolves with its own answer', async () => {
    const user = userEvent.setup();
    renderHost();

    let first!: Promise<boolean>;
    let second!: Promise<boolean>;
    act(() => {
      first = confirmRef!(ask('First'));
      second = confirmRef!(ask('Second'));
    });

    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('First')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Go' }));
    await expect(first).resolves.toBe(true);

    dialog = await screen.findByRole('dialog');
    await waitFor(() => expect(within(dialog).getByText('Second')).toBeInTheDocument());
    await user.click(within(dialog).getByRole('button', { name: 'Stop' }));
    await expect(second).resolves.toBe(false);
  });

  it('Escape answers cancel', async () => {
    const user = userEvent.setup();
    renderHost();
    let answer!: Promise<boolean>;
    act(() => {
      answer = confirmRef!(ask('Leave'));
    });
    await screen.findByRole('dialog');
    await user.keyboard('{Escape}');
    await expect(answer).resolves.toBe(false);
  });

  it('a request still open when its owner unmounts answers cancel instead of hanging', async () => {
    const { unmount } = renderHost();
    let answer!: Promise<boolean>;
    act(() => {
      answer = confirmRef!(ask('Orphan'));
    });
    await screen.findByRole('dialog');
    unmount();
    await expect(answer).resolves.toBe(false);
  });
});
