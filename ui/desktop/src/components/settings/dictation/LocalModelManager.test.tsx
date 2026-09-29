import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import { LocalModelManager } from './LocalModelManager';

const dictation = vi.hoisted(() => ({
  listLocalDictationModels: vi.fn(),
  deleteLocalDictationModel: vi.fn(),
}));
const config = vi.hoisted(() => ({
  read: vi.fn(),
  upsert: vi.fn(),
}));

vi.mock('../../../acp/dictation', () => ({
  listLocalDictationModels: dictation.listLocalDictationModels,
  deleteLocalDictationModel: dictation.deleteLocalDictationModel,
  cancelLocalDictationModelDownload: vi.fn(),
  downloadLocalDictationModel: vi.fn(),
  getLocalDictationModelDownloadProgress: vi.fn(),
}));
vi.mock('../../ConfigContext', () => ({ useConfig: () => config }));

describe('deleting a local dictation model asks in the app, never through window.confirm', () => {
  let nativeConfirm: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.clearAllMocks();
    nativeConfirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    dictation.listLocalDictationModels.mockResolvedValue([
      {
        id: 'base',
        sizeMb: 142,
        description: 'Balanced model',
        downloaded: true,
        recommended: true,
      },
    ]);
    dictation.deleteLocalDictationModel.mockResolvedValue(undefined);
    config.read.mockResolvedValue('base');
    config.upsert.mockResolvedValue(undefined);
  });

  afterEach(() => {
    nativeConfirm.mockRestore();
  });

  it('cancel keeps the model; Delete removes it and clears the selection', async () => {
    const user = userEvent.setup();
    render(<LocalModelManager />, { wrapper: IntlTestWrapper });

    await user.click(await screen.findByRole('button', { name: 'Delete' }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Delete Base?')).toBeInTheDocument();
    expect(
      within(dialog).getByText('Delete this model? You can re-download it later.')
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(dictation.deleteLocalDictationModel).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Delete' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Delete' }));
    await waitFor(() => expect(dictation.deleteLocalDictationModel).toHaveBeenCalledWith('base'));
    await waitFor(() =>
      expect(config.upsert).toHaveBeenCalledWith('LOCAL_WHISPER_MODEL', '', false)
    );
    expect(nativeConfirm).not.toHaveBeenCalled();
  });
});
