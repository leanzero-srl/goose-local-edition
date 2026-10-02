import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { acpReadConfig, acpRemoveConfig, acpUpsertConfig } from '../../acp/config';
import { WalletLimit } from './WalletLimit';

vi.mock('../../acp/config', () => ({
  acpReadConfig: vi.fn(),
  acpUpsertConfig: vi.fn(async () => {}),
  acpRemoveConfig: vi.fn(async () => {}),
}));

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

const input = () => screen.getByRole('textbox', { name: 'Stop a run at (US dollars, OpenRouter)' });

it('shows the saved limit and saves every valid edit to BENCH_MAX_USD', async () => {
  vi.mocked(acpReadConfig).mockResolvedValue(5);
  render(<WalletLimit disabled={false} />);
  await waitFor(() => expect(input()).toHaveValue('5'));
  expect(screen.getByText(/Once OpenRouter has billed \$5\.00, the run stops/)).toBeInTheDocument();
  fireEvent.change(input(), { target: { value: '2.50' } });
  expect(acpUpsertConfig).toHaveBeenLastCalledWith('BENCH_MAX_USD', 2.5);
  fireEvent.change(input(), { target: { value: '' } });
  await waitFor(() => expect(acpRemoveConfig).toHaveBeenCalledWith('BENCH_MAX_USD', false));
});

it('never saves an invalid amount, and says why', async () => {
  vi.mocked(acpReadConfig).mockResolvedValue(null);
  render(<WalletLimit disabled={false} />);
  await waitFor(() => expect(input()).toBeEnabled());
  fireEvent.change(input(), { target: { value: '-3' } });
  expect(screen.getByRole('alert')).toHaveTextContent('Enter a dollar amount above zero');
  expect(acpUpsertConfig).not.toHaveBeenCalled();
  fireEvent.change(input(), { target: { value: '' } });
  expect(acpRemoveConfig).not.toHaveBeenCalled();
});

it('a failed read is stated and the field stays locked', async () => {
  vi.mocked(acpReadConfig).mockRejectedValue(new Error('backend disconnected'));
  render(<WalletLimit disabled={false} />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Your saved limit could not be read: backend disconnected'
  );
  expect(input()).toBeDisabled();
});
