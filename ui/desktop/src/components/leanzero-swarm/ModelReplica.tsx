import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Loader2, Network, RefreshCw, X, Zap } from 'lucide-react';
import {
  Button,
  Chip,
  Panel,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type Tone,
} from '../lz';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { defineMessages, useIntl } from '../../i18n';
import {
  mlxEngineReplicaCancel,
  mlxEngineReplicaProgress,
  mlxEngineReplicaTargets,
  mlxEngineReplicate,
  type ReplicaLink,
  type ReplicaLinkKind,
  type ReplicaProgress,
  type ReplicaTarget,
  type ReplicaTargets,
} from '../../acp/mlx-replica';
import type { MlxDownloadProgress, MlxLocalModel } from '../../acp/mlx-engine';
import { mlxErrorMessage } from './mlxErrorMessage';
import { formatBytesShort } from './primitives';
import { ToneBanner } from './studio';
import { LocalNetworkNotice, touchLocalNetwork } from './LocalNetworkNotice';

const i18n = defineMessages({
  copyTo: {
    id: 'mlxReplica.copyTo',
    defaultMessage: 'Copy to {host} · {kind, select, thunderbolt {Thunderbolt} other {network}}',
  },
  copyTitleThunderbolt: {
    id: 'mlxReplica.copyTitleThunderbolt',
    defaultMessage: 'Copies over the Thunderbolt cable: {local} → {peer}',
  },
  copyTitleNetwork: {
    id: 'mlxReplica.copyTitleNetwork',
    defaultMessage:
      'No Thunderbolt link to {host}, so the copy goes over the local network: {local} → {peer}',
  },
  linksTitle: { id: 'mlxReplica.linksTitle', defaultMessage: 'Linked devices' },
  checkLinks: { id: 'mlxReplica.checkLinks', defaultMessage: 'Check links' },
  linkThunderbolt: { id: 'mlxReplica.linkThunderbolt', defaultMessage: 'Thunderbolt' },
  linkThunderboltSpeed: {
    id: 'mlxReplica.linkThunderboltSpeed',
    defaultMessage: 'Thunderbolt · {speed}',
  },
  linkNetwork: { id: 'mlxReplica.linkNetwork', defaultMessage: 'network' },
  networkNote: {
    id: 'mlxReplica.networkNote',
    defaultMessage: 'No Thunderbolt link, so copies go over the local network.',
  },
  linksNote: {
    id: 'mlxReplica.linksNote',
    defaultMessage:
      'A model on this device can be copied to a linked device instead of being downloaded again. Every file is checked against the original before it is used.',
  },
  linksError: {
    id: 'mlxReplica.linksError',
    defaultMessage: 'Could not check the links to your other devices: {reason}',
  },
  checking: { id: 'mlxReplica.checking', defaultMessage: 'Checking links…' },
  copyingTo: {
    id: 'mlxReplica.copyingTo',
    defaultMessage:
      'Copying to {host} over {kind, select, thunderbolt {Thunderbolt} other {the network}}',
  },
  copiedTo: { id: 'mlxReplica.copiedTo', defaultMessage: 'Copied to {host}' },
  copyFailed: { id: 'mlxReplica.copyFailed', defaultMessage: 'Copy to {host} failed' },
  copyCancelled: { id: 'mlxReplica.copyCancelled', defaultMessage: 'Copy to {host} cancelled' },
  starting: { id: 'mlxReplica.starting', defaultMessage: 'Starting the copy to {host}…' },
  files: { id: 'mlxReplica.files', defaultMessage: '{done} of {total} files' },
  rate: { id: 'mlxReplica.rate', defaultMessage: '{rate}/s' },
  verifying: { id: 'mlxReplica.verifying', defaultMessage: 'checking {file}' },
  resumed: {
    id: 'mlxReplica.resumed',
    defaultMessage: 'continued from a partial copy: {count, plural, one {# file} other {# files}}',
  },
  restarted: {
    id: 'mlxReplica.restarted',
    defaultMessage: 'restarted from zero: {count, plural, one {# file} other {# files}}',
  },
  releaseWarning: {
    id: 'mlxReplica.releaseWarning',
    defaultMessage: 'The sending device kept its offer open: {reason}',
  },
  progressLabel: {
    id: 'mlxReplica.progressLabel',
    defaultMessage: 'Copy progress for {model} to {host}',
  },
  cancel: { id: 'mlxReplica.cancel', defaultMessage: 'Cancel' },
  cancelTitle: {
    id: 'mlxReplica.cancelTitle',
    defaultMessage: 'Stop the copy and delete the partial copy on {host}',
  },
  dismiss: { id: 'mlxReplica.dismiss', defaultMessage: 'Dismiss' },
  startFailed: { id: 'mlxReplica.startFailed', defaultMessage: 'The copy did not start.' },
  cancelFailed: {
    id: 'mlxReplica.cancelFailed',
    defaultMessage: 'The copy could not be cancelled.',
  },
  linksFailed: { id: 'mlxReplica.linksFailed', defaultMessage: 'The links could not be read.' },
  cancelConfirmTitle: { id: 'mlxReplica.cancelConfirmTitle', defaultMessage: 'Cancel copy' },
  cancelConfirmMessage: {
    id: 'mlxReplica.cancelConfirmMessage',
    defaultMessage: 'Stop copying {model} to {host}? The partial copy on {host} is deleted.',
  },
  keepCopying: { id: 'mlxReplica.keepCopying', defaultMessage: 'Keep copying' },
  offerLabel: { id: 'mlxReplica.offerLabel', defaultMessage: 'Downloaded' },
  offerText: {
    id: 'mlxReplica.offerText',
    defaultMessage:
      '{model} is on this device now. {host} is linked by Thunderbolt: copy it there instead of downloading it again.',
  },
});

