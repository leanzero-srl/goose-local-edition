import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ModelSettingsPanel } from './ModelSettingsPanel';
import { IntlTestWrapper } from '../../../i18n/test-utils';

const updateModelSettings = vi.fn().mockResolvedValue(undefined);

vi.mock('../../../acp/local-inference', () => ({
  getModelSettings: vi.fn().mockResolvedValue({
    sampling: { type: 'Temperature', temperature: 0.8, topK: 40, topP: 0.95, minP: 0.05 },
    toolCalling: 'auto',
    chatTemplate: { type: 'embedded' },
    flashAttention: null,
  }),
  listBuiltinChatTemplates: vi.fn().mockResolvedValue(['chatml', 'llama3']),
  updateModelSettings: (...args: unknown[]) => updateModelSettings(...args),
}));

const renderPanel = async () => {
  const utils = render(<ModelSettingsPanel modelId="qwen" />, { wrapper: IntlTestWrapper });
  await screen.findByRole('combobox', { name: 'Tool calling' });
  return utils;
};

describe('ModelSettingsPanel select fields', () => {
  beforeEach(() => updateModelSettings.mockClear());

  it('renders every select field as the Studio listbox, never a native <select>', async () => {
    const { container } = await renderPanel();

    expect(container.querySelector('select')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'Sampling Strategy' })).toHaveTextContent(
      'Temperature'
    );
    expect(screen.getByRole('combobox', { name: 'Flash attention' })).toHaveTextContent('Auto');
    expect(screen.getByRole('combobox', { name: 'Tool calling' })).toHaveTextContent('Auto');
    expect(screen.getByRole('combobox', { name: 'Chat template' })).toHaveTextContent('Embedded');
  });

  it('picking an option saves the chosen value', async () => {
    const user = userEvent.setup();
    await renderPanel();

    await user.click(screen.getByRole('combobox', { name: 'Tool calling' }));
    await user.click(screen.getByRole('option', { name: 'Force native' }));

    expect(updateModelSettings).toHaveBeenCalledWith(
      'qwen',
      expect.objectContaining({ toolCalling: 'force_native' })
    );
    expect(screen.getByRole('combobox', { name: 'Tool calling' })).toHaveTextContent(
      'Force native'
    );
  });

  it('flash attention maps On to true', async () => {
    const user = userEvent.setup();
    await renderPanel();

    await user.click(screen.getByRole('combobox', { name: 'Flash attention' }));
    await user.click(screen.getByRole('option', { name: 'On' }));

    expect(updateModelSettings).toHaveBeenCalledWith(
      'qwen',
      expect.objectContaining({ flashAttention: true })
    );
  });
});
