import { useEffect, useState } from 'react';
import { Loader2, Play, X } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Combobox, Segmented, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import {
  jobErrorText,
  leanzeroLinkJobsPlace,
  leanzeroLinkJobsSubmit,
  type JobBrief,
  type JobKind,
  type JobPlan,
  type JobRequest,
  type PlacementStrategy,
} from '../../acp/leanzero-link';
import { INPUT, StudioSwitch, TEXTAREA } from './studio';
import { macNameOfNode, type Mac } from './macs';
import { JobDialog, useStrategyWords } from './MacLoadPanel';
import { STRATEGIES } from './macLoad';

const i18n = defineMessages({
  title: { id: 'runJob.title', defaultMessage: 'Run a job on…' },
  target: { id: 'runJob.target', defaultMessage: 'Run on' },
  best: { id: 'runJob.best', defaultMessage: 'Best Mac' },
  ranking: { id: 'runJob.ranking', defaultMessage: 'Best now: {macs}' },
  inUse: { id: 'runJob.inUse', defaultMessage: '{name} (in use)' },
  noPlace: {
    id: 'runJob.noPlace',
    defaultMessage: 'No Mac can take this job right now.',
  },
  willQueue: {
    id: 'runJob.willQueue',
    defaultMessage:
      'No Mac has room right now. Run it and it waits in line on this Mac, then starts on the first Mac that has room.',
  },
  refusal: { id: 'runJob.refusal', defaultMessage: '{mac}: {reason}' },
  none: {
    id: 'runJob.none',
    defaultMessage:
      'None of your Macs takes jobs now. Turn on “Run jobs other Macs send” on the Mac that should run them.',
  },
  kind: { id: 'runJob.kind', defaultMessage: 'Kind' },
  kindShell: { id: 'runJob.kindShell', defaultMessage: 'Shell command' },
  kindPrompt: { id: 'runJob.kindPrompt', defaultMessage: 'Prompt' },
  command: {
    id: 'runJob.command',
    defaultMessage: '{kind, select, shell {Command} other {Prompt}}',
  },
  heavy: { id: 'runJob.heavy', defaultMessage: 'Heavy job' },
  heavyHint: {
    id: 'runJob.heavyHint',
    defaultMessage: 'A Mac runs one heavy job at a time (builds, renders, training).',
  },
  mem: { id: 'runJob.mem', defaultMessage: 'Memory it needs (GB, optional)' },
  timeout: { id: 'runJob.timeout', defaultMessage: 'Time limit (seconds, optional)' },
  submit: { id: 'runJob.submit', defaultMessage: 'Run job' },
  close: { id: 'runJob.close', defaultMessage: 'Close' },
  refused: { id: 'runJob.refused', defaultMessage: 'Not sent: {reason}' },
});

const BEST = '__best__';

/** The Macs a job can be sent to by name: online, and taking jobs as they report it now. */
export function jobTargets(macs: readonly Mac[]): Mac[] {
  return macs.filter((m) => m.online && m.nodeId != null && m.allows?.run_jobs === true);
}

/** A blank field is "not given"; anything else must be a positive number (an integer if `int`). */
function parseOptional(text: string, int: boolean): number | undefined | 'bad' {
  const t = text.trim();
  if (t === '') return undefined;
  const n = Number(t);
  if (!Number.isFinite(n) || n <= 0 || (int && !Number.isInteger(n))) return 'bad';
  return n;
}

interface Sent {
  nodeId: string;
  brief: JobBrief;
}

/**
 * Send one job to a Mac on the Link mesh: to a Mac picked by name, or to the best Mac now
 * (`targetNodeId` omitted, goosed places it on `jobsPlace`'s first). On success the job's own
 * dialog opens in its place; a refusal shows the target's reason and keeps the form.
 */
