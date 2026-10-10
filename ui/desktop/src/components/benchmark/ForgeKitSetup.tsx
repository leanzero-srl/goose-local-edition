import { useCallback, useEffect, useState } from 'react';
import { CheckCircle2, Loader2 } from 'lucide-react';
import type { ForgeKitStatus } from '../../benchForgeKitTypes';
import { FORGE_BENCHMARK_TIER } from '../../benchTierPayload';
import { TIER_SCORER } from './baselines';
import { Button, Panel, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';

/** The kit parts as a person reads them; an unknown part keeps its own name. */
const PART_WORDS: Record<string, string> = {
  'app-modules': 'Forge app packages',
  'lint-modules': 'Forge lint packages',
  'wrapper/wrapper.js': 'Atlassian runtime wrapper',
  'wrapper/loader.js': 'runtime loader',
  schema: 'manifest schema',
  kit: 'kit tools',
};

/**
 * Forge's own runtime needs, beside the shared Benchmark tools: the bundled era's pinned Forge module
 * trees (npm ci from the committed lockfiles) and Atlassian's runtime wrapper, fetched and sha-pinned by
 * the era's kit module in the payload — never shipped in the app. Readiness is its status(), read without
 * the network, and the launch refuses until it says ready. Also states the Forge tier's run policy.
 */
export function ForgeKitSetup({
  toolsReady,
  disabled,
  onStatus,
}: {
  /** The shared Benchmark tools (Python/Node) are installed — the kit cannot be read without them. */
  toolsReady: boolean;
  disabled: boolean;
  onStatus: (status: ForgeKitStatus | null) => void;
}) {
  const [status, setStatus] = useState<ForgeKitStatus | null>(null);
  const [preparing, setPreparing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const publish = useCallback(
    (next: ForgeKitStatus | null) => {
      setStatus(next);
      onStatus(next);
    },
    [onStatus]
  );

  const inspect = useCallback(async () => {
    try {
      const next = await window.electron.benchmarkForgeKitStatus();
      publish(next);
      setError(
        next.state === 'error' || next.state === 'needs-tools' ? (next.error ?? null) : null
      );
    } catch (err) {
      publish(null);
      setError(`Forge kit status unavailable: ${err instanceof Error ? err.message : String(err)}`);
    }
  }, [publish]);

  useEffect(() => {
    void inspect();
  }, [inspect, toolsReady]);

  const prepare = async () => {
    setPreparing(true);
    setError(null);
    try {
      const next = await window.electron.benchmarkForgeKitPrepare();
      publish(next);
      if (next.state !== 'ready') setError(next.error ?? 'The Forge kit is still incomplete.');
    } catch (err) {
      setError(
        `Preparing the Forge kit failed: ${err instanceof Error ? err.message : String(err)}`
      );
      await inspect();
    } finally {
      setPreparing(false);
    }
  };

  const ready = status?.state === 'ready';
  const missing = (status?.missing ?? []).map((part) => PART_WORDS[part] ?? part);
  return (
    <Panel
      title="Forge kit"
      headerRight={<span className={TYPE.meta}>{TIER_SCORER[FORGE_BENCHMARK_TIER]} runtime</span>}
    >
      <p className={cx('max-w-[75ch]', TYPE.bodyMuted)}>
        Forge apps run on Atlassian&rsquo;s own runtime wrapper and the pinned Forge packages.
        Neither ships in Goose Swarm: preparing the kit downloads them once, checks every file
        against its pinned hash, and refuses anything that differs.
      </p>
      {ready ? (
        <p
          data-testid="forge-kit-ready"
          className={cx(
            'mt-3 flex items-center gap-2 text-lz-body [&>svg]:size-4',
            WEIGHT.semibold,
            TONE_TEXT.ok
          )}
        >
          <CheckCircle2 aria-hidden />
          Forge kit ready
          {status?.kitLockSha256 && (
            <span className={cx('font-mono', TYPE.meta)}>{status.kitLockSha256.slice(0, 12)}</span>
          )}
        </p>
      ) : status?.state === 'needs-tools' || !toolsReady ? (
        <p className={cx('mt-3', TYPE.body)}>
          Install Benchmark tools first — the kit is prepared with them.
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-3">
          {status?.state === 'missing' && missing.length > 0 && (
            <p className={TYPE.body}>Not prepared yet: {missing.join(', ')}.</p>
          )}
          {preparing && (
            <p role="status" className={cx('flex items-center gap-2 [&>svg]:size-4', TYPE.body)}>
              <Loader2 className="animate-spin" aria-hidden />
              Downloading and verifying the Forge kit — the first time takes a few minutes.
            </p>
          )}
          <div>
            <Button
              variant="secondary"
              disabled={disabled || preparing || !status}
              onClick={prepare}
            >
              {preparing ? 'Preparing…' : error ? 'Retry preparing the kit' : 'Prepare Forge kit'}
            </Button>
          </div>
        </div>
      )}
      {error && (
        <p role="alert" className={cx('mt-2 whitespace-pre-wrap break-words', TONE_TEXT.err)}>
          {error}
        </p>
      )}
    </Panel>
  );
}