/** One copy this view started: which model, to which device, and the receiver's truth. */
export interface ReplicaJob {
  modelId: string;
  targetNodeId: string;
  targetHostname: string;
  linkKind: ReplicaLinkKind;
  /** The RECEIVING device's progress; null until its first answer. */
  progress: ReplicaProgress | null;
  /** The start or cancel failed — the reason, verbatim. */
  error: string | null;
}

export interface ModelReplicas {
  /** This device's Link node id: a copy whose receiver is this id is fixed on THIS Mac. */
  selfNodeId: string | null;
  targets: ReplicaTargets | null;
  targetsError: string | null;
  checking: boolean;
  refreshTargets: () => void;
  jobs: Record<string, ReplicaJob>;
  start: (modelId: string, target: ReplicaTarget) => void;
  cancel: (modelId: string) => void;
  dismiss: (modelId: string) => void;
}

const JOB_POLL_MS = 1000;

function isRunning(job: ReplicaJob): boolean {
  if (job.error != null) return false;
  return (
    job.progress == null || job.progress.state === 'queued' || job.progress.state === 'copying'
  );
}

/**
 * The copy targets and the copies in flight, for the device the Models tab shows
 * (`senderNodeId`, undefined = this device). `enabled` is false when Link is off or no peer is
 * linked — then nothing is fetched and every surface renders nothing: a single machine looks
 * exactly as it did before. Targets are read when the tab opens, the device or the peer set
 * changes, and on "Check links" — each read asks every peer for its interfaces, so it is not a
 * poll. Copies poll the RECEIVER every second while they run.
 */
