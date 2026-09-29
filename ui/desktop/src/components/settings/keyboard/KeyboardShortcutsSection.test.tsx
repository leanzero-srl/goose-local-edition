import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import KeyboardShortcutsSection from './KeyboardShortcutsSection';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import { defaultKeyboardShortcuts } from '../../../utils/settings';

vi.mock('../../../utils/analytics', () => ({ trackSettingToggled: vi.fn() }));

// The recorder's key capture is not what is under test: its Save hands the component a shortcut
// that collides with New Window, which is the path that used to open a native message box.
vi.mock('./ShortcutRecorder', () => ({
  ShortcutRecorder: ({ onSave }: { onSave: (shortcut: string) => void }) => (
    <button type="button" onClick={() => onSave(defaultKeyboardShortcuts.newChatWindow)}>
      save-colliding-shortcut
    </button>
  ),
}));

const electron = window.electron as unknown as {
  getSetting: ReturnType<typeof vi.fn>;
  setSetting: ReturnType<typeof vi.fn>;
  showMessageBox: ReturnType<typeof vi.fn>;
};

const renderSection = () =>
  render(
    <IntlTestWrapper>
      <KeyboardShortcutsSection />
    </IntlTestWrapper>
  );

describe('KeyboardShortcutsSection confirmations are in-app dialogs, never native boxes', () => {
  let stored: Record<string, string | null>;

  beforeEach(() => {
    stored = { ...defaultKeyboardShortcuts };
    electron.showMessageBox = vi.fn(() => Promise.resolve({ response: 0 }));
    electron.getSetting = vi.fn(async () => stored);
    electron.setSetting = vi.fn(async () => {});
  });

  it('reset: opens the app dialog; cancel writes nothing, confirm restores the defaults', async () => {
    const user = userEvent.setup();
    stored = { ...defaultKeyboardShortcuts, find: null };
    renderSection();

    await user.click(await screen.findByRole('button', { name: 'Reset All Shortcuts' }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Reset Keyboard Shortcuts')).toBeInTheDocument();
    expect(
      within(dialog).getByText('Reset all keyboard shortcuts to their default values?')
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(electron.setSetting).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Reset All Shortcuts' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Reset to Defaults' }));

    await waitFor(() =>
      expect(electron.setSetting).toHaveBeenCalledWith('keyboardShortcuts', {
        ...defaultKeyboardShortcuts,
      })
    );
    expect(electron.showMessageBox).not.toHaveBeenCalled();
  });

  it('enabling a shortcut whose default is taken asks in-app before reassigning it', async () => {
    const user = userEvent.setup();
    // Focus Window is off, and its default combo now belongs to New Window.
    stored = {
      ...defaultKeyboardShortcuts,
      focusWindow: null,
      newChatWindow: defaultKeyboardShortcuts.focusWindow,
    };
    renderSection();

    const switches = await screen.findAllByRole('switch');
    // Focus Window is the only global shortcut, so its card and switch render first.
    await user.click(switches[0]);

    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Shortcut Conflict')).toBeInTheDocument();
    expect(within(dialog).getByText(/already assigned to "New Window"/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(electron.setSetting).not.toHaveBeenCalled();

    await user.click(switches[0]);
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Reassign Shortcut' }));

    await waitFor(() =>
      expect(electron.setSetting).toHaveBeenCalledWith('keyboardShortcuts', {
        ...stored,
        focusWindow: defaultKeyboardShortcuts.focusWindow,
        newChatWindow: null,
      })
    );
    expect(electron.showMessageBox).not.toHaveBeenCalled();
  });

  it('saving a recorded shortcut that collides asks in-app before taking it over', async () => {
    const user = userEvent.setup();
    renderSection();

    // Settings' Change button (the fourth row: Focus Window, New Window, Open Directory, Settings).
    const changeButtons = await screen.findAllByRole('button', { name: 'Change' });
    await user.click(changeButtons[3]);
    await user.click(screen.getByRole('button', { name: 'save-colliding-shortcut' }));

    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Shortcut Conflict')).toBeInTheDocument();
    expect(
      within(dialog).getByText(/Saving this will remove the shortcut from "New Window"/)
    ).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(electron.setSetting).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'save-colliding-shortcut' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Reassign Shortcut' }));

    await waitFor(() =>
      expect(electron.setSetting).toHaveBeenCalledWith('keyboardShortcuts', {
        ...defaultKeyboardShortcuts,
        newChatWindow: null,
        settings: defaultKeyboardShortcuts.newChatWindow,
      })
    );
    expect(electron.showMessageBox).not.toHaveBeenCalled();
  });
});
