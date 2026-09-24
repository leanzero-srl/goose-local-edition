import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ReplicaJobRow, type ReplicaJob } from './ModelReplica';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });
const GB = 1024 * 1024 * 1024;
const MODEL = 'mlx-community/Qwen3-30B-A3B-4bit';

afterEach(() => cleanup());

/**
 * The copy's detail row under a model in the Models table: the receiver's real bytes, the rate, the
 * files, and what went wrong in goose's words. The cancel it offers is confirmed by the table.
 */
describe('a running copy', () => {
  const running: ReplicaJob = {
    modelId: MODEL,
    targetNodeId: 'peer-workhorse',
    targetHostname: 'Work’s Mac Studio',
    linkKind: 'thunderbolt',
    error: null,
    progress: {
      state: 'copying',
      sourceUrl: 'http://192.168.0.1:54496',
      link: 'thunderbolt',
      linkDetail: 'Thunderbolt 3 en3 192.168.0.1 → 192.168.0.2 (80 Gb/s)',
      totalBytes: 4 * GB,
      copiedBytes: 4 * GB,
      filesTotal: 4,
      filesDone: 3,
      currentFile: 'model-00004-of-00004.safetensors',
      phase: 'verifying',
      resumedFiles: ['model-00002-of-00004.safetensors'],
      wireBytes: 3 * GB,
      wireMillis: 1000,
      elapsedMillis: 1500,
    },
  };

  it('says it is verifying, counts resumed files, the measured rate, and hands the cancel up', async () => {
    const onCancel = vi.fn();
    render(
      <ReplicaJobRow
        job={running}
        receiverIsThisDevice={false}
        onCancel={onCancel}
        onDismiss={vi.fn()}
      />
    );
    const row = screen.getByTestId(`mlx-replica-${MODEL}`);
    expect(row).toHaveTextContent('Copying to Work’s Mac Studio over Thunderbolt');
    expect(row).toHaveTextContent('checking model-00004-of-00004.safetensors');
    expect(row).toHaveTextContent('continued from a partial copy: 1 file');
    expect(row).toHaveTextContent('3.00 GB/s');
    expect(row).toHaveTextContent('3 of 4 files');
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('a verification failure is shown verbatim, with Dismiss instead of Cancel', () => {
    const failed: ReplicaJob = {
      ...running,
      progress: {
        ...running.progress!,
        state: 'failed',
        phase: undefined,
        error: "model-00004-of-00004.safetensors failed verification: the sender's sha256 is ab…",
      },
    };
    render(
      <ReplicaJobRow
        job={failed}
        receiverIsThisDevice={false}
        onCancel={vi.fn()}
        onDismiss={vi.fn()}
      />
    );
    const row = screen.getByTestId(`mlx-replica-${MODEL}`);
    expect(row).toHaveTextContent('Copy to Work’s Mac Studio failed');
    expect(row).toHaveTextContent('failed verification');
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
  });

  it('a pull macOS refused the local network is named on the Mac that must allow it', async () => {
    const refused: ReplicaJob = {
      ...running,
      targetHostname: 'workhorse',
      progress: {
        ...running.progress!,
        state: 'failed',
        phase: undefined,
        localNetworkBlocked: true,
        error:
          'fetching the manifest of …: error sending request: No route to host (os error 65) — macOS is blocking Goose Swarm from the local network on this node',
      },
    };
    const { rerender } = render(
      <ReplicaJobRow
        job={refused}
        receiverIsThisDevice={false}
        onCancel={vi.fn()}
        onDismiss={vi.fn()}
      />
    );
    // The receiver is workhorse: the fix is there, so nothing here opens THIS Mac's settings.
    expect(screen.getByTestId('local-network-blocked')).toHaveTextContent(
      'macOS on workhorse is blocking Goose Swarm from the local network — allow it on workhorse in System Settings › Privacy & Security › Local Network'
    );
    expect(screen.queryByRole('button', { name: 'Open Privacy & Security' })).toBeNull();
    expect(screen.getByTestId(`mlx-replica-${MODEL}`)).toHaveTextContent('No route to host');

    rerender(
      <ReplicaJobRow job={refused} receiverIsThisDevice onCancel={vi.fn()} onDismiss={vi.fn()} />
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open Privacy & Security' }));
    expect(window.electron.openLocalNetworkSettings).toHaveBeenCalled();
  });
});
