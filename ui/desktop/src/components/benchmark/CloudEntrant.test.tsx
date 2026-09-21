import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { acpListProviderDetails } from '../../acp/providers';
import { CloudEntrant } from './CloudEntrant';
vi.mock('../../acp/providers', () => ({ acpListProviderDetails: vi.fn() }));
afterEach(cleanup);
const row = (name: string, configured = true) => ({
  name,
  is_configured: configured,
  provider_type: 'Builtin' as const,
  metadata: {
    name,
    display_name: name,
    description: '',
    default_model: '',
    model_doc_link: '',
    config_keys: [],
    known_models: [],
  },
});
it('lists configured supported providers and excludes removed providers', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue(
    [
      'google',
      'anthropic',
      'openai',
      'aws_bedrock',
      'custom_deepseek',
      'custom_team',
      'lmstudio',
      'ollama',
      'omlx',
      'local',
    ]
      .map((name) => row(name))
      .concat(row('unconfigured', false))
  );
  const change = vi.fn();
  render(<CloudEntrant provider="" model="" disabled={false} onChange={change} />);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Model provider' })).toBeEnabled());
  fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
  expect(await screen.findAllByRole('menuitem')).toHaveLength(5);
  for (const name of ['unconfigured', 'lmstudio', 'ollama', 'omlx', 'local', 'custom_team']) {
    expect(screen.queryByRole('menuitem', { name })).toBeNull();
  }
  fireEvent.click(screen.getByRole('menuitem', { name: 'custom_deepseek' }));
  expect(change).toHaveBeenCalledWith('custom_deepseek', '');
});
it('shows fresh-profile setup and an explicit failed-read state', async () => {
  vi.mocked(acpListProviderDetails).mockResolvedValue([]);
  const { unmount } = render(
    <CloudEntrant provider="" model="" disabled={false} onChange={vi.fn()} />
  );
  expect(await screen.findByText(/No configured providers/)).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Configure providers' })).toHaveAttribute(
    'href',
    '#/leanzero-swarm'
  );
  unmount();
  vi.mocked(acpListProviderDetails).mockRejectedValue(new Error('backend disconnected'));
  render(<CloudEntrant provider="" model="" disabled={false} onChange={vi.fn()} />);
  expect(await screen.findByRole('alert')).toHaveTextContent('backend disconnected');
  expect(screen.getByRole('textbox')).toBeDisabled();
});
