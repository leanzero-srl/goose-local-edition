import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  MIDDLE_OUT_KEY,
  OpenRouterMiddleOutSwitch,
  middleOutFrom,
} from './OpenRouterMiddleOutSwitch';

const config = vi.hoisted(() => ({
  stored: undefined as unknown,
  read: vi.fn(),
  upsert: vi.fn(),
}));

vi.mock('../../acp/config', () => ({
  acpReadConfig: config.read,
  acpUpsertConfig: config.upsert,
}));

const toggle = () => screen.getByTestId('openrouter-middle-out-switch');

beforeEach(() => {
  config.stored = null;
  config.read.mockReset().mockImplementation(async () => config.stored);
  config.upsert.mockReset().mockImplementation(async (_key: string, value: unknown) => {
    config.stored = value;
  });
});

describe('OpenRouter middle-out switch', () => {
  it('is off when goose config holds nothing', async () => {
    render(<OpenRouterMiddleOutSwitch />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    expect(toggle()).toHaveAttribute('aria-checked', 'false');
    expect(config.read).toHaveBeenCalledWith(MIDDLE_OUT_KEY, false);
  });

  it('turning it on saves true under the key the provider reads', async () => {
    render(<OpenRouterMiddleOutSwitch />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    await waitFor(() => expect(config.upsert).toHaveBeenCalledWith(MIDDLE_OUT_KEY, true, false));
    expect(toggle()).toHaveAttribute('aria-checked', 'true');
  });

  it('a failed save puts the switch back and says why', async () => {
    config.upsert.mockRejectedValueOnce(new Error('disk full'));
    render(<OpenRouterMiddleOutSwitch />);
    await waitFor(() => expect(toggle()).not.toBeDisabled());
    fireEvent.click(toggle());
    expect(await screen.findByRole('alert')).toHaveTextContent('disk full');
    expect(toggle()).toHaveAttribute('aria-checked', 'false');
  });

  it('reads a boolean the way goose does', () => {
    expect(middleOutFrom(true)).toBe(true);
    expect(middleOutFrom(' TRUE ')).toBe(true);
    expect(middleOutFrom(false)).toBe(false);
    expect(middleOutFrom(null)).toBe(false);
    expect(middleOutFrom('yes')).toBe(false);
  });
});
