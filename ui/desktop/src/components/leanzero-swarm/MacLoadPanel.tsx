import { useEffect, useRef, useState } from 'react';
import { Loader2, Square, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Segmented, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { useConfirmDialog } from '../ui/useConfirmDialog';
import { acpReadConfig, acpUpsertConfig } from '../../acp/config';
import {
  jobErrorText,
  leanzeroLinkJobsCancel,
  leanzeroLinkJobsGet,
  leanzeroLinkJobsLog,
  type JobBrief,
  type JobState,
  type JobView,
  type NodeLoad,
} from '../../acp/leanzero-link';
import { INPUT, StudioSwitch } from './studio';
import {
  BAR_FILL,
  JOBS_DEFAULTS,
  JOBS_KEYS,
  USER_ACTIVE_POLICIES,
  inferenceCount,
  isTerminal,
  liveJobs,
  loadBars,
  ownParts,
  parseBool,
  parseNumber,
  parsePolicy,
  type BarId,
  type UserActivePolicy,
} from './macLoad';
import { macNameOfNode } from './macs';
import { useMacsMaybe } from './useMacs';

const i18n = defineMessages({
  loadTitle: { id: 'macLoad.title', defaultMessage: 'Load' },
  own: { id: 'macLoad.own', defaultMessage: 'Own use' },
  inference: { id: 'macLoad.inference', defaultMessage: 'AI inference' },
  jobs: { id: 'macLoad.jobs', defaultMessage: 'Jobs' },
  ownLine: {
    id: 'macLoad.ownLine',
    defaultMessage: '{user} · cpu {cpu} · memory {mem} used · gpu {gpu}',
  },
  userActive: { id: 'macLoad.userActive', defaultMessage: 'Someone is using it' },
  userIdle: { id: 'macLoad.userIdle', defaultMessage: 'Idle {idle}' },
  inferenceLine: {
    id: 'macLoad.inferenceLine',
    defaultMessage:
      '{engine, plural, one {# engine request} other {# engine requests}} · {slots, plural, one {# router slot} other {# router slots}}',
  },
  jobsLine: {
    id: 'macLoad.jobsLine',
    defaultMessage:
      '{live, plural, =0 {No job running} one {# job running} other {# jobs running}}',
  },
  noLoad: {
    id: 'macLoad.noLoad',
    defaultMessage: 'Its goose does not report load yet (an older build).',
  },
  benchmark: { id: 'macLoad.benchmark', defaultMessage: 'Benchmark running: {why}' },
  jobFrom: { id: 'macLoad.jobFrom', defaultMessage: '{kind} from {origin}' },
  stateCancelling: { id: 'macLoad.stateCancelling', defaultMessage: 'Cancelling…' },
  dialogTitle: { id: 'macLoad.dialogTitle', defaultMessage: 'Job {id}' },
  state: { id: 'macLoad.state', defaultMessage: 'State' },
  kind: { id: 'macLoad.kind', defaultMessage: 'Kind' },
  origin: { id: 'macLoad.origin', defaultMessage: 'Sent from' },
  started: { id: 'macLoad.started', defaultMessage: 'Started' },
  ended: { id: 'macLoad.ended', defaultMessage: 'Ended' },
  exit: { id: 'macLoad.exit', defaultMessage: 'Exit code' },
  reason: { id: 'macLoad.reason', defaultMessage: 'Why' },
  log: { id: 'macLoad.log', defaultMessage: 'Log' },
  logEmpty: { id: 'macLoad.logEmpty', defaultMessage: 'Nothing written yet.' },
  close: { id: 'macLoad.close', defaultMessage: 'Close' },
  cancelJob: { id: 'macLoad.cancelJob', defaultMessage: 'Cancel job' },
  cancelTitle: { id: 'macLoad.cancelTitle', defaultMessage: 'Cancel job {id}?' },
  cancelBody: {
    id: 'macLoad.cancelBody',
    defaultMessage:
      'This stops the job on {mac} and every process it started. What it already wrote stays in its log.',
  },
  keepRunning: { id: 'macLoad.keepRunning', defaultMessage: 'Keep running' },
  settingsTitle: { id: 'macLoad.settingsTitle', defaultMessage: 'Jobs from other Macs' },
  allowJobs: { id: 'macLoad.allowJobs', defaultMessage: 'Run jobs other Macs send' },
  allowJobsHint: {
    id: 'macLoad.allowJobsHint',
    defaultMessage:
      'Shell commands and prompts, one heavy job at a time, killed when they pass the memory floor. Takes effect at once.',
  },
  policy: { id: 'macLoad.policy', defaultMessage: 'While someone uses this Mac' },
  policy_allow: { id: 'macLoad.policy.allow', defaultMessage: 'Take jobs' },
  policy_last_resort: { id: 'macLoad.policy.lastResort', defaultMessage: 'Only as a last resort' },
  policy_refuse: { id: 'macLoad.policy.refuse', defaultMessage: 'Refuse jobs' },
  idleS: { id: 'macLoad.idleS', defaultMessage: 'Counts as idle after (seconds)' },
  minFree: { id: 'macLoad.minFree', defaultMessage: 'Memory floor (% kept free)' },
  reconnectHint: {
    id: 'macLoad.reconnectHint',
    defaultMessage: 'The policy and the memory floor apply after Link reconnects.',
  },
  saveFailed: { id: 'macLoad.saveFailed', defaultMessage: 'Not saved: {reason}' },
});

const BAR_LABEL: Record<BarId, (typeof i18n)['own']> = {
  own: i18n.own,
  inference: i18n.inference,
  jobs: i18n.jobs,
};

const LOG_POLL_MS = 2000;

/** The job's state as shown: `cancelling` in words (it is waiting on the group), the rest verbatim. */
function stateText(intl: ReturnType<typeof useIntl>, state: JobState): string {
  return state === 'cancelling' ? intl.formatMessage(i18n.stateCancelling) : state;
}

/** Node id → the Mac's name, through the view's Macs when a MacsProvider is above. */
function useNodeName(): (nodeId: string) => string {
  const macs = useMacsMaybe()?.macs ?? [];
  return (nodeId) => macNameOfNode(macs, nodeId);
}

function pctText(v: number | null): string {
  return v == null ? '—' : `${v}%`;
}

function idleText(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h`;
}

/** The three bars and the job list of one Mac; `target` is the Link node id job calls go to. */
export function MacLoadBars({
  load,
  target,
  macName,
}: {
  load: NodeLoad | null;
  target: string | null;
  macName: string;
}) {
  const intl = useIntl();
  const nodeName = useNodeName();
  const [openJob, setOpenJob] = useState<JobBrief | null>(null);
  if (!load) {
    return (
      <p data-testid="mac-load-none" className={TYPE.meta}>
        {intl.formatMessage(i18n.noLoad)}
      </p>
    );
  }
  const own = ownParts(load);
  const words: Record<BarId, string> = {
    own: intl.formatMessage(i18n.ownLine, {
      user: own.userActive
        ? intl.formatMessage(i18n.userActive)
        : intl.formatMessage(i18n.userIdle, {
            idle: own.idleS == null ? '—' : idleText(own.idleS),
          }),
      cpu: pctText(own.cpuPct),
      mem: pctText(own.memUsedPct),
      gpu: pctText(own.gpuPct),
    }),
    inference: intl.formatMessage(i18n.inferenceLine, {
      engine: load.inference.engine_active,
      slots: load.inference.router_slots_used,
    }),
    jobs: intl.formatMessage(i18n.jobsLine, { live: liveJobs(load.jobs).length }),
  };
  const counts: Record<BarId, string> = {
    own: `${loadBars(load)[0].pct}%`,
    inference: String(inferenceCount(load)),
    jobs: String(liveJobs(load.jobs).length),
  };
  return (
    <div data-testid="mac-load" className="flex flex-col gap-2">
      {load.own.benchmark && (
        <p
          data-testid="mac-load-benchmark"
          className={cx(TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}
        >
          {intl.formatMessage(i18n.benchmark, { why: load.own.benchmark })}
        </p>
      )}
      {loadBars(load).map((bar) => (
        <div key={bar.id} data-testid={`mac-load-${bar.id}`} className="flex flex-col gap-1">
          <div className="flex items-baseline justify-between gap-2">
            <span className={cx(TYPE.meta, WEIGHT.semibold, 'uppercase')}>
              {intl.formatMessage(BAR_LABEL[bar.id])}
            </span>
            <span className={cx(TYPE.meta, TNUM)} data-testid={`mac-load-${bar.id}-count`}>
              {counts[bar.id]}
            </span>
          </div>
          <div
            role="meter"
            aria-label={intl.formatMessage(BAR_LABEL[bar.id])}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={bar.pct}
            className="h-2.5 w-full border border-lz-border bg-lz-surface-2"
          >
            <div
              data-testid={`mac-load-${bar.id}-fill`}
              className={cx('h-full', BAR_FILL[bar.id])}
              style={{ width: `${bar.pct}%` }}
            />
          </div>
          <span className={cx(TYPE.meta, TNUM, 'break-words')}>{words[bar.id]}</span>
        </div>
      ))}
      {load.jobs.length > 0 && (
        <ul data-testid="mac-load-job-list" className="flex flex-col gap-1">
          {load.jobs.map((job) => (
            <li key={job.job_id} className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                data-testid={`mac-job-${job.job_id}`}
                className={cx(TYPE.meta, TNUM, WEIGHT.semibold, 'underline underline-offset-2')}
                onClick={() => setOpenJob(job)}
              >
                {job.job_id}
              </button>
              <span className={TYPE.meta}>
                {intl.formatMessage(i18n.jobFrom, {
                  kind: job.kind,
                  origin: nodeName(job.origin_node),
                })}
              </span>
              <span
                data-testid={`mac-job-${job.job_id}-state`}
                className={cx(
                  TYPE.meta,
                  WEIGHT.semibold,
                  isTerminal(job.state) ? '' : TONE_TEXT.ok
                )}
              >
                {stateText(intl, job.state)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {openJob && target && (
        <JobDialog
          brief={openJob}
          target={target}
          macName={macName}
          onClose={() => setOpenJob(null)}
        />
      )}
    </div>
  );
}

export function JobDialog({
  brief,
  target,
  macName,
  onClose,
}: {
  brief: JobBrief;
  target: string;
  macName: string;
  onClose: () => void;
}) {
  const intl = useIntl();
  const nodeName = useNodeName();
  const { confirm, dialog } = useConfirmDialog();
  const [view, setView] = useState<JobView | null>(null);
  const [log, setLog] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const next = useRef(0);
  const done = useRef(false);
  // The view's state is final: with the log complete, nothing more will change. Until then (a
  // `cancelling` job whose group has not exited, say) the view is read again on every tick.
  const ended = useRef(isTerminal(brief.state));

  useEffect(() => {
    let alive = true;
    const read = async () => {
      try {
        const v = await leanzeroLinkJobsGet(target, brief.job_id);
        if (!alive) return;
        ended.current = isTerminal(v.state);
        setView(v);
        if (!done.current) {
          const chunk = await leanzeroLinkJobsLog(target, brief.job_id, next.current);
          if (!alive) return;
          next.current = chunk.next;
          done.current = chunk.done;
          if (chunk.data) setLog((prev) => prev + chunk.data);
        }
        setError(null);
      } catch (e) {
        if (alive) setError(jobErrorText(e));
      }
    };
    void read();
    const timer = setInterval(() => {
      if (!done.current || !ended.current) void read();
    }, LOG_POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [target, brief.job_id]);

  const shown = view ?? brief;
  const cancel = async () => {
    const ok = await confirm({
      title: intl.formatMessage(i18n.cancelTitle, { id: brief.job_id }),
      message: intl.formatMessage(i18n.cancelBody, { mac: macName }),
      confirmLabel: intl.formatMessage(i18n.cancelJob),
      cancelLabel: intl.formatMessage(i18n.keepRunning),
      confirmVariant: 'destructive',
    });
    if (!ok) return;
    setCancelling(true);
    try {
      const v = await leanzeroLinkJobsCancel(target, brief.job_id);
      ended.current = isTerminal(v.state);
      setView(v);
      setError(null);
    } catch (e) {
      setError(jobErrorText(e));
    } finally {
      setCancelling(false);
    }
  };

  const rows: { key: string; label: string; value: string }[] = [
    { key: 'state', label: intl.formatMessage(i18n.state), value: stateText(intl, shown.state) },
    { key: 'kind', label: intl.formatMessage(i18n.kind), value: shown.kind },
    { key: 'origin', label: intl.formatMessage(i18n.origin), value: nodeName(shown.origin_node) },
    { key: 'started', label: intl.formatMessage(i18n.started), value: shown.started_at },
    ...(view?.ended_at
      ? [{ key: 'ended', label: intl.formatMessage(i18n.ended), value: view.ended_at }]
      : []),
    ...(view?.exit_code != null
      ? [{ key: 'exit', label: intl.formatMessage(i18n.exit), value: String(view.exit_code) }]
      : []),
    ...(view?.reason
      ? [{ key: 'reason', label: intl.formatMessage(i18n.reason), value: view.reason }]
      : []),
  ];

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx('flex w-[40rem] max-w-[90vw] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div data-testid="job-dialog" className="flex flex-col gap-3">
        <OverlayDialogTitle asChild>
          <h2 className={cx('break-words', TYPE.h2)}>
            {intl.formatMessage(i18n.dialogTitle, { id: brief.job_id })}
          </h2>
        </OverlayDialogTitle>
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1">
          {rows.map((r) => (
            <div key={r.key} className="contents">
              <dt className={cx(TYPE.meta, WEIGHT.semibold)}>{r.label}</dt>
              <dd
                data-testid={`job-dialog-${r.key}`}
                className={cx(TYPE.body, TNUM, 'break-words')}
              >
                {r.value}
              </dd>
            </div>
          ))}
        </dl>
        <div className="flex flex-col gap-1">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.log)}</span>
          <pre
            data-testid="job-dialog-log"
            className={cx(
              'max-h-72 overflow-auto whitespace-pre-wrap break-words border border-lz-border p-2 font-mono text-xs',
              SURFACE.card
            )}
          >
            {log || intl.formatMessage(i18n.logEmpty)}
          </pre>
        </div>
        {error && (
          <p
            data-testid="job-dialog-error"
            className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
          >
            {error}
          </p>
        )}
      </div>
      <div className="flex flex-wrap justify-end gap-2">
        {!isTerminal(shown.state) && shown.state !== 'cancelling' && (
          <Button
            size="sm"
            variant="destructive"
            data-testid="job-dialog-cancel"
            disabled={cancelling}
            onClick={() => void cancel()}
            icon={cancelling ? <Loader2 className="animate-spin" /> : <Square />}
          >
            {intl.formatMessage(i18n.cancelJob)}
          </Button>
        )}
        <Button size="sm" variant="secondary" icon={<X />} onClick={onClose}>
          {intl.formatMessage(i18n.close)}
        </Button>
      </div>
      {dialog}
    </OverlayDialog>
  );
}

interface JobsSettingsState {
  allow: boolean;
  policy: UserActivePolicy;
  idleS: string;
  minFree: string;
}

/** This Mac's jobs tunables, each a goose config key the jobs side reads (link.rs `jobs_config`). */
export function JobsSettings() {
  const intl = useIntl();
  const [state, setState] = useState<JobsSettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [allow, policy, idleS, minFree] = await Promise.all([
          acpReadConfig(JOBS_KEYS.allow),
          acpReadConfig(JOBS_KEYS.policy),
          acpReadConfig(JOBS_KEYS.idleS),
          acpReadConfig(JOBS_KEYS.minFree),
        ]);
        if (!alive) return;
        setState({
          allow: allow == null ? JOBS_DEFAULTS.allow : parseBool(allow),
          policy: parsePolicy(policy),
          idleS: String(parseNumber(idleS) ?? JOBS_DEFAULTS.idleS),
          minFree: String(parseNumber(minFree) ?? JOBS_DEFAULTS.minFree),
        });
      } catch (e) {
        if (alive) setError(jobErrorText(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const save = async (key: string, value: unknown, patch: Partial<JobsSettingsState>) => {
    setSaving(true);
    setError(null);
    setState((s) => (s ? { ...s, ...patch } : s));
    try {
      await acpUpsertConfig(key, value);
    } catch (e) {
      setError(intl.formatMessage(i18n.saveFailed, { reason: jobErrorText(e) }));
    } finally {
      setSaving(false);
    }
  };

  const saveNumber = (key: string, field: 'idleS' | 'minFree', text: string, max: number) => {
    const n = parseNumber(text);
    if (n == null || n < 0 || n > max) return;
    void save(key, n, { [field]: String(n) });
  };

  return (
    <div data-testid="jobs-settings" className="flex flex-col gap-2">
      <span className={cx(TYPE.body, WEIGHT.semibold)}>
        {intl.formatMessage(i18n.settingsTitle)}
      </span>
      {state && (
        <>
          <span className="flex items-center gap-2">
            <StudioSwitch
              checked={state.allow}
              disabled={saving}
              aria-label={intl.formatMessage(i18n.allowJobs)}
              onChange={(on) => void save(JOBS_KEYS.allow, on, { allow: on })}
            />
            <span className={TYPE.body}>{intl.formatMessage(i18n.allowJobs)}</span>
          </span>
          <p className={TYPE.meta}>{intl.formatMessage(i18n.allowJobsHint)}</p>
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.policy)}</span>
          <Segmented
            size="sm"
            aria-label={intl.formatMessage(i18n.policy)}
            data-testid="jobs-settings-policy"
            disabled={saving}
            value={state.policy}
            options={USER_ACTIVE_POLICIES.map((p) => ({
              value: p,
              label: intl.formatMessage(i18n[`policy_${p}`]),
              testId: `jobs-policy-${p}`,
            }))}
            onChange={(p) => void save(JOBS_KEYS.policy, p, { policy: p })}
          />
          <label className="flex flex-wrap items-center gap-2">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.idleS)}</span>
            <input
              type="text"
              inputMode="numeric"
              data-testid="jobs-settings-idle"
              className={cx(INPUT, 'w-24 text-right', TNUM)}
              value={state.idleS}
              onChange={(e) => setState({ ...state, idleS: e.target.value })}
              onBlur={(e) => saveNumber(JOBS_KEYS.idleS, 'idleS', e.target.value, 86400)}
            />
          </label>
          <label className="flex flex-wrap items-center gap-2">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.minFree)}</span>
            <input
              type="text"
              inputMode="numeric"
              data-testid="jobs-settings-minfree"
              className={cx(INPUT, 'w-24 text-right', TNUM)}
              value={state.minFree}
              onChange={(e) => setState({ ...state, minFree: e.target.value })}
              onBlur={(e) => saveNumber(JOBS_KEYS.minFree, 'minFree', e.target.value, 90)}
            />
          </label>
          <p className={TYPE.meta}>{intl.formatMessage(i18n.reconnectHint)}</p>
        </>
      )}
      {error && (
        <p
          data-testid="jobs-settings-error"
          className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
        >
          {error}
        </p>
      )}
    </div>
  );
}