export function useModelReplicas({
  enabled,
  selfNodeId = null,
  senderNodeId,
  peerKey,
}: {
  enabled: boolean;
  selfNodeId?: string | null;
  senderNodeId?: string;
  /** Changes whenever the linked peer set changes, so the targets are re-read. */
  peerKey: string;
}): ModelReplicas {
  const intl = useIntl();
  const [targets, setTargets] = useState<ReplicaTargets | null>(null);
  const [targetsError, setTargetsError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [jobs, setJobs] = useState<Record<string, ReplicaJob>>({});
  const readSeq = useRef(0);
  // Plain strings, so the callbacks below do not change identity with the intl object.
  const linksFailed = intl.formatMessage(i18n.linksFailed);
  const startFailed = intl.formatMessage(i18n.startFailed);
  const cancelFailed = intl.formatMessage(i18n.cancelFailed);

  const refreshTargets = useCallback(() => {
    if (!enabled) return;
    const seq = ++readSeq.current;
    setChecking(true);
    void (async () => {
      try {
        const next = await mlxEngineReplicaTargets(senderNodeId);
        if (seq !== readSeq.current) return;
        setTargets(next);
        setTargetsError(null);
      } catch (error) {
        if (seq !== readSeq.current) return;
        setTargetsError(mlxErrorMessage(error, linksFailed));
      } finally {
        if (seq === readSeq.current) setChecking(false);
      }
    })();
  }, [enabled, senderNodeId, linksFailed]);

  useEffect(() => {
    if (!enabled) {
      readSeq.current += 1;
      setTargets(null);
      setTargetsError(null);
      setChecking(false);
      return;
    }
    refreshTargets();
  }, [enabled, senderNodeId, peerKey, refreshTargets]);

  // A different sending device is a different set of copies. Returning `prev` when there is
  // nothing to drop keeps the mount free of an extra render.
  useEffect(() => {
    setJobs((prev) => (Object.keys(prev).length === 0 ? prev : {}));
  }, [senderNodeId]);

  const setJob = useCallback((modelId: string, apply: (job: ReplicaJob) => ReplicaJob) => {
    setJobs((prev) => (prev[modelId] ? { ...prev, [modelId]: apply(prev[modelId]) } : prev));
  }, []);

  const start = useCallback(
    (modelId: string, target: ReplicaTarget) => {
      if (!target.link) return;
      setJobs((prev) => ({
        ...prev,
        [modelId]: {
          modelId,
          targetNodeId: target.nodeId,
          targetHostname: target.hostname,
          linkKind: target.link!.kind,
          progress: null,
          error: null,
        },
      }));
      void (async () => {
        try {
          await touchLocalNetwork();
          await mlxEngineReplicate(modelId, target.nodeId, senderNodeId);
        } catch (error) {
          setJob(modelId, (job) => ({
            ...job,
            error: mlxErrorMessage(error, startFailed),
          }));
        }
      })();
    },
    [senderNodeId, setJob, startFailed]
  );

  const cancel = useCallback(
    (modelId: string) => {
      const job = jobs[modelId];
      if (!job) return;
      void (async () => {
        try {
          await mlxEngineReplicaCancel(modelId, job.targetNodeId);
        } catch (error) {
          setJob(modelId, (j) => ({
            ...j,
            error: mlxErrorMessage(error, cancelFailed),
          }));
        }
      })();
    },
    [jobs, setJob, cancelFailed]
  );

  const dismiss = useCallback((modelId: string) => {
    setJobs((prev) => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });
  }, []);

  const runningKey = useMemo(
    () =>
      Object.values(jobs)
        .filter(isRunning)
        .map((job) => `${job.modelId}\t${job.targetNodeId}`)
        .sort()
        .join('\n'),
    [jobs]
  );
  useEffect(() => {
    if (runningKey === '') return undefined;
    const running = runningKey.split('\n').map((line) => line.split('\t') as [string, string]);
    const timer = setInterval(() => {
      for (const [modelId, receiver] of running) {
        void (async () => {
          try {
            const progress = await mlxEngineReplicaProgress(modelId, receiver);
            if (progress) setJob(modelId, (job) => ({ ...job, progress }));
          } catch {
            // A transient poll failure keeps the last real numbers rather than inventing any.
          }
        })();
      }
    }, JOB_POLL_MS);
    return () => clearInterval(timer);
  }, [runningKey, setJob]);

  return {
    selfNodeId,
    targets,
    targetsError,
    checking,
    refreshTargets,
    jobs,
    start,
    cancel,
    dismiss,
  };
}

function availableTargets(replicas: ModelReplicas): ReplicaTarget[] {
  return (replicas.targets?.targets ?? []).filter((t) => t.link != null);
}

function linkChip(link: ReplicaLink, label: string) {
  const tone: Tone = link.kind === 'thunderbolt' ? 'accent' : 'secondary';
  const icon = link.kind === 'thunderbolt' ? <Zap /> : <Network />;
  return (
    <Chip tone={tone} icon={icon} title={`${link.local.ipv4} → ${link.peer.ipv4}`}>
      {label}
    </Chip>
  );
}

/**
 * The linked devices a copy can go to from the shown device, each with its path — or why not.
 * Renders nothing when Link is off, the mesh is down, or no peer is linked.
 */
