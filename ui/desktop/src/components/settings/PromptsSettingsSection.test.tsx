import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import PromptsSettingsSection from './PromptsSettingsSection';
import { IntlTestWrapper } from '../../i18n/test-utils';

const acp = vi.hoisted(() => ({
  acpListPrompts: vi.fn(),
  acpGetPrompt: vi.fn(),
  acpResetPrompt: vi.fn(),
  acpSavePrompt: vi.fn(),
}));
vi.mock('../../acp/prompts', () => acp);
vi.mock('react-toastify', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const renderSection = () =>
  render(
    <IntlTestWrapper>
      <PromptsSettingsSection />
    </IntlTestWrapper>
  );

describe('PromptsSettingsSection confirmations are in-app dialogs, never window.confirm', () => {
  let nativeConfirm: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    nativeConfirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    acp.acpListPrompts.mockResolvedValue([
      { name: 'system.md', description: 'The system prompt', isCustomized: true },
    ]);
    acp.acpGetPrompt.mockResolvedValue({
      name: 'system.md',
      content: 'custom text',
      defaultContent: 'default text',
      isCustomized: true,
    });
    acp.acpResetPrompt.mockResolvedValue(undefined);
    acp.acpSavePrompt.mockResolvedValue(undefined);
  });

  afterEach(() => {
    nativeConfirm.mockRestore();
    vi.clearAllMocks();
  });

  it('Reset All: cancel resets nothing, confirm resets every customized prompt', async () => {
    const user = userEvent.setup();
    renderSection();

    await user.click(await screen.findByRole('button', { name: /Reset All/ }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Reset all prompts?')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(acp.acpResetPrompt).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: /Reset All/ }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Reset All' }));
    await waitFor(() => expect(acp.acpResetPrompt).toHaveBeenCalledWith('system.md'));
    expect(nativeConfirm).not.toHaveBeenCalled();
  });

  it('Reset to Default on one prompt asks in-app first', async () => {
    const user = userEvent.setup();
    renderSection();
    await user.click(await screen.findByRole('button', { name: 'Edit' }));

    await user.click(await screen.findByRole('button', { name: /Reset to Default/ }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Reset this prompt?')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(acp.acpResetPrompt).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: /Reset to Default/ }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Reset to Default' }));
    await waitFor(() => expect(acp.acpResetPrompt).toHaveBeenCalledWith('system.md'));
    expect(nativeConfirm).not.toHaveBeenCalled();
  });

  it('Restore Default over unsaved edits and Back with unsaved edits both ask in-app', async () => {
    const user = userEvent.setup();
    renderSection();
    await user.click(await screen.findByRole('button', { name: 'Edit' }));

    const editor = await screen.findByDisplayValue('custom text');
    await user.clear(editor);
    await user.type(editor, 'edited');

    await user.click(screen.getByRole('button', { name: 'Restore Default' }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Replace with the default?')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByDisplayValue('edited')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Back to List' }));
    dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Discard unsaved changes?')).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(screen.getByDisplayValue('edited')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Restore Default' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Restore Default' }));
    expect(await screen.findByDisplayValue('default text')).toBeInTheDocument();

    await user.type(screen.getByDisplayValue('default text'), '!');
    await user.click(screen.getByRole('button', { name: 'Back to List' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Discard changes' }));
    expect(await screen.findByRole('button', { name: 'Edit' })).toBeInTheDocument();
    expect(nativeConfirm).not.toHaveBeenCalled();
  });
});
