import { describe, it, expect, vi, afterEach } from 'vitest';
import { cleanup, render as rtlRender, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import {
  ReplicaDownloadOffers,
  ReplicaModelControls,
  type ModelReplicas,
  type ReplicaJob,
} from './ModelReplica';
import type { ReplicaTarget } from '../../acp/mlx-replica';
import type { MlxDownloadProgress, MlxLocalModel } from '../../acp/mlx-engine';

const render = (ui: React.ReactElement) => rtlRender(ui, { wrapper: IntlTestWrapper });
const GB = 1024 * 1024 * 1024;
const MODEL = 'mlx-community/Qwen3-30B-A3B-4bit';

const iface = (ipv4: string, kind: 'thunderbolt' | 'wifi') => ({
  device: 'en3',
  kind,
  ipv4,
  prefixLen: kind === 'thunderbolt' ? 30 : 24,
});

const TB: ReplicaTarget = {
  nodeId: 'peer-workhorse',
  hostname: 'workhorse',
  link: {
    kind: 'thunderbolt',
    local: iface('192.168.0.1', 'thunderbolt'),
    peer: iface('192.168.0.2', 'thunderbolt'),
  },
};
const LAN: ReplicaTarget = {
  nodeId: 'peer-laptop',
  hostname: 'laptop',
  link: {
    kind: 'network',
    local: iface('192.168.10.2', 'wifi'),
    peer: iface('192.168.10.3', 'wifi'),
  },
};

function replicasWith(
  targets: ReplicaTarget[],
  jobs: Record<string, ReplicaJob> = {}
): ModelReplicas {
  return {
    selfNodeId: null,
    targets: { meshConnected: true, targets },
    targetsError: null,
    checking: false,
    refreshTargets: vi.fn(),
    jobs,
    start: vi.fn(),
    cancel: vi.fn(),
    dismiss: vi.fn(),
  };
}

const DONE: Record<string, MlxDownloadProgress> = {
  [MODEL]: { state: 'done', totalBytes: 17 * GB, downloadedBytes: 17 * GB },
};
const COMPLETE: MlxLocalModel[] = [
  { id: MODEL, sizeBytes: 17 * GB, complete: true, missingFiles: 0 },
];

afterEach(() => cleanup());

describe('the inline offer after a download', () => {
  it('a finished download with a Thunderbolt peer offers the copy right there', async () => {
    const replicas = replicasWith([TB, LAN]);
    render(<ReplicaDownloadOffers downloads={DONE} models={COMPLETE} replicas={replicas} />);
    const offer = screen.getByTestId(`mlx-replica-offer-${MODEL}`);
    expect(offer).toHaveTextContent(
      `${MODEL} is on this device now. workhorse is linked by Thunderbolt`
    );
    // Only the Thunderbolt peer is offered inline; the LAN peer stays on the model row.
    expect(screen.getAllByRole('button', { name: /^Copy to/ })).toHaveLength(1);
    await userEvent.click(screen.getByRole('button', { name: 'Copy to workhorse · Thunderbolt' }));
    expect(replicas.start).toHaveBeenCalledWith(MODEL, TB);
    assertStudioClean(document.body);
    expect(
      await missingUtilities(allClasses(document.body).filter((c) => !c.startsWith('lucide')))
    ).toEqual([]);
  });

  it('no Thunderbolt peer, an unfinished download or an incomplete model: no offer', () => {
    const { container, rerender } = render(
      <ReplicaDownloadOffers downloads={DONE} models={COMPLETE} replicas={replicasWith([LAN])} />
    );
    expect(container).toBeEmptyDOMElement();
    rerender(
      <ReplicaDownloadOffers
        downloads={{ [MODEL]: { state: 'downloading', totalBytes: 10, downloadedBytes: 5 } }}
        models={COMPLETE}
        replicas={replicasWith([TB])}
      />
    );
    expect(container).toBeEmptyDOMElement();
    rerender(
      <ReplicaDownloadOffers
        downloads={DONE}
        models={[{ ...COMPLETE[0], complete: false, missingFiles: 1 }]}
        replicas={replicasWith([TB])}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('dismissing the offer hides it', async () => {
    render(
      <ReplicaDownloadOffers downloads={DONE} models={COMPLETE} replicas={replicasWith([TB])} />
    );
    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId(`mlx-replica-offer-${MODEL}`)).not.toBeInTheDocument();
  });
});

describe('a running copy', () => {
  const running: ReplicaJob = {
    modelId: MODEL,
    targetNodeId: 'peer-workhorse',
    targetHostname: 'workhorse',
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

  it('says it is verifying, counts resumed files, and cancels only after naming the device', async () => {
    const replicas = replicasWith([TB], { [MODEL]: running });
    render(<ReplicaModelControls modelId={MODEL} replicas={replicas} />);
    const row = screen.getByTestId(`mlx-replica-${MODEL}`);
    expect(row).toHaveTextContent('checking model-00004-of-00004.safetensors');
    expect(row).toHaveTextContent('continued from a partial copy: 1 file');
    expect(row).toHaveTextContent('3.00 GB/s');
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(replicas.cancel).not.toHaveBeenCalled();
    expect(
      screen.getByText(
        `Stop copying ${MODEL} to workhorse? The partial copy on workhorse is deleted.`
      )
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Cancel copy' }));
    expect(replicas.cancel).toHaveBeenCalledWith(MODEL);
  });

  it('a verification failure is shown verbatim', () => {
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
      <ReplicaModelControls modelId={MODEL} replicas={replicasWith([TB], { [MODEL]: failed })} />
    );
    const row = screen.getByTestId(`mlx-replica-${MODEL}`);
    expect(row).toHaveTextContent('Copy to workhorse failed');
    expect(row).toHaveTextContent('failed verification');
    expect(screen.getByRole('button', { name: 'Dismiss' })).toBeInTheDocument();
  });

  it('a pull macOS refused the local network is named on the Mac that must allow it', async () => {
    const refused: ReplicaJob = {
      ...running,
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
      <ReplicaModelControls modelId={MODEL} replicas={replicasWith([TB], { [MODEL]: refused })} />
    );
    // The receiver is workhorse: the fix is there, so nothing here opens THIS Mac's settings.
    expect(screen.getByTestId('local-network-blocked')).toHaveTextContent(
      'macOS on workhorse is blocking Goose Swarm from the local network — allow it on workhorse in System Settings › Privacy & Security › Local Network'
    );
    expect(screen.queryByRole('button', { name: 'Open Privacy & Security' })).toBeNull();
    expect(screen.getByTestId(`mlx-replica-${MODEL}`)).toHaveTextContent('No route to host');

    rerender(
      <ReplicaModelControls
        modelId={MODEL}
        replicas={{ ...replicasWith([TB], { [MODEL]: refused }), selfNodeId: 'peer-workhorse' }}
      />
    );
    await userEvent.click(screen.getByRole('button', { name: 'Open Privacy & Security' }));
    expect(window.electron.openLocalNetworkSettings).toHaveBeenCalled();
  });
});
