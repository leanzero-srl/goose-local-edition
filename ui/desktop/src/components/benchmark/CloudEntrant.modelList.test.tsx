import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { acpListProviderDetails, acpListProviderLiveModels } from '../../acp/providers';
import { CloudEntrant } from './CloudEntrant';
import { BENCHMARK_THINKING_LINE } from '../../benchModelFields';

vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: vi.fn(),
  acpListProviderLiveModels: vi.fn(),
  acpReadProviderConfig: vi.fn(),
  acpRecheckProviderConnections: vi.fn(),
}));
vi.mock('../openrouter/OpenRouterHostPicker', () => ({ OpenRouterHostPicker: () => null }));
afterEach(cleanup);

const row = (name: string, display: string, catalog: string[] = [], defaultModel = '') => ({
  name,
  is_configured: true,
  provider_type: 'Builtin' as const,
  default_model: defaultModel,
  metadata: {
    name,
    display_name: display,
    description: '',
    default_model: '',
    model_doc_link: '',
    config_keys: [],
    known_models: catalog.map((id) => ({ name: id, context_limit: 0 })),
  },
});

function Form({ initialProvider = 'anthropic' }: { initialProvider?: string }) {
  const [provider, setProvider] = useState(initialProvider);
  const [model, setModel] = useState('');
  return (
    <>
      <CloudEntrant
        provider={provider}
        model={model}
        disabled={false}
        onChange={(p, m) => {
          setProvider(p);
          setModel(m);
        }}
      />
      <output data-testid="chosen">{`${provider}|${model}`}</output>
    </>
  );
}

beforeEach(() => {
  vi.mocked(acpListProviderDetails).mockReset();
  vi.mocked(acpListProviderLiveModels).mockReset();
});

it('lists the models the chosen provider serves, default first, in a searchable dropdown', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([
    row('anthropic', 'Claude', [], 'claude-sonnet-5'),
  ]);
  vi.mocked(acpListProviderLiveModels).mockResolvedValue([
    'claude-haiku-5',
    'claude-opus-5-5',
    'claude-sonnet-5',
  ]);
  const { container } = render(<Form />);
  expect(await screen.findByTestId('bench-models-read')).toHaveTextContent(
    '3 models Claude serves with your key'
  );
  expect(acpListProviderLiveModels).toHaveBeenCalledWith('anthropic');
  expect(container.querySelector('select, datalist')).toBeNull();
  const field = screen.getByRole('combobox', { name: 'Model ID' });
  fireEvent.focus(field);
  const options = within(screen.getByRole('listbox')).getAllByRole('option');
  expect(options.map((o) => o.textContent)).toEqual([
    'claude-sonnet-5default',
    'claude-haiku-5',
    'claude-opus-5-5',
  ]);
  fireEvent.change(field, { target: { value: 'opus' } });
  expect(within(screen.getByRole('listbox')).getAllByRole('option')).toHaveLength(1);
  fireEvent.click(screen.getByRole('option', { name: 'claude-opus-5-5' }));
  expect(screen.getByTestId('chosen')).toHaveTextContent('anthropic|claude-opus-5-5');
  expect(screen.getByTestId('bench-thinking-line')).toHaveTextContent(BENCHMARK_THINKING_LINE);
});

it('an ID the list does not carry is still the value, exactly as typed', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([row('anthropic', 'Claude')]);
  vi.mocked(acpListProviderLiveModels).mockResolvedValue(['claude-sonnet-5']);
  render(<Form />);
  await screen.findByTestId('bench-models-read');
  const field = screen.getByRole('combobox', { name: 'Model ID' });
  fireEvent.focus(field);
  fireEvent.change(field, { target: { value: 'claude-next-preview' } });
  expect(screen.getByText('Not in the list — the ID is used exactly as typed.')).toBeVisible();
  expect(screen.getByTestId('chosen')).toHaveTextContent('anthropic|claude-next-preview');
  fireEvent.mouseDown(document.body);
  expect(field).toHaveValue('claude-next-preview');
});

it('a value set from outside (the CDP driver) lands without opening a list over the Run button', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([row('anthropic', 'Claude')]);
  vi.mocked(acpListProviderLiveModels).mockResolvedValue(['claude-sonnet-5']);
  render(<Form />);
  await screen.findByTestId('bench-models-read');
  const field = document.querySelector<HTMLInputElement>('input[aria-label="Model ID"]');
  expect(field).not.toBeNull();
  fireEvent.change(field!, { target: { value: 'claude-sonnet-5' } });
  expect(screen.getByTestId('chosen')).toHaveTextContent('anthropic|claude-sonnet-5');
  expect(field!.value).toBe('claude-sonnet-5');
  expect(screen.queryByRole('listbox')).toBeNull();
});

it('a failed listing is stated, the catalog offered under it, and typing still works', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([
    row('anthropic', 'Claude', ['claude-sonnet-5']),
  ]);
  vi.mocked(acpListProviderLiveModels).mockRejectedValue(new Error('401 invalid x-api-key'));
  render(<Form />);
  const failed = await screen.findByTestId('bench-models-failed');
  expect(failed).toHaveAttribute('role', 'alert');
  expect(failed).toHaveTextContent('Claude did not list its models: 401 invalid x-api-key.');
  expect(failed).toHaveTextContent('These are the models goose knows for it');
  const field = screen.getByRole('combobox', { name: 'Model ID' });
  expect(field).toBeEnabled();
  fireEvent.focus(field);
  expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual(['claude-sonnet-5']);
  fireEvent.change(field, { target: { value: 'claude-x' } });
  expect(screen.getByTestId('chosen')).toHaveTextContent('anthropic|claude-x');
});

it('says it is asking while the listing is in flight, never an empty "no models"', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([row('anthropic', 'Claude')]);
  vi.mocked(acpListProviderLiveModels).mockReturnValue(new Promise(() => undefined));
  render(<Form />);
  expect(await screen.findByTestId('bench-models-reading')).toHaveTextContent(
    'Asking Claude for its models'
  );
  expect(screen.queryByTestId('bench-models-read')).toBeNull();
});

it('a provider with no listing endpoint says so instead of showing an empty list', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([row('anthropic', 'Claude')]);
  vi.mocked(acpListProviderLiveModels).mockResolvedValue([]);
  render(<Form />);
  expect(await screen.findByTestId('bench-models-unlisted')).toHaveTextContent(
    'Claude has no model listing and goose knows no models for it.'
  );
});

it('choosing another provider asks that provider for its models', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([
    row('anthropic', 'Claude'),
    row('openrouter', 'OpenRouter'),
  ]);
  vi.mocked(acpListProviderLiveModels).mockImplementation(async (id: string) =>
    id === 'openrouter' ? ['qwen/qwen3.8-27b'] : ['claude-sonnet-5']
  );
  render(<Form />);
  await screen.findByText(/1 models Claude serves/);
  fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
  fireEvent.click(await screen.findByRole('menuitem', { name: 'OpenRouter' }));
  await waitFor(() =>
    expect(screen.getByTestId('bench-models-read')).toHaveTextContent('OpenRouter serves')
  );
  expect(acpListProviderLiveModels).toHaveBeenLastCalledWith('openrouter');
});
