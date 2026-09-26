import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../i18n/test-utils';
import { liveSplitSnapshot } from '../utils/mlxInFlight.fixtures';

const distributedStop = vi.fn();
const unmount = vi.fn();
vi.mock('../acp/mlx-engine', () => ({
  mlxEngineMount: vi.fn(),
  mlxEngineUnmount: (...a: unknown[]) => unmount(...a),
  mlxEngineStatus: vi.fn(async () => ({ state: 'stopped' })),
  mlxEngineSettingsRead: vi.fn(),
}));
vi.mock('../acp/mlx-distributed', async (importActual) => ({
  ...(await importActual<typeof import('../acp/mlx-distributed')>()),
  mlxDistributedStop: (...a: unknown[]) => distributedStop(...a),
  mlxDistributedStatus: vi.fn(async () => ({ mode: 'single', state: 'stopped', nodes: [] })),
}));
vi.mock('../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ mlxDistributed: false }),
}));
vi.mock('./useMlxRemoteReporter', () => ({ useMlxRemoteReporter: () => undefined }));

import { useMlxTrayActions } from './useMlxTrayActions';

function Host() {
  return useMlxTrayActions();
}

type Bridge = {
  on: ReturnType<typeof vi.fn>;
  mlxEngineActivity?: () => Promise<unknown>;
};
const bridge = window.electron as unknown as Bridge;

/** The tray's click as main delivers it: `mlx-tray-action` to the window's listener. */
function trayClick(action: string) {
  const call = bridge.on.mock.calls.find(([channel]) => channel === 'mlx-tray-action');
  if (!call) throw new Error('no mlx-tray-action listener');
  act(() => (call[1] as (e: unknown, a: string) => void)({}, action));
}

describe('Q-148: the tray’s stop asks in the window while the split writes', () => {
  beforeEach(() => {
    bridge.on.mockClear();
    distributedStop.mockReset();
    distributedStop.mockResolvedValue({ stop: { verified: true, steps: [] } });
  });
  afterEach(() => {
    cleanup();
    delete bridge.mlxEngineActivity;
  });

  it('“Stop the distributed engine” during the 39-min answer asks, naming it, and stops only on yes', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot());
    render(
      <IntlTestWrapper>
        <Host />
      </IntlTestWrapper>
    );
    trayClick('stop-distributed');
    const dialog = await screen.findByRole('dialog');
    expect(
      within(dialog).getByText(
        'Stop the split cuts the answer being written in “Jira Migration Kickoff Notes” — 39m 15s in, 24,228 tokens written.'
      )
    ).toBeInTheDocument();
    expect(distributedStop).not.toHaveBeenCalled();
    await userEvent.click(within(dialog).getByRole('button', { name: 'Keep it writing' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(distributedStop).not.toHaveBeenCalled();

    trayClick('stop-distributed');
    await userEvent.click(
      within(await screen.findByRole('dialog')).getByRole('button', { name: 'Stop the split' })
    );
    await waitFor(() => expect(distributedStop).toHaveBeenCalledTimes(1));
  });

  it('with the split idle, the tray’s stop acts at once — no question', async () => {
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot([]));
    render(
      <IntlTestWrapper>
        <Host />
      </IntlTestWrapper>
    );
    trayClick('stop-distributed');
    await waitFor(() => expect(distributedStop).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('work on ANOTHER engine than the one stopped is not asked about', async () => {
    // The split writes; the tray's Unmount stops this Mac's single engine, which holds nothing.
    bridge.mlxEngineActivity = vi.fn(async () => liveSplitSnapshot());
    unmount.mockResolvedValue(undefined);
    render(
      <IntlTestWrapper>
        <Host />
      </IntlTestWrapper>
    );
    trayClick('unmount');
    await waitFor(() => expect(unmount).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
