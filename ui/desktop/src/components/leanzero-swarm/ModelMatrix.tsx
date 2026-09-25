import { useMemo, useState } from 'react';
import { Download, Network, Pause, Play, SlidersHorizontal, Trash2, X, Zap } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
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
  type EnginePhase,
} from '../lz';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import type { MlxDownloadProgress, MlxLocalModel } from '../../acp/mlx-engine';
import { formatGb } from './primitives';
import { mlxErrorMessage } from './mlxErrorMessage';
import { peerRefuses, type Mac } from './macs';
import { ReplicaJobRow } from './ModelReplica';
import {
  copyKey,
  copyRunning,
  useMacs,
  type CopyJob,
  type MacFacts,
  type MacsValue,
} from './useMacs';

/**
 * MODELS — one row per model, one column per Mac. A cell says what that Mac holds of that model
 * (on disk, loaded, copying, downloading, not there) and offers the one thing that gets it there:
 * a copy from a Mac that has it over the cable they share, else a download. A column goose could
 * not read says "Can't read" in red with the reason once under its name — never a "0".
 */

const i18n = defineMessages({
  model: { id: 'modelMatrix.model', defaultMessage: 'Model' },
  empty: {
    id: 'modelMatrix.empty',
    defaultMessage: 'No models on your Macs yet — download one from the Hugging Face tab.',
  },
  free: { id: 'modelMatrix.free', defaultMessage: '{free} free' },
  onDisk: { id: 'modelMatrix.onDisk', defaultMessage: 'On disk' },
  loaded: { id: 'modelMatrix.loaded', defaultMessage: 'Loaded' },
  loading: { id: 'modelMatrix.loading', defaultMessage: 'Loading' },
  notHere: { id: 'modelMatrix.notHere', defaultMessage: 'Not here' },
  cantRead: { id: 'modelMatrix.cantRead', defaultMessage: 'Can’t read' },
  checking: { id: 'modelMatrix.checking', defaultMessage: 'Checking…' },
  offline: { id: 'modelMatrix.offline', defaultMessage: 'Offline' },
  incomplete: {
    id: 'modelMatrix.incomplete',
    defaultMessage: 'Incomplete · {count, plural, one {# file} other {# files}} missing',
  },
  copying: { id: 'modelMatrix.copying', defaultMessage: 'Copying {pct}%' },
  copyStarting: { id: 'modelMatrix.copyStarting', defaultMessage: 'Starting the copy' },
  copyFailed: { id: 'modelMatrix.copyFailed', defaultMessage: 'Copy failed' },
  downloading: { id: 'modelMatrix.downloading', defaultMessage: 'Downloading {pct}%' },
  queued: { id: 'modelMatrix.queued', defaultMessage: 'Queued' },
  paused: { id: 'modelMatrix.paused', defaultMessage: 'Paused {pct}%' },
  downloadFailed: { id: 'modelMatrix.downloadFailed', defaultMessage: 'Download failed' },
  copyFrom: {
    id: 'modelMatrix.copyFrom',
    defaultMessage: 'Copy from {name} · {kind, select, thunderbolt {Thunderbolt} other {network}}',
  },
  copyFromTitle: {
    id: 'modelMatrix.copyFromTitle',
    defaultMessage:
      'Copies {name}’s files straight over {kind, select, thunderbolt {the Thunderbolt cable} other {the local network}}; every file is checked against the original.',
  },
  downloadHere: { id: 'modelMatrix.downloadHere', defaultMessage: 'Download here' },
  noCopyPath: { id: 'modelMatrix.noCopyPath', defaultMessage: 'No copy from {name}: {reason}' },
  downloadHereTitle: {
    id: 'modelMatrix.downloadHereTitle',
    defaultMessage: 'Download {model} from Hugging Face onto {name}',
  },
  resume: { id: 'modelMatrix.resume', defaultMessage: 'Resume' },
  pause: { id: 'modelMatrix.pause', defaultMessage: 'Pause' },
  cancel: { id: 'modelMatrix.cancel', defaultMessage: 'Cancel' },
  dismiss: { id: 'modelMatrix.dismiss', defaultMessage: 'Dismiss' },
  sampling: { id: 'modelMatrix.sampling', defaultMessage: 'Sampling on {name}' },
  delete: { id: 'modelMatrix.delete', defaultMessage: 'Delete from {name}' },
  deleteTitle: { id: 'modelMatrix.deleteTitle', defaultMessage: 'Delete model' },
  deleteMessage: {
    id: 'modelMatrix.deleteMessage',
    defaultMessage:
      'Delete {model} ({size}) from {name}? Its files are removed from that Mac’s disk.',
  },
  deleteConfirm: { id: 'modelMatrix.deleteConfirm', defaultMessage: 'Delete' },
  deleteFailed: { id: 'modelMatrix.deleteFailed', defaultMessage: 'Could not delete {model}.' },
  cancelCopyTitle: { id: 'modelMatrix.cancelCopyTitle', defaultMessage: 'Cancel copy' },
  cancelCopyMessage: {
    id: 'modelMatrix.cancelCopyMessage',
    defaultMessage: 'Stop copying {model} to {name}? The partial copy on {name} is deleted.',
  },
  keepCopying: { id: 'modelMatrix.keepCopying', defaultMessage: 'Keep copying' },
  cancelDownloadTitle: { id: 'modelMatrix.cancelDownloadTitle', defaultMessage: 'Cancel download' },
  cancelDownloadMessage: {
    id: 'modelMatrix.cancelDownloadMessage',
    defaultMessage: 'Cancel the download of {model} on {name} and delete its partial files there?',
  },
  keep: { id: 'modelMatrix.keep', defaultMessage: 'Keep' },
});