export function ReplicaLinksPanel({ replicas }: { replicas: ModelReplicas }) {
  const intl = useIntl();
  const { targets, targetsError, checking } = replicas;
  if (targetsError == null && (!targets || !targets.meshConnected || targets.targets.length === 0))
    return null;
  return (
    <Panel
      title={intl.formatMessage(i18n.linksTitle)}
      count={targets?.targets.length}
      headerRight={
        <Button
          size="sm"
          variant="ghost"
          icon={checking ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          onClick={replicas.refreshTargets}
          disabled={checking}
        >
          {checking ? intl.formatMessage(i18n.checking) : intl.formatMessage(i18n.checkLinks)}
        </Button>
      }
    >
      <div className="flex flex-col gap-2">
        {targetsError != null && (
          <ToneBanner
            tone="err"
            label={intl.formatMessage(i18n.linksTitle)}
            text={intl.formatMessage(i18n.linksError, { reason: targetsError })}
          />
        )}
        {(targets?.targets ?? []).map((target) => (
          <div
            key={target.nodeId}
            className="flex min-w-0 flex-wrap items-center gap-2"
            data-testid={`mlx-replica-target-${target.nodeId}`}
          >
            <span className={cx('font-mono text-lz-mono text-lz-ink', WEIGHT.semibold)}>
              {target.hostname}
            </span>
            {target.link ? (
              <>
                {linkChip(
                  target.link,
                  target.link.kind === 'thunderbolt'
                    ? target.link.local.linkSpeed
                      ? intl.formatMessage(i18n.linkThunderboltSpeed, {
                          speed: target.link.local.linkSpeed,
                        })
                      : intl.formatMessage(i18n.linkThunderbolt)
                    : intl.formatMessage(i18n.linkNetwork)
                )}
                <span className={cx(TYPE.meta, TNUM)}>
                  {target.link.local.device} {target.link.local.ipv4} → {target.link.peer.ipv4}
                </span>
                {target.link.kind === 'network' && (
                  <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}>
                    {intl.formatMessage(i18n.networkNote)}
                  </span>
                )}
              </>
            ) : (
              <span
                className={cx('min-w-0 break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
              >
                {target.unavailable}
              </span>
            )}
          </div>
        ))}
        {replicas.targets?.warning && (
          <p className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}>
            {replicas.targets.warning}
          </p>
        )}
        <p className={TYPE.meta}>{intl.formatMessage(i18n.linksNote)}</p>
      </div>
    </Panel>
  );
}

/** One "Copy to <device> · Thunderbolt/network" button per device a copy can go to now. */
export function ReplicaCopyButtons({
  modelId,
  replicas,
}: {
  modelId: string;
  replicas: ModelReplicas;
}) {
  const intl = useIntl();
  const job = replicas.jobs[modelId];
  if (job && isRunning(job)) return null;
  return (
    <>
      {availableTargets(replicas).map((target) => {
        const link = target.link!;
        return (
          <Button
            key={target.nodeId}
            size="sm"
            variant="secondary"
            icon={link.kind === 'thunderbolt' ? <Zap /> : <Network />}
            onClick={() => replicas.start(modelId, target)}
            aria-label={intl.formatMessage(i18n.copyTo, {
              host: target.hostname,
              kind: link.kind,
            })}
            title={
              link.kind === 'thunderbolt'
                ? intl.formatMessage(i18n.copyTitleThunderbolt, {
                    local: link.local.ipv4,
                    peer: link.peer.ipv4,
                  })
                : intl.formatMessage(i18n.copyTitleNetwork, {
                    host: target.hostname,
                    local: link.local.ipv4,
                    peer: link.peer.ipv4,
                  })
            }
          >
            {intl.formatMessage(i18n.copyTo, { host: target.hostname, kind: link.kind })}
          </Button>
        );
      })}
    </>
  );
}

const STATE_TONE: Record<ReplicaProgress['state'], Tone> = {
  queued: 'stopped',
  copying: 'accent',
  done: 'ok',
  failed: 'err',
  cancelled: 'stopped',
};

/** A copy's live row under its model: real bytes from the receiver, the rate, the files. */
export function ReplicaJobRow({
  job,
  receiverIsThisDevice,
  onCancel,
  onDismiss,
}: {
  job: ReplicaJob;
  receiverIsThisDevice: boolean;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const intl = useIntl();
  const progress = job.progress;
  const state = job.error != null ? 'failed' : (progress?.state ?? 'queued');
  const tone = STATE_TONE[state];
  const running = isRunning(job);
  const pct =
    progress && progress.totalBytes > 0
      ? Math.min(100, (progress.copiedBytes / progress.totalBytes) * 100)
      : 0;
  const headline =
    state === 'done'
      ? intl.formatMessage(i18n.copiedTo, { host: job.targetHostname })
      : state === 'failed'
        ? intl.formatMessage(i18n.copyFailed, { host: job.targetHostname })
        : state === 'cancelled'
          ? intl.formatMessage(i18n.copyCancelled, { host: job.targetHostname })
          : progress == null
            ? intl.formatMessage(i18n.starting, { host: job.targetHostname })
            : intl.formatMessage(i18n.copyingTo, {
                host: job.targetHostname,
                kind: job.linkKind,
              });
  const rate =
    progress && progress.wireMillis > 0
      ? intl.formatMessage(i18n.rate, {
          rate: formatBytesShort((progress.wireBytes / progress.wireMillis) * 1000),
        })
      : null;
  const error = job.error ?? progress?.error;
  return (
    <div className="mt-2 flex flex-col gap-1.5" data-testid={`mlx-replica-${job.modelId}`}>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <Chip
          tone={tone}
          icon={
            running ? (
              <Loader2 className="animate-spin" />
            ) : job.linkKind === 'thunderbolt' ? (
              <Zap />
            ) : (
              <Network />
            )
          }
        >
          {headline}
        </Chip>
        {progress && progress.filesTotal > 0 && (
          <span className={cx(TYPE.meta, TNUM)}>
            {intl.formatMessage(i18n.files, {
              done: progress.filesDone,
              total: progress.filesTotal,
            })}
          </span>
        )}
        {rate && (
          <span className={cx('text-lz-meta', WEIGHT.semibold, TNUM, TONE_TEXT.accent)}>
            {rate}
          </span>
        )}
      </div>
      {progress && state !== 'cancelled' && (
        <div className="flex flex-wrap items-center gap-2">
          <div
            className={cx('h-2 min-w-[160px] flex-1 overflow-hidden', RADIUS.pill, SURFACE.inset)}
            role="progressbar"
            aria-valuenow={Math.round(pct)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={intl.formatMessage(i18n.progressLabel, {
              model: job.modelId,
              host: job.targetHostname,
            })}
          >
            <div className={cx('h-full', TONE_DOT[tone])} style={{ width: `${pct}%` }} />
          </div>
          <span className={cx('shrink-0 text-lz-meta', WEIGHT.semibold, TNUM, TONE_TEXT[tone])}>
            {formatBytesShort(progress.copiedBytes)}
            {progress.totalBytes > 0 ? ` / ${formatBytesShort(progress.totalBytes)}` : ''}
          </span>
        </div>
      )}
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {progress?.currentFile && (
          <span className="truncate font-mono text-lz-mono text-lz-ink-3">
            {progress.phase === 'verifying'
              ? intl.formatMessage(i18n.verifying, { file: progress.currentFile })
              : progress.currentFile}
          </span>
        )}
        {running && (
          <Button
            size="sm"
            variant="ghost"
            icon={<X />}
            onClick={onCancel}
            title={intl.formatMessage(i18n.cancelTitle, { host: job.targetHostname })}
          >
            {intl.formatMessage(i18n.cancel)}
          </Button>
        )}
        {!running && (
          <Button size="sm" variant="ghost" icon={<X />} onClick={onDismiss}>
            {intl.formatMessage(i18n.dismiss)}
          </Button>
        )}
      </div>
      {progress?.resumedFiles && progress.resumedFiles.length > 0 && (
        <div className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.accent)}>
          {intl.formatMessage(i18n.resumed, { count: progress.resumedFiles.length })}
        </div>
      )}
      {progress?.restartedFiles && progress.restartedFiles.length > 0 && (
        <div
          className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
          title={progress.restartedFiles.join('\n')}
        >
          {intl.formatMessage(i18n.restarted, { count: progress.restartedFiles.length })}
        </div>
      )}
      {progress?.localNetworkBlocked && (
        <LocalNetworkNotice host={receiverIsThisDevice ? undefined : job.targetHostname} />
      )}
      {error && (
        <div className={cx('break-words text-lz-body', WEIGHT.semibold, TONE_TEXT.err)}>
          {error}
        </div>
      )}
      {progress?.releaseError && (
        <div className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}>
          {intl.formatMessage(i18n.releaseWarning, { reason: progress.releaseError })}
        </div>
      )}
    </div>
  );
}

