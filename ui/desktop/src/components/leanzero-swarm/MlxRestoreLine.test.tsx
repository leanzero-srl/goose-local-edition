import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import { MlxRestoreBanner } from './MlxRestoreLine';
import { latestRestoreLine, publishRestoreLine } from './mlxRestore';

const retry = vi.hoisted(() => vi.fn());
vi.mock('./mlxRestore', async (importActual) => ({
  ...(await importActual<typeof import('./mlxRestore')>()),
  retryRestore: () => retry(),
}));

const QWEN = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';

function banner() {
  return render(
    <IntlTestWrapper>
      <MlxRestoreBanner />
    </IntlTestWrapper>
  );
}

afterEach(() => {
  act(() => publishRestoreLine({ phase: 'idle' }));
  retry.mockReset();
});

describe('the restore line on the Engine tab', () => {
  it('nothing to restore: nothing shown', () => {
    banner();
    expect(screen.queryByTestId('mlx-restore')).toBeNull();
  });

  it('restoring: where it comes back, by the Mac’s one name', () => {
    const { container } = banner();
    act(() =>
      publishRestoreLine({
        phase: 'restoring',
        what: { kind: 'remoteSingle', modelId: QWEN, peerName: "Work's Mac Studio" },
      })
    );
    expect(screen.getByTestId('mlx-restore')).toHaveTextContent(
      "Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio…"
    );
    assertStudioClean(container);
  });

  it('this Mac and the split say where too', () => {
    banner();
    act(() =>
      publishRestoreLine({
        phase: 'restoring',
        what: { kind: 'single', modelId: QWEN, peerName: null },
      })
    );
    expect(screen.getByTestId('mlx-restore')).toHaveTextContent(
      'Restoring Qwen3.8-27B-Atlassian-Q8-mlx on this Mac…'
    );
    act(() =>
      publishRestoreLine({
        phase: 'restoring',
        what: { kind: 'split', modelId: QWEN, peerName: null },
      })
    );
    expect(screen.getByTestId('mlx-restore')).toHaveTextContent(
      'Restoring Qwen3.8-27B-Atlassian-Q8-mlx across your Macs…'
    );
  });

  it('failed: red, goose’s reason, Try again and Dismiss', async () => {
    const user = userEvent.setup();
    banner();
    act(() =>
      publishRestoreLine({
        phase: 'failed',
        what: { kind: 'remoteSingle', modelId: QWEN, peerName: "Work's Mac Studio" },
        reason: { code: 'linkDown', detail: 'tailscaled did not start' },
      })
    );
    const line = screen.getByTestId('mlx-restore');
    expect(line).toHaveAttribute('data-tone', 'err');
    expect(line).toHaveTextContent(
      "Could not restore Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio: LeanZero Link is not connected (tailscaled did not start)"
    );
    await user.click(screen.getByTestId('mlx-restore-retry'));
    expect(retry).toHaveBeenCalledTimes(1);
    await user.click(screen.getByTestId('mlx-restore-dismiss'));
    expect(latestRestoreLine()).toEqual({ phase: 'idle' });
    expect(screen.queryByTestId('mlx-restore')).toBeNull();
  });

  it('the previous split still shutting down: amber, where, and that goose restores it after', () => {
    const { container } = banner();
    act(() =>
      publishRestoreLine({
        phase: 'restoring',
        what: { kind: 'split', modelId: QWEN, peerName: null },
        waitingOn: 'Mihai Macbook',
      })
    );
    const line = screen.getByTestId('mlx-restore');
    expect(line).toHaveAttribute('data-tone', 'warn');
    expect(line).toHaveTextContent(
      'The previous split is still shutting down on Mihai Macbook — goose restores it when that finishes'
    );
    expect(screen.queryByTestId('mlx-restore-details')).toBeNull();
    assertStudioClean(container);
  });

  it('another MLX split: its words on the line, the pid only behind Details', async () => {
    const user = userEvent.setup();
    const { container } = banner();
    act(() =>
      publishRestoreLine({
        phase: 'failed',
        what: { kind: 'split', modelId: QWEN, peerName: null },
        reason: {
          code: 'said',
          text: 'Another MLX split (not goose’s) is running on Mihai Macbook — stop it to start this one',
          detail: 'pid 9425 `/Applications/Xcode.app/…/Python -c import base64,sys;exe`',
        },
      })
    );
    const line = screen.getByTestId('mlx-restore');
    expect(line).toHaveTextContent(
      'Could not restore Qwen3.8-27B-Atlassian-Q8-mlx across your Macs: Another MLX split (not goose’s) is running on Mihai Macbook — stop it to start this one'
    );
    expect(line).not.toHaveTextContent('9425');
    expect(screen.queryByTestId('mlx-restore-detail')).toBeNull();
    await user.click(screen.getByTestId('mlx-restore-details'));
    expect(screen.getByTestId('mlx-restore-detail')).toHaveTextContent('pid 9425');
    expect(screen.getByTestId('mlx-restore-details')).toHaveTextContent('Hide details');
    assertStudioClean(container);
    await user.click(screen.getByTestId('mlx-restore-details'));
    expect(screen.queryByTestId('mlx-restore-detail')).toBeNull();
  });

  it('an unreadable record says so', () => {
    banner();
    act(() =>
      publishRestoreLine({
        phase: 'failed',
        what: null,
        reason: { code: 'said', text: 'the record is unreadable: expected value' },
      })
    );
    expect(screen.getByTestId('mlx-restore')).toHaveTextContent(
      'Could not read what served before the relaunch: the record is unreadable: expected value'
    );
  });
});
