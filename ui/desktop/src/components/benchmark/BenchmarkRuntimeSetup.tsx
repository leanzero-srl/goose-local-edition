import { useCallback, useEffect, useState } from 'react';
import type { BenchmarkRuntimeProgress, BenchmarkRuntimeStatus } from '../../benchRuntimeTypes';
import { Button, Panel, TYPE } from '../lz';

const size = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
export function BenchmarkRuntimeSetup({
  onReady,
  disabled,
}: {
  onReady: (ready: boolean) => void;
  disabled: boolean;
}) {
  const [status, setStatus] = useState<BenchmarkRuntimeStatus | null>(null);
  const [progress, setProgress] = useState<BenchmarkRuntimeProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inspect = useCallback(async () => {
    try {
      const current = await window.electron.benchmarkRuntimeStatus();
      setStatus(current);
      setError(current.error ?? null);
      onReady(current.state === 'ready');
    } catch (err) {
      onReady(false);
      setError(`Runtime status unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, [onReady]);
  useEffect(() => {
    void inspect();
  }, [inspect]);
  useEffect(() => {
    const listener = (_event: unknown, payload: unknown) =>
      setProgress(payload as BenchmarkRuntimeProgress);
    window.electron.on('benchmark-runtime-progress', listener);
    return () => window.electron.off('benchmark-runtime-progress', listener);
  }, []);
  const install = async () => {
    setInstalling(true);
    setError(null);
    setProgress(null);
    onReady(false);
    try {
      await window.electron.benchmarkRuntimeInstall();
      await inspect();
    } catch (err) {
      setError(`Installation failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setInstalling(false);
    }
  };
  return (
    <Panel title="Benchmark tools">
      <p className={TYPE.bodyMuted}>
        Python and video tools are optional downloads required to run SB7.1. The app checks its
        browser before each launch.
      </p>
      {status?.state === 'ready' ? (
        <p className="mt-2 text-lz-ok">Benchmark tools ready</p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          {status && (
            <p className={TYPE.body}>
              {size(status.downloadBytes)} download · installed only when you choose Install
            </p>
          )}
          {installing && (
            <p role="status">
              {progress?.phase ?? 'Preparing installation'}
              {progress?.receivedBytes != null
                ? ` · ${size(progress.receivedBytes)}${progress.totalBytes != null ? ` / ${size(progress.totalBytes)}` : ''}`
                : ''}
            </p>
          )}
          {status?.state !== 'unsupported' && (
            <Button
              variant="secondary"
              disabled={disabled || installing || !status}
              onClick={install}
            >
              {installing
                ? 'Installing…'
                : error
                  ? 'Retry installation'
                  : 'Install benchmark tools'}
            </Button>
          )}
          {!status && !installing && error && (
            <Button variant="secondary" onClick={inspect}>
              Retry status check
            </Button>
          )}
        </div>
      )}
      {error && (
        <p role="alert" className="mt-2 text-lz-err">
          {error}
        </p>
      )}
    </Panel>
  );
}
