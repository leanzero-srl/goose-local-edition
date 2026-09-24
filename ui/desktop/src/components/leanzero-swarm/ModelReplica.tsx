import { Loader2, Network, X, Zap } from 'lucide-react';
import {
  Button,
  Chip,
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
import { defineMessages, useIntl } from '../../i18n';
import type { ReplicaLinkKind, ReplicaProgress } from '../../acp/mlx-replica';
import { formatBytesShort } from './primitives';
import { LocalNetworkNotice } from './LocalNetworkNotice';

const i18n = defineMessages({
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

function isRunning(job: ReplicaJob): boolean {
  if (job.error != null) return false;
  return (
    job.progress == null || job.progress.state === 'queued' || job.progress.state === 'copying'
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
