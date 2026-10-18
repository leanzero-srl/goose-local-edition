import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { OperatorKey } from './OperatorKey';

afterEach(cleanup);

const input = () => screen.getByLabelText('LeanZero operator key') as HTMLInputElement;
const mountWith = (status: { saved: boolean; available: boolean }) => {
  const set = vi.fn(async (key: string) => ({ ok: true, saved: key.trim().length > 0 }));
  Object.assign(window.electron, { benchmarkOperatorKeyStatus: vi.fn(async () => status), benchmarkOperatorKeySet: set });
  render(<OperatorKey disabled={false} />);
  return set;
};

it('is write-only: the key goes to main on Enter, the field clears, and nothing reads it back', async () => {
  const set = mountWith({ saved: false, available: true });
  await waitFor(() => expect(input()).toBeEnabled());
  expect(input().type).toBe('password');
  fireEvent.change(input(), { target: { value: ' k3y ' } });
  fireEvent.keyDown(input(), { key: 'Enter' });
  await waitFor(() => expect(set).toHaveBeenCalledWith(' k3y '));
  await waitFor(() => expect(input().value).toBe(''));
  expect(screen.getByText(/Saved in this Mac’s keychain/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Remove key' }));
  await waitFor(() => expect(set).toHaveBeenLastCalledWith(''));
});

it('a saved key is never displayed, and without keychain encryption the field is locked with the reason', async () => {
  mountWith({ saved: true, available: true });
  await waitFor(() => expect(input()).toBeEnabled());
  expect(input().value).toBe('');
  expect(input().placeholder).toMatch(/saved/);
  cleanup();
  mountWith({ saved: false, available: false });
  await waitFor(() => expect(screen.getByText(/keychain encryption is not available/)).toBeInTheDocument());
  expect(input()).toBeDisabled();
});
