import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { GABEE, gb } from './memfree.fixtures';
import type { MemoryHolders } from './memfree';

const mockHolders = vi.fn();
const mockFree = vi.fn();
vi.mock('../../acp/memfree', () => ({
  memoryHolders: (...a: unknown[]) => mockHolders(...a),
  memoryFree: (...a: unknown[]) => mockFree(...a),
}));

import { FreeMemoryDialog } from './FreeMemoryDialog';

const show = (props: Partial<Parameters<typeof FreeMemoryDialog>[0]> = {}) =>
  render(
    <IntlTestWrapper>
      <FreeMemoryDialog macName="Gabee" fitModelId="qwen-30b" onClose={() => {}} {...props} />
    </IntlTestWrapper>
  );

const WITH_OWN: MemoryHolders = {
  ...GABEE,
  gooseOwnedBytes: gb(12),
  shortBytes: gb(10),
  verdict: { kind: 'fitsByGooseOwned', remainingBytes: 0, appsToQuit: [] },
  groups: [
    ...GABEE.groups,
    {
      id: 'engine-8080',
      label: 'goose MLX engine (idle)',
      category: 'gooseEngine',
      bytes: gb(12),
      pids: [{ pid: 300, startedAt: 2 }],
      quittable: false,
      reclaimableByGoose: true,
    },
  ],
};

beforeEach(() => {
  mockHolders.mockReset();
  mockFree.mockReset();
});
afterEach(cleanup);

describe('FreeMemoryDialog', () => {
  it('tells Gabee the honest cannotFit verdict, with both alternatives, in bold GB', async () => {
    mockHolders.mockResolvedValue(GABEE);
    show();
    const verdict = await screen.findByTestId('free-memory-verdict');
    expect(verdict.dataset.verdict).toBe('cannotFit');
    expect(verdict.textContent).toContain('15.5 GB');
    expect(verdict.textContent).toContain('5.7 GB short');
    expect(within(verdict).getAllByText(/GB$/, { selector: 'strong' }).length).toBeGreaterThan(0);
    const alts = screen.getByTestId('free-memory-alternatives');
    expect(alts.textContent).toContain('mlx-community/Qwen3.8-27B-4bit');
    expect(alts.textContent).toContain('Mac Studio');
    expect(mockHolders).toHaveBeenCalledWith({ nodeId: undefined, fitModelId: 'qwen-30b' });
    // goose holds nothing here: the one-click button says so and is disabled.
    expect(screen.getByTestId('free-memory-own')).toBeDisabled();
    // Categories: apps and system, each with a solid chip and its total.
    expect(screen.getByTestId('free-memory-cat-userApp').textContent).toContain('15.5 GB');
    expect(screen.getByTestId('free-memory-cat-system')).toBeInTheDocument();
    // system holders never get a Quit.
    expect(screen.queryByTestId('free-memory-quit-WindowServer')).toBeNull();
  });

  it('frees goose’s own memory in one click and shows before → after and the new verdict', async () => {
    mockHolders.mockResolvedValueOnce(WITH_OWN).mockResolvedValueOnce({
      ...WITH_OWN,
      gooseOwnedBytes: 0,
      shortBytes: 0,
      budgetBytes: gb(21.4),
      groups: GABEE.groups,
    });
    mockFree.mockResolvedValue({
      items: [
        {
          id: 'engine-8080',
          label: 'goose MLX engine',
          ok: true,
          freedBytes: gb(12),
          message: 'stopped',
        },
      ],
      before: { budgetBytes: gb(9.4) },
      after: { budgetBytes: gb(21.4) },
      fit: { status: 'fits', shortBytes: null },
    });
    const user = userEvent.setup();
    show({ target: 'n-gabee' });
    const own = await screen.findByTestId('free-memory-own');
    expect(own.textContent).toContain('12.0 GB');
    await user.click(own);
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith({
        nodeId: 'n-gabee',
        gooseOwned: true,
        quitApps: [],
        fitModelId: 'qwen-30b',
      })
    );
    const after = await screen.findByTestId('free-memory-after');
    expect(after.textContent).toContain('9.4 GB');
    expect(after.textContent).toContain('21.4 GB');
    expect(after.textContent).toContain('The model fits now.');
    expect(mockHolders).toHaveBeenCalledTimes(2);
    await waitFor(() => expect(screen.queryByTestId('free-memory-verdict')).toBeNull());
  });

  it('quits an app only after the house confirm dialog', async () => {
    mockHolders.mockResolvedValue(GABEE);
    mockFree.mockResolvedValue({
      items: [
        {
          id: 'com.google.Chrome',
          label: 'Google Chrome',
          ok: true,
          freedBytes: gb(7.2),
          message: 'quit',
        },
      ],
      before: { budgetBytes: gb(9.4) },
      after: { budgetBytes: gb(16.6) },
      fit: { status: 'short', shortBytes: gb(14) },
    });
    const user = userEvent.setup();
    show();
    await user.click(await screen.findByTestId('free-memory-quit-com.google.Chrome'));
    await user.click(await screen.findByRole('button', { name: 'Keep it open' }));
    expect(mockFree).not.toHaveBeenCalled();
    await user.click(screen.getByTestId('free-memory-quit-com.google.Chrome'));
    await user.click(await screen.findByRole('button', { name: 'Quit Google Chrome' }));
    await waitFor(() =>
      expect(mockFree).toHaveBeenCalledWith({
        nodeId: undefined,
        gooseOwned: false,
        quitApps: ['com.google.Chrome'],
        fitModelId: 'qwen-30b',
      })
    );
    expect((await screen.findByTestId('free-memory-after')).textContent).toContain('14.0 GB');
  });

  it('disables a peer’s Quit with the reason when remote app quit is off', async () => {
    mockHolders.mockResolvedValue({ ...GABEE, quitAppsAllowed: false });
    show({ target: 'n-gabee' });
    expect(await screen.findByTestId('free-memory-quit-com.google.Chrome')).toBeDisabled();
    expect(screen.getByTestId('free-memory-remote-off').textContent).toContain('Gabee');
  });

  it('shows the measurement failure in words', async () => {
    mockHolders.mockRejectedValue(new Error('engine gone'));
    show();
    expect((await screen.findByTestId('free-memory-error')).textContent).toContain('engine gone');
  });
});