type Pending =
  | { kind: 'delete'; mac: Mac; model: MlxLocalModel }
  | { kind: 'cancelCopy'; job: CopyJob }
  | { kind: 'cancelDownload'; mac: Mac; modelId: string };

function pct(done: number, total: number): number {
  return total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;
}

/** A thin solid bar under a cell's state — real bytes only. */
function CellBar({ value, tone }: { value: number; tone: 'accent' | 'stopped' | 'err' }) {
  return (
    <div
      role="progressbar"
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={100}
      className={cx('h-1.5 w-full min-w-[96px] overflow-hidden', RADIUS.pill, SURFACE.inset)}
    >
      <div className={cx('h-full', TONE_DOT[tone])} style={{ width: `${value}%` }} />
    </div>
  );
}

/** Whether a Mac's column can be read at all, and why not. */
function columnGap(
  mac: Mac,
  facts: MacFacts,
  offText: (mac: Mac) => string
): { word: 'offline' | 'cantRead' | 'checking'; reason: string | null } | null {
  if (!mac.online) return { word: 'offline', reason: mac.pollError };
  if (peerRefuses(mac, 'manage')) return { word: 'cantRead', reason: offText(mac) };
  if (facts.modelsError) return { word: 'cantRead', reason: facts.modelsError };
  if (facts.models == null) return { word: 'checking', reason: null };
  return null;
}

interface CellProps {
  mac: Mac;
  modelId: string;
  macs: Mac[];
  onPending: (pending: Pending) => void;
  onOpenSampling: (macKey: string, modelId: string) => void;
}