export function RunJobDialog({ macs, onClose }: { macs: readonly Mac[]; onClose: () => void }) {
  const intl = useIntl();
  const targets = jobTargets(macs);
  const self = macs.find((m) => m.isSelf);
  const [target, setTarget] = useState(BEST);
  const [kind, setKind] = useState<JobKind>('shell');
  const [command, setCommand] = useState('');
  const [heavy, setHeavy] = useState(false);
  const [mem, setMem] = useState('');
  const [timeout, setTimeoutText] = useState('');
  const strategyWords = useStrategyWords();
  // null = not picked here: the setting decides, and jobsPlace answers which one that is.
  const [picked, setPicked] = useState<PlacementStrategy | null>(null);
  const [plan, setPlan] = useState<JobPlan | null>(null);
  const [placeError, setPlaceError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sent, setSent] = useState<Sent | null>(null);

  const memGb = parseOptional(mem, false);
  const timeoutS = parseOptional(timeout, true);
  const placeMem = typeof memGb === 'number' ? memGb : undefined;

  useEffect(() => {
    let alive = true;
    leanzeroLinkJobsPlace({
      heavy,
      ...(placeMem != null ? { needMemGb: placeMem } : {}),
      ...(picked ? { strategy: picked } : {}),
    })
      .then((p) => {
        if (!alive) return;
        setPlan(p);
        setPlaceError(null);
      })
      .catch((e: unknown) => {
        if (alive) setPlaceError(jobErrorText(e));
      });
    return () => {
      alive = false;
    };
  }, [heavy, placeMem, picked]);
  const placements = plan?.placements ?? null;
  const strategy: PlacementStrategy = picked ?? plan?.strategy ?? 'local-first';

  if (sent) {
    return (
      <JobDialog
        brief={sent.brief}
        target={sent.nodeId}
        macName={macNameOfNode(macs, sent.nodeId)}
        onClose={onClose}
      />
    );
  }

  const nameOf = (id: string) => macNameOfNode(macs, id);
  const ranking = placements?.map((p) =>
    p.userActive ? intl.formatMessage(i18n.inUse, { name: nameOf(p.nodeId) }) : nameOf(p.nodeId)
  );
  const canSend = command.trim() !== '' && memGb !== 'bad' && timeoutS !== 'bad' && !sending;

  const submit = async () => {
    if (!canSend) return;
    const job: JobRequest = { kind, command, heavy };
    if (typeof memGb === 'number') job.need_mem_gb = memGb;
    if (typeof timeoutS === 'number') job.timeout_s = timeoutS;
    setSending(true);
    setError(null);
    try {
      const r =
        target === BEST
          ? await leanzeroLinkJobsSubmit(job, undefined, strategy)
          : await leanzeroLinkJobsSubmit(job, target);
      setSent({
        nodeId: r.nodeId,
        brief: {
          job_id: r.jobId,
          origin_node: self?.nodeId ?? '',
          kind,
          started_at: '—',
          state: 'queued',
          heavy,
          ...(r.queued
            ? { queue: { position: r.queuePosition ?? 1, strategy, refusals: [] } }
            : {}),
        },
      });
    } catch (e) {
      setError(intl.formatMessage(i18n.refused, { reason: jobErrorText(e) }));
    } finally {
      setSending(false);
    }
  };

  const bestHint = placements?.[0] ? nameOf(placements[0].nodeId) : undefined;
  const options = [
    { value: BEST, label: intl.formatMessage(i18n.best), hint: bestHint },
    ...targets.map((m) => ({ value: m.nodeId as string, label: m.name })),
  ];

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx('flex w-[40rem] max-w-[90vw] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div data-testid="run-job-dialog" className="flex flex-col gap-3">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>{intl.formatMessage(i18n.title)}</h2>
        </OverlayDialogTitle>
        {targets.length === 0 && (placements?.length ?? 0) === 0 && (
          <p
            data-testid="run-job-none"
            className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.warn, 'break-words')}
          >
            {intl.formatMessage(i18n.none)}
          </p>
        )}
        <div className="flex flex-col gap-1">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.target)}</span>
          <Combobox
            aria-label={intl.formatMessage(i18n.target)}
            options={options}
            value={target}
            onChange={setTarget}
          />
          {target === BEST && placements && (
            <span data-testid="run-job-ranking" className={cx(TYPE.meta, TNUM, 'break-words')}>
              {ranking && ranking.length > 0
                ? intl.formatMessage(i18n.ranking, { macs: ranking.join(' · ') })
                : intl.formatMessage(i18n.noPlace)}
            </span>
          )}
          {target === BEST && plan && plan.placements.length === 0 && plan.refusals.length > 0 && (
            <div data-testid="run-job-refusals" className="flex flex-col gap-0.5">
              <span className={cx(TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn, 'break-words')}>
                {intl.formatMessage(i18n.willQueue)}
              </span>
              <ul className="flex flex-col gap-0.5">
                {plan.refusals.map((r) => (
                  <li key={r.nodeId} className={cx(TYPE.meta, 'break-words')}>
                    {intl.formatMessage(i18n.refusal, { mac: nameOf(r.nodeId), reason: r.reason })}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {target === BEST && placeError && (
            <span className={cx(TYPE.meta, TONE_TEXT.err, 'break-words')}>{placeError}</span>
          )}
        </div>
        {target === BEST && (
          <div className="flex flex-col gap-1">
            <span className={cx(TYPE.meta, WEIGHT.semibold)}>{strategyWords.title}</span>
            <Segmented
              size="sm"
              aria-label={strategyWords.title}
              value={strategy}
              options={STRATEGIES.map((st) => ({
                value: st,
                label: strategyWords.label(st),
                title: strategyWords.hint(st),
                testId: `run-job-strategy-${st}`,
              }))}
              onChange={setPicked}
            />
            <span data-testid="run-job-strategy-hint" className={cx(TYPE.meta, 'break-words')}>
              {strategyWords.hint(strategy)}
            </span>
          </div>
        )}
        <div className="flex flex-col gap-1">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(i18n.kind)}</span>
          <Segmented
            size="sm"
            aria-label={intl.formatMessage(i18n.kind)}
            value={kind}
            options={[
              {
                value: 'shell' as const,
                label: intl.formatMessage(i18n.kindShell),
                testId: 'run-job-kind-shell',
              },
              {
                value: 'prompt' as const,
                label: intl.formatMessage(i18n.kindPrompt),
                testId: 'run-job-kind-prompt',
              },
            ]}
            onChange={setKind}
          />
        </div>
        <label className="flex flex-col gap-1">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.command, { kind })}
          </span>
          <textarea
            data-testid="run-job-command"
            className={TEXTAREA}
            value={command}
            onChange={(e) => setCommand(e.target.value)}
          />
        </label>
        <span className="flex items-center gap-2">
          <StudioSwitch
            checked={heavy}
            aria-label={intl.formatMessage(i18n.heavy)}
            onChange={setHeavy}
          />
          <span className={TYPE.body}>{intl.formatMessage(i18n.heavy)}</span>
        </span>
        <p className={TYPE.meta}>{intl.formatMessage(i18n.heavyHint)}</p>
        <div className="flex flex-wrap gap-4">
          <label className="flex flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.mem)}</span>
            <input
              type="text"
              inputMode="decimal"
              data-testid="run-job-mem"
              aria-invalid={memGb === 'bad'}
              className={cx(INPUT, 'w-28 text-right', TNUM)}
              value={mem}
              onChange={(e) => setMem(e.target.value)}
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.timeout)}</span>
            <input
              type="text"
              inputMode="numeric"
              data-testid="run-job-timeout"
              aria-invalid={timeoutS === 'bad'}
              className={cx(INPUT, 'w-28 text-right', TNUM)}
              value={timeout}
              onChange={(e) => setTimeoutText(e.target.value)}
            />
          </label>
        </div>
        {error && (
          <p
            data-testid="run-job-error"
            className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err, 'break-words')}
          >
            {error}
          </p>
        )}
      </div>
      <div className="flex flex-wrap justify-end gap-2">
        <Button size="sm" variant="secondary" icon={<X />} onClick={onClose}>
          {intl.formatMessage(i18n.close)}
        </Button>
        <Button
          size="sm"
          variant="primary"
          data-testid="run-job-submit"
          disabled={!canSend}
          onClick={() => void submit()}
          icon={sending ? <Loader2 className="animate-spin" /> : <Play />}
        >
          {intl.formatMessage(i18n.submit)}
        </Button>
      </div>
    </OverlayDialog>
  );
}