/**
 * Everything a model row shows about copies: its live copy, else the copy buttons. The cancel
 * deletes the partial copy on ANOTHER device, so it is confirmed first, naming that device.
 */
export function ReplicaModelControls({
  modelId,
  replicas,
}: {
  modelId: string;
  replicas: ModelReplicas;
}) {
  const intl = useIntl();
  const [confirming, setConfirming] = useState(false);
  const job = replicas.jobs[modelId];
  if (!job) return null;
  return (
    <>
      <ReplicaJobRow
        job={job}
        receiverIsThisDevice={job.targetNodeId === replicas.selfNodeId}
        onCancel={() => setConfirming(true)}
        onDismiss={() => replicas.dismiss(modelId)}
      />
      <ConfirmationModal
        isOpen={confirming}
        title={intl.formatMessage(i18n.cancelConfirmTitle)}
        message={intl.formatMessage(i18n.cancelConfirmMessage, {
          model: modelId,
          host: job.targetHostname,
        })}
        confirmLabel={intl.formatMessage(i18n.cancelConfirmTitle)}
        cancelLabel={intl.formatMessage(i18n.keepCopying)}
        confirmVariant="destructive"
        onConfirm={() => {
          setConfirming(false);
          replicas.cancel(modelId);
        }}
        onCancel={() => setConfirming(false)}
      />
    </>
  );
}