function MatrixCell({ mac, modelId, macs, onPending, onOpenSampling }: CellProps) {
  const intl = useIntl();
  const ctx = useMacs();
  const facts = ctx.factsOf(mac.key);
  const gap = columnGap(mac, facts, (m) => ctx.offText(m, 'manage'));
  const job = ctx.copies[copyKey(modelId, mac.key)];
  const progress: MlxDownloadProgress | undefined = ctx.downloads[mac.key]?.[modelId];
  const downloadError = ctx.downloadErrors[mac.key]?.[modelId];
  const model = facts.models?.find((m) => m.id === modelId) ?? null;
  const testId = `model-cell-${mac.key}-${modelId}`;

  if (job && (copyRunning(job) || job.error != null || job.progress?.state === 'failed')) {
    const failed = job.error != null || job.progress?.state === 'failed';
    const value = job.progress ? pct(job.progress.copiedBytes, job.progress.totalBytes) : 0;
    return (
      <div className="flex min-w-0 flex-col gap-1" data-testid={testId} data-cell="copying">
        <span className="flex items-center gap-1.5">
          <Chip
            tone={failed ? 'err' : 'accent'}
            icon={failed ? undefined : job.linkKind === 'thunderbolt' ? <Zap /> : <Network />}
          >
            {failed
              ? intl.formatMessage(i18n.copyFailed)
              : job.progress
                ? intl.formatMessage(i18n.copying, { pct: value })
                : intl.formatMessage(i18n.copyStarting)}
          </Chip>
          <Button
            size="sm"
            variant="ghost"
            iconOnly
            icon={<X />}
            aria-label={intl.formatMessage(failed ? i18n.dismiss : i18n.cancel)}
            title={intl.formatMessage(failed ? i18n.dismiss : i18n.cancel)}
            onClick={() =>
              failed ? ctx.dismissCopy(job.key) : onPending({ kind: 'cancelCopy', job })
            }
          />
        </span>
        {!failed && <CellBar value={value} tone="accent" />}
      </div>
    );
  }

  if (progress && progress.state !== 'done') {
    const value = pct(progress.downloadedBytes, progress.totalBytes);
    const paused = progress.state === 'paused';
    const failed = progress.state === 'failed';
    const word = failed
      ? intl.formatMessage(i18n.downloadFailed)
      : paused
        ? intl.formatMessage(i18n.paused, { pct: value })
        : progress.state === 'queued'
          ? intl.formatMessage(i18n.queued)
          : intl.formatMessage(i18n.downloading, { pct: value });
    return (
      <div className="flex min-w-0 flex-col gap-1" data-testid={testId} data-cell="downloading">
        <span className="flex items-center gap-1">
          <Chip tone={failed ? 'err' : paused ? 'stopped' : 'accent'} icon={<Download />}>
            {word}
          </Chip>
          {paused || failed ? (
            <Button
              size="sm"
              variant="ghost"
              iconOnly
              icon={<Play />}
              aria-label={intl.formatMessage(i18n.resume)}
              title={intl.formatMessage(i18n.resume)}
              onClick={() => void ctx.resumeDownload(mac.key, modelId)}
            />
          ) : (
            <Button
              size="sm"
              variant="ghost"
              iconOnly
              icon={<Pause />}
              aria-label={intl.formatMessage(i18n.pause)}
              title={intl.formatMessage(i18n.pause)}
              onClick={() => void ctx.pauseDownload(mac.key, modelId)}
            />
          )}
          <Button
            size="sm"
            variant="ghost"
            iconOnly
            icon={<X />}
            aria-label={intl.formatMessage(i18n.cancel)}
            title={intl.formatMessage(i18n.cancel)}
            onClick={() => onPending({ kind: 'cancelDownload', mac, modelId })}
          />
        </span>
        {!failed && <CellBar value={value} tone={paused ? 'stopped' : 'accent'} />}
        {(progress.error || downloadError) && (
          <span className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {progress.error ?? downloadError}
          </span>
        )}
      </div>
    );
  }

  if (gap) {
    return (
      <span data-testid={testId} data-cell={gap.word} title={gap.reason ?? undefined}>
        {gap.word === 'cantRead' ? (
          <Chip tone="err">{intl.formatMessage(i18n.cantRead)}</Chip>
        ) : (
          <span className={TYPE.meta}>
            {intl.formatMessage(gap.word === 'offline' ? i18n.offline : i18n.checking)}
          </span>
        )}
      </span>
    );
  }

  if (model) {
    const incomplete = model.missingFiles > 0 || !model.complete;
    const status = facts.status;
    const inEngine =
      status?.modelId === modelId && (status.state === 'running' || status.state === 'mounting');
    const phase: EnginePhase = status?.state === 'mounting' ? 'loading' : 'idle';
    return (
      <div className="flex min-w-0 flex-col gap-1" data-testid={testId} data-cell="present">
        <span className="flex flex-wrap items-center gap-1">
          {incomplete ? (
            <Chip tone="warn">
              {intl.formatMessage(i18n.incomplete, { count: Math.max(1, model.missingFiles) })}
            </Chip>
          ) : inEngine ? (
            // The copy an engine holds is the one FILLED chip in the table, in the engine-phase
            // palette; a copy that only sits on disk is quiet. A solid green "On disk" beside a
            // grey "Loaded" made the serving copy look the least alive (Q-45).
            <Chip phase={phase}>
              {intl.formatMessage(phase === 'loading' ? i18n.loading : i18n.loaded)}
            </Chip>
          ) : (
            <Chip>{intl.formatMessage(i18n.onDisk)}</Chip>
          )}
          <span className={cx(TYPE.meta, TNUM)}>{formatGb(model.sizeBytes)}</span>
          {incomplete ? (
            <Button
              size="sm"
              variant="ghost"
              iconOnly
              icon={<Play />}
              aria-label={intl.formatMessage(i18n.resume)}
              title={intl.formatMessage(i18n.resume)}
              onClick={() => void ctx.resumeDownload(mac.key, modelId)}
            />
          ) : (
            <Button
              size="sm"
              variant="ghost"
              iconOnly
              icon={<SlidersHorizontal />}
              aria-label={intl.formatMessage(i18n.sampling, { name: mac.name })}
              title={intl.formatMessage(i18n.sampling, { name: mac.name })}
              onClick={() => onOpenSampling(mac.key, modelId)}
            />
          )}
          <Button
            size="sm"
            variant="ghost"
            iconOnly
            icon={<Trash2 />}
            aria-label={intl.formatMessage(i18n.delete, { name: mac.name })}
            title={intl.formatMessage(i18n.delete, { name: mac.name })}
            onClick={() => onPending({ kind: 'delete', mac, model })}
          />
        </span>
        {downloadError && (
          <span className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {downloadError}
          </span>
        )}
      </div>
    );
  }

  // Not here: a copy from a Mac that has it complete, over the path they share — else a download.
  const source = macs.find((other) => {
    if (other.key === mac.key) return false;
    const has = ctx.factsOf(other.key).models?.find((m) => m.id === modelId);
    return (
      has != null && has.complete && has.missingFiles === 0 && ctx.linkBetween(other.key, mac.key)
    );
  });
  const link = source ? ctx.linkBetween(source.key, mac.key) : null;
  // A Mac holds it but shares no path with this one: say why the copy is not offered.
  const holder = source
    ? undefined
    : macs.find((other) => {
        if (other.key === mac.key) return false;
        const has = ctx.factsOf(other.key).models?.find((m) => m.id === modelId);
        return has != null && has.complete && has.missingFiles === 0;
      });
  const noPath = holder ? ctx.whyNoLink(holder.key, mac.key) : null;
  return (
    <div
      className="flex min-w-0 flex-col items-start gap-1"
      data-testid={testId}
      data-cell="absent"
    >
      <span className={TYPE.meta}>{intl.formatMessage(i18n.notHere)}</span>
      {source && link ? (
        <Button
          size="sm"
          variant="secondary"
          icon={link.kind === 'thunderbolt' ? <Zap /> : <Network />}
          title={intl.formatMessage(i18n.copyFromTitle, { name: source.name, kind: link.kind })}
          onClick={() => ctx.copy(modelId, source.key, mac.key)}
        >
          {intl.formatMessage(i18n.copyFrom, { name: source.name, kind: link.kind })}
        </Button>
      ) : (
        <Button
          size="sm"
          variant="secondary"
          icon={<Download />}
          title={intl.formatMessage(i18n.downloadHereTitle, { model: modelId, name: mac.name })}
          onClick={() => void ctx.download(mac.key, modelId)}
        >
          {intl.formatMessage(i18n.downloadHere)}
        </Button>
      )}
      {holder && noPath && (
        <span className={cx('break-words', TYPE.meta)} data-testid={`${testId}-no-copy`}>
          {intl.formatMessage(i18n.noCopyPath, { name: holder.name, reason: noPath })}
        </span>
      )}
      {downloadError && (
        <span className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
          {downloadError}
        </span>
      )}
    </div>
  );
}

