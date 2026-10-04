import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { acpListProviderDetails } from '../../acp/providers';
import { CloudEntrant } from './CloudEntrant';

vi.mock('../../acp/providers', () => ({ acpListProviderDetails: vi.fn() }));
vi.mock('../openrouter/OpenRouterHostPicker', () => ({
  OpenRouterHostPicker: ({ model, disabled }: { model: string; disabled: boolean }) => (
    <div data-testid="host-picker" data-model={model} data-disabled={String(disabled)} />
  ),
}));
afterEach(cleanup);

const row = (name: string, display: string) => ({
  name,
  is_configured: true,
  provider_type: 'Builtin' as const,
  metadata: {
    name,
    display_name: display,
    description: '',
    default_model: '',
    model_doc_link: '',
    config_keys: [],
    known_models: [],
  },
});

it('mounts the OpenRouter host picker for the typed model, and only for OpenRouter', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([
    row('openrouter', 'OpenRouter'),
    row('anthropic', 'Claude'),
  ]);
  const { rerender } = render(
    <CloudEntrant
      provider="openrouter"
      model="qwen/qwen3.8-27b"
      disabled={false}
      onChange={vi.fn()}
    />
  );
  const picker = await screen.findByTestId('host-picker');
  expect(picker).toHaveAttribute('data-model', 'qwen/qwen3.8-27b');
  expect(picker).toHaveAttribute('data-disabled', 'false');

  rerender(
    <CloudEntrant provider="openrouter" model="qwen/qwen3.8-27b" disabled onChange={vi.fn()} />
  );
  expect(screen.getByTestId('host-picker')).toHaveAttribute('data-disabled', 'true');

  rerender(
    <CloudEntrant
      provider="anthropic"
      model="claude-sonnet-4-5"
      disabled={false}
      onChange={vi.fn()}
    />
  );
  await waitFor(() => expect(screen.queryByTestId('host-picker')).toBeNull());
});

it('typing a new model id hands the picker that id', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([row('openrouter', 'OpenRouter')]);
  const change = vi.fn();
  const { rerender } = render(
    <CloudEntrant
      provider="openrouter"
      model="qwen/qwen3.8-27b"
      disabled={false}
      onChange={change}
    />
  );
  await screen.findByTestId('host-picker');
  fireEvent.change(screen.getByRole('textbox', { name: 'Model ID' }), {
    target: { value: 'deepseek/deepseek-v4.1-flash' },
  });
  expect(change).toHaveBeenCalledWith('openrouter', 'deepseek/deepseek-v4.1-flash');
  // The form owns the value: once it re-renders with the typed id, the picker lists that model.
  rerender(
    <CloudEntrant
      provider="openrouter"
      model="deepseek/deepseek-v4.1-flash"
      disabled={false}
      onChange={change}
    />
  );
  expect(screen.getByTestId('host-picker')).toHaveAttribute(
    'data-model',
    'deepseek/deepseek-v4.1-flash'
  );
});
