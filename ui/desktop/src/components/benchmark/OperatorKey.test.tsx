import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { acpReadConfig, acpRemoveConfig, acpUpsertConfig } from '../../acp/config';
import { OperatorKey } from './OperatorKey';

vi.mock('../../acp/config', () => ({
  acpReadConfig: vi.fn(),
  acpUpsertConfig: vi.fn(async () => {}),
  acpRemoveConfig: vi.fn(async () => {}),
}));

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

const input = () => screen.getByLabelText('LeanZero operator key') as HTMLInputElement;

it('saves the key to goose config (never shown back) and removes it when cleared', async () => {
  vi.mocked(acpReadConfig).mockResolvedValue(null);
  render(<OperatorKey disabled={false} />);
  await waitFor(() => expect(input()).toBeEnabled());
  expect(input().type).toBe('password');
  fireEvent.change(input(), { target: { value: ' k3y ' } });
  expect(acpUpsertConfig).toHaveBeenLastCalledWith('LEANZERO_BENCH_OPERATOR_KEY', 'k3y');
  await waitFor(() => expect(screen.getByText(/Saved\./)).toBeInTheDocument());
  fireEvent.change(input(), { target: { value: '' } });
  await waitFor(() => expect(acpRemoveConfig).toHaveBeenCalledWith('LEANZERO_BENCH_OPERATOR_KEY', false));
});

it('a saved key is not displayed, only that one is saved', async () => {
  vi.mocked(acpReadConfig).mockResolvedValue('secret');
  render(<OperatorKey disabled={false} />);
  await waitFor(() => expect(input()).toBeEnabled());
  expect(input().value).toBe('');
  expect(input().placeholder).toMatch(/saved/);
});