function ColumnHeader({ mac }: { mac: Mac }) {
  const intl = useIntl();
  const ctx = useMacs();
  const facts = ctx.factsOf(mac.key);
  const gap = columnGap(mac, facts, (m) => ctx.offText(m, 'manage'));
  return (
    <div className="flex min-w-[160px] flex-col gap-0.5 normal-case">
      <span className={cx('text-lz-body text-lz-ink', WEIGHT.semibold)}>{mac.name}</span>
      {facts.disk && !gap && (
        <span className={cx(TYPE.meta, TNUM)}>
          {intl.formatMessage(i18n.free, { free: formatGb(facts.disk.availableBytes) })}
        </span>
      )}
      {gap?.word === 'cantRead' && gap.reason && (
        <span
          data-testid={`model-column-gap-${mac.key}`}
          className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}
        >
          {ctx.describeError(mac, gap.reason)}
        </span>
      )}
    </div>
  );
}

/** Every model any Mac holds, plus the ones arriving on one by copy or download — the table's rows. */
export function matrixRows(
  ctx: Pick<MacsValue, 'macs' | 'factsOf' | 'downloads' | 'copies'>
): string[] {
  const ids = new Set<string>();
  for (const mac of ctx.macs) {
    for (const m of ctx.factsOf(mac.key).models ?? []) ids.add(m.id);
    for (const id of Object.keys(ctx.downloads[mac.key] ?? {})) ids.add(id);
  }
  for (const job of Object.values(ctx.copies)) ids.add(job.modelId);
  return [...ids].sort((a, b) => a.localeCompare(b));
}