/**
 * The inline offer after a download: a model that just finished downloading on the shown device,
 * with a device linked by THUNDERBOLT, gets "Copy to <device> · Thunderbolt" right where the
 * download finished — the reason the feature exists (download once, reuse the cable).
 */
export function ReplicaDownloadOffers({
  downloads,
  models,
  replicas,
}: {
  downloads: Record<string, MlxDownloadProgress>;
  models: MlxLocalModel[];
  replicas: ModelReplicas;
}) {
  const intl = useIntl();
  const [dismissed, setDismissed] = useState<Set<string>>(() => new Set());
  const thunderbolt = availableTargets(replicas).filter((t) => t.link?.kind === 'thunderbolt');
  if (thunderbolt.length === 0) return null;
  const complete = new Set(models.filter((m) => m.complete).map((m) => m.id));
  const finished = Object.entries(downloads)
    .filter(([id, p]) => p.state === 'done' && complete.has(id))
    .map(([id]) => id)
    .filter((id) => !dismissed.has(id) && replicas.jobs[id] == null);
  if (finished.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {finished.map((modelId) => (
        <ToneBanner
          key={modelId}
          tone="ok"
          label={intl.formatMessage(i18n.offerLabel)}
          testId={`mlx-replica-offer-${modelId}`}
          text={intl.formatMessage(i18n.offerText, {
            model: modelId,
            host: thunderbolt.map((t) => t.hostname).join(', '),
          })}
          action={
            <span className="inline-flex flex-wrap items-center gap-1">
              {thunderbolt.map((target) => (
                <Button
                  key={target.nodeId}
                  size="sm"
                  variant="primary"
                  icon={<Zap />}
                  onClick={() => replicas.start(modelId, target)}
                >
                  {intl.formatMessage(i18n.copyTo, {
                    host: target.hostname,
                    kind: 'thunderbolt',
                  })}
                </Button>
              ))}
              <Button
                size="sm"
                variant="ghost"
                icon={<X />}
                aria-label={intl.formatMessage(i18n.dismiss)}
                iconOnly
                onClick={() => setDismissed((prev) => new Set(prev).add(modelId))}
              />
            </span>
          }
        />
      ))}
    </div>
  );
}