/** What the Models tab counts: exactly the rows the table shows. */
export function matrixRowCount(
  ctx: Pick<MacsValue, 'macs' | 'factsOf' | 'downloads' | 'copies'>
): number {
  return matrixRows(ctx).length;
}

export function ModelMatrix({
  onOpenSampling,
}: {
  onOpenSampling: (macKey: string, modelId: string) => void;
}) {
  const intl = useIntl();
  const ctx = useMacs();
  const { macs } = ctx;
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const rows = useMemo(() => matrixRows(ctx), [ctx]);

  // "No models" is said only when every column was READ — an unreadable Mac is not an empty one.
  const allRead = macs.every(
    (mac) => columnGap(mac, ctx.factsOf(mac.key), (m) => ctx.offText(m, 'manage')) == null
  );

  const sizeOf = (id: string) => {
    for (const mac of macs) {
      const m = ctx.factsOf(mac.key).models?.find((x) => x.id === id);
      if (m) return m.sizeBytes;
    }
    return null;
  };

  const confirm = async () => {
    if (!pending) return;
    if (pending.kind === 'cancelCopy') {
      setPending(null);
      await ctx.cancelCopy(pending.job.key);
      return;
    }
    if (pending.kind === 'cancelDownload') {
      setPending(null);
      await ctx.cancelDownload(pending.mac.key, pending.modelId);
      return;
    }
    setBusy(true);
    setDeleteError(null);
    try {
      await ctx.deleteModel(pending.mac.key, pending.model.id);
      setPending(null);
    } catch (e) {
      setDeleteError(
        ctx.describeError(
          pending.mac,
          mlxErrorMessage(e, intl.formatMessage(i18n.deleteFailed, { model: pending.model.id }))
        )
      );
      setPending(null);
    } finally {
      setBusy(false);
    }
  };

  const modal = (() => {
    if (!pending) return { title: '', message: '', confirm: '', cancel: undefined };
    if (pending.kind === 'delete') {
      return {
        title: intl.formatMessage(i18n.deleteTitle),
        message: intl.formatMessage(i18n.deleteMessage, {
          model: pending.model.id,
          size: formatGb(pending.model.sizeBytes),
          name: pending.mac.name,
        }),
        confirm: intl.formatMessage(i18n.deleteConfirm),
        cancel: undefined,
      };
    }
    if (pending.kind === 'cancelCopy') {
      const to = ctx.macByKey(pending.job.toKey);
      return {
        title: intl.formatMessage(i18n.cancelCopyTitle),
        message: intl.formatMessage(i18n.cancelCopyMessage, {
          model: pending.job.modelId,
          name: to?.name ?? pending.job.toKey,
        }),
        confirm: intl.formatMessage(i18n.cancelCopyTitle),
        cancel: intl.formatMessage(i18n.keepCopying),
      };
    }
    return {
      title: intl.formatMessage(i18n.cancelDownloadTitle),
      message: intl.formatMessage(i18n.cancelDownloadMessage, {
        model: pending.modelId,
        name: pending.mac.name,
      }),
      confirm: intl.formatMessage(i18n.cancelDownloadTitle),
      cancel: intl.formatMessage(i18n.keep),
    };
  })();

  return (
    <div className={cx('overflow-x-auto', SURFACE.card)} data-testid="model-matrix">
      {deleteError && (
        <p
          role="alert"
          className={cx(
            'break-words border-b px-4 py-3',
            TYPE.body,
            WEIGHT.semibold,
            TONE_TEXT.err,
            SURFACE.hairline
          )}
        >
          {deleteError}
        </p>
      )}
      <table className="w-full border-collapse text-left">
        <thead>
          <tr className={cx('border-b', SURFACE.hairline)}>
            <th scope="col" className={cx('px-4 py-2 align-bottom', TYPE.zone, 'text-lz-ink-3')}>
              {intl.formatMessage(i18n.model)}
            </th>
            {macs.map((mac) => (
              <th key={mac.key} scope="col" className="px-3 py-2 align-bottom">
                <ColumnHeader mac={mac} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((id) => {
            const jobs = macs
              .map((mac) => ctx.copies[copyKey(id, mac.key)])
              .filter((j): j is CopyJob => j != null && (copyRunning(j) || j.error != null));
            const size = sizeOf(id);
            return (
              <tr
                key={id}
                className={cx('border-b last:border-b-0 align-top', SURFACE.hairline)}
                data-testid={`model-row-${id}`}
              >
                <td className="max-w-[360px] px-4 py-3">
                  <span className="block truncate font-mono text-lz-mono text-lz-ink" title={id}>
                    {id}
                  </span>
                  {size != null && <span className={cx(TYPE.meta, TNUM)}>{formatGb(size)}</span>}
                  {jobs.map((job) => {
                    const to = ctx.macByKey(job.toKey);
                    return (
                      <ReplicaJobRow
                        key={job.key}
                        job={{
                          modelId: job.modelId,
                          targetNodeId: to?.nodeId ?? job.toKey,
                          targetHostname: to?.name ?? job.toKey,
                          linkKind: job.linkKind,
                          progress: job.progress,
                          error: job.error,
                        }}
                        receiverIsThisDevice={to?.isSelf ?? false}
                        onCancel={() => setPending({ kind: 'cancelCopy', job })}
                        onDismiss={() => ctx.dismissCopy(job.key)}
                      />
                    );
                  })}
                </td>
                {macs.map((mac) => (
                  <td key={mac.key} className="px-3 py-3">
                    <MatrixCell
                      mac={mac}
                      modelId={id}
                      macs={macs}
                      onPending={setPending}
                      onOpenSampling={onOpenSampling}
                    />
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
      {rows.length === 0 && allRead && (
        <p className={cx('px-4 py-6', TYPE.bodyMuted)} data-testid="model-matrix-empty">
          {intl.formatMessage(i18n.empty)}
        </p>
      )}
      <ConfirmationModal
        isOpen={pending != null}
        title={modal.title}
        message={modal.message}
        confirmLabel={modal.confirm}
        cancelLabel={modal.cancel}
        confirmVariant="destructive"
        isSubmitting={busy}
        onConfirm={() => void confirm()}
        onCancel={() => setPending(null)}
      />
    </div>
  );
}
