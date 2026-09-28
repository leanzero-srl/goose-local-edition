import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Repeat } from 'lucide-react';
import { useIntl } from '../../i18n';
import type { Message } from '../../types/message';
import { Button, SURFACE, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { loopWords as w, sentenceMessage, sentenceValues } from './loopWords';
import {
  cadenceWords,
  endedTicks,
  hm,
  lastTick,
  revealTickMarker,
  statusChip,
  tickSlices,
  totalTokens,
  totalWallSeconds,
  viewerOffsetMinutes,
} from './loopView';
import {
  durationWords,
  goalFirstLine,
  statusSentence,
  type LoopControlAction,
  type LoopRecord,
  type LoopStatus,
  type LoopStatusReason,
} from './model';
import { requestStartLoop, type StartLoopRequest } from './startLoopRequest';
import { TickRow } from './TickRow';
import type { ControlResult, SessionLoop } from './useSessionLoop';

type Control = (action: LoopControlAction) => Promise<ControlResult>;

/** What the last control or dialog request came back with, said under the controls. */
type Said =
  | { kind: 'refused'; reason: string }
  | { kind: 'failed'; error: string }
  | { kind: 'noDialog' };

/**
 * The rail's Loop tab (§8.4): the loop's header and controls, the NOW block for its status, and the
 * tick ledger, newest first. Every control calls L0's `loops/control`; what goosed answers is shown
 * as it is — a named refusal ("The loop runner is not in this build") is said in words, never a
 * success the runner did not report.
 */
export function LoopPanel({
  sessionId,
  state,
  messages,
  workingDir,
  nowMs,
  control,
}: {
  sessionId: string;
  state: SessionLoop;
  messages: readonly Message[];
  workingDir?: string;
  nowMs: number;
  control: Control;
}) {
  const intl = useIntl();
  const [said, setSaid] = useState<Said | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);

  const act = async (action: LoopControlAction) => {
    setSaid(null);
    const result = await control(action);
    if (result.kind === 'refused') setSaid({ kind: 'refused', reason: result.refusal.reason });
    else if (result.kind === 'failed') setSaid({ kind: 'failed', error: result.error });
  };
  const start = (request: StartLoopRequest) => {
    setSaid(requestStartLoop(request) ? null : { kind: 'noDialog' });
  };

  const saidLine = said && (
    <p data-testid="loop-said" role="alert" className="text-xs font-lz-semibold text-lz-err">
      {said.kind === 'refused'
        ? intl.formatMessage(w.controlRefused, { reason: said.reason })
        : said.kind === 'failed'
          ? intl.formatMessage(w.controlFailed, { error: said.error })
          : intl.formatMessage(w.startDialogAbsent)}
    </p>
  );

  if (state.kind === 'loading') return null;

  if (state.kind === 'none') {
    return (
      <div data-testid="loop-panel-empty" className="flex flex-col items-start gap-2 p-4">
        <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(w.emptyTitle)}</p>
        <p className={TYPE.bodyMuted}>
          {intl.formatMessage(w.emptyBody, { command: '/loop <goal>' })}
        </p>
        <Button
          variant="primary"
          size="sm"
          icon={<Repeat aria-hidden />}
          onClick={() => start({ sessionId, mode: 'start' })}
        >
          {intl.formatMessage(w.startLoop)}
        </Button>
        {saidLine}
      </div>
    );
  }

  if (state.kind === 'unreadable') {
    return (
      <div data-testid="loop-panel-unreadable" className="flex flex-col items-start gap-2 p-4">
        <p data-testid="loop-now" className="text-lz-body text-lz-err">
          {intl.formatMessage(w.unreadable, { error: state.error })}
        </p>
        <Button variant="destructive" size="sm" onClick={() => setConfirmStop(true)}>
          {intl.formatMessage(w.stopLoop)}
        </Button>
        {saidLine}
        <StopDialog
          open={confirmStop}
          running={false}
          after={null}
          onKeep={() => setConfirmStop(false)}
          onStop={() => {
            setConfirmStop(false);
            void act('stop');
          }}
        />
      </div>
    );
  }

  const { loop, status, reason } = state;
  return (
    <LoopBody
      sessionId={sessionId}
      loop={loop}
      status={status}
      reason={reason}
      messages={messages}
      workingDir={workingDir}
      nowMs={nowMs}
      act={act}
      start={start}
      saidLine={saidLine}
      confirmStop={confirmStop}
      setConfirmStop={setConfirmStop}
    />
  );
}

function LoopBody({
  sessionId,
  loop,
  status,
  reason,
  messages,
  workingDir,
  nowMs,
  act,
  start,
  saidLine,
  confirmStop,
  setConfirmStop,
}: {
  sessionId: string;
  loop: LoopRecord;
  status: LoopStatus;
  reason?: LoopStatusReason | null;
  messages: readonly Message[];
  workingDir?: string;
  nowMs: number;
  act: (action: LoopControlAction) => Promise<void>;
  start: (request: StartLoopRequest) => void;
  saidLine: ReactNode;
  confirmStop: boolean;
  setConfirmStop: (open: boolean) => void;
}) {
  const intl = useIntl();
  const chip = statusChip(status);
  const last = lastTick(loop);
  const running = status === 'running' || status === 'checking';
  const ended = status === 'ended';
  const slices = useMemo(() => tickSlices(messages, loop), [messages, loop]);
  const ledger = useMemo(() => {
    const done = endedTicks(loop).map((tick) => tick.n);
    return slices.filter((slice) => done.includes(slice.tick.n)).reverse();
  }, [slices, loop]);

  const cadence = cadenceWords(loop.cadence);
  const lineTwo: string[] = [intl.formatMessage(cadence.message, cadence.values)];
  if (last) {
    lineTwo.push(
      loop.stopAfterTicks != null
        ? intl.formatMessage(w.headerTickOf, { n: last.n, k: loop.stopAfterTicks })
        : intl.formatMessage(w.headerTick, { n: last.n })
    );
  }
  const since = hm(loop.startedAt);
  if (since) lineTwo.push(intl.formatMessage(w.headerSince, { time: since }));
  const wall = totalWallSeconds(loop, nowMs);
  if (wall !== null) lineTwo.push(durationWords(wall));
  const tokens = totalTokens(loop);
  if (tokens !== null) {
    lineTwo.push(
      intl.formatMessage(w.headerTokens, {
        tokens: intl.formatNumber(tokens, { notation: 'compact' }),
      })
    );
  }

  const pause = () => void act('pause');
  const resume = () => void act('resume');
  const tickNow = () => void act('tickNow');
  const edit = () => start({ sessionId, mode: 'edit', from: loop });

  return (
    <div data-testid="loop-panel" className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <section className={cx('flex flex-col gap-1.5 border-b px-3 py-3', SURFACE.hairline)}>
        <div className="flex min-w-0 items-start gap-2">
          <p
            data-testid="loop-goal"
            title={loop.goal}
            className={cx(TYPE.body, WEIGHT.semibold, 'min-w-0 flex-1 truncate')}
          >
            {goalFirstLine(loop.goal)}
          </p>
          <span
            data-testid="loop-status-chip"
            data-tone={chip.tone}
            className={cx(
              'inline-flex h-6 shrink-0 items-center rounded-lz-pill px-2.5 text-xs font-lz-semibold',
              TONE_FILL[chip.tone]
            )}
          >
            {intl.formatMessage(chip.label)}
          </span>
        </div>
        <p data-testid="loop-line-two" className={cx('text-xs text-lz-ink-2', TNUM)}>
          {lineTwo.join(' · ')}
        </p>
        <p
          data-testid="loop-check-line"
          className="truncate text-xs text-lz-ink-2"
          title={loop.check ?? undefined}
        >
          {loop.check
            ? intl.formatMessage(w.headerCheck, { command: loop.check })
            : intl.formatMessage(w.headerNoCheck)}
        </p>
        <StateFileLine
          path={loop.stateFile}
          workingDir={workingDir}
          ticksEnded={endedTicks(loop).length}
        />
        {!ended && (
          <div data-testid="loop-controls" className="mt-1 flex flex-wrap gap-1.5">
            <Button
              size="sm"
              disabled={running}
              title={running ? intl.formatMessage(w.tickAlreadyRunning) : undefined}
              onClick={tickNow}
            >
              {intl.formatMessage(w.tickNow)}
            </Button>
            {status === 'paused' ? (
              <Button size="sm" onClick={resume}>
                {intl.formatMessage(w.resume)}
              </Button>
            ) : (
              <Button size="sm" onClick={pause}>
                {intl.formatMessage(w.pause)}
              </Button>
            )}
            <Button size="sm" variant="destructive" onClick={() => setConfirmStop(true)}>
              {intl.formatMessage(w.stopLoop)}
            </Button>
            <Button size="sm" variant="ghost" onClick={edit}>
              {intl.formatMessage(w.edit)}
            </Button>
          </div>
        )}
        {saidLine}
      </section>

      <section className={cx('flex flex-col gap-2 border-b px-3 py-3', SURFACE.hairline)}>
        <h3 className={TYPE.zone}>{intl.formatMessage(w.nowLabel)}</h3>
        <NowBlock
          loop={loop}
          status={status}
          reason={reason}
          nowMs={nowMs}
          act={act}
          onStop={() => setConfirmStop(true)}
          onEdit={edit}
          onStartNew={() => start({ sessionId, mode: 'start', from: loop })}
        />
      </section>

      <section className="flex flex-col">
        <h3 className={cx(TYPE.zone, 'px-3 pt-3 pb-1')}>{intl.formatMessage(w.ticksLabel)}</h3>
        {ledger.length === 0 ? (
          <p className="px-3 pb-3 text-xs text-lz-ink-2">{intl.formatMessage(w.noTicks)}</p>
        ) : (
          <ul data-testid="loop-ticks">
            {ledger.map((slice) => (
              <TickRow
                key={slice.tick.n}
                slice={slice}
                prev={(loop.ticks ?? []).find((tick) => tick.n === slice.tick.n - 1)}
                record={loop}
                nowMs={nowMs}
              />
            ))}
          </ul>
        )}
      </section>

      <StopDialog
        open={confirmStop}
        running={running}
        after={last?.n ?? null}
        onKeep={() => setConfirmStop(false)}
        onStop={() => {
          setConfirmStop(false);
          void act('stop');
        }}
      />
    </div>
  );
}

/** The NOW block: the model's status sentence for this status, and its own actions (§8.4). */
function NowBlock({
  loop,
  status,
  reason,
  nowMs,
  act,
  onStop,
  onEdit,
  onStartNew,
}: {
  loop: LoopRecord;
  status: LoopStatus;
  reason?: LoopStatusReason | null;
  nowMs: number;
  act: (action: LoopControlAction) => Promise<void>;
  onStop: () => void;
  onEdit: () => void;
  onStartNew: () => void;
}) {
  const intl = useIntl();
  const sentence = statusSentence(loop, status, reason, nowMs, viewerOffsetMinutes(nowMs));
  let text: string;
  if (!sentence.ok) {
    text = intl.formatMessage(w.statusUnreadable, { error: sentence.error });
  } else {
    const message = sentenceMessage(sentence.value);
    text = message
      ? intl.formatMessage(message, sentenceValues(sentence.value))
      : sentence.value.text;
  }
  const last = lastTick(loop);
  const buttons: ReactNode[] = [];
  const add = (key: string, label: string, onClick: () => void, primary = false) =>
    buttons.push(
      <Button key={key} size="sm" variant={primary ? 'primary' : 'secondary'} onClick={onClick}>
        {label}
      </Button>
    );

  switch (status) {
    case 'running':
      if (last) {
        add('show', intl.formatMessage(w.showInChat), () => revealTickMarker(last.firstMessageId));
      }
      break;
    case 'checking':
      add('stopCheck', intl.formatMessage(w.stopCheck), () => void act('stopCheck'));
      break;
    case 'waiting_you':
      add('next', intl.formatMessage(w.runNextTick), () => void act('tickNow'), true);
      add('pause', intl.formatMessage(w.pause), () => void act('pause'));
      break;
    case 'needs_you':
      if (reason?.kind === 'asked') {
        add('question', intl.formatMessage(w.goToQuestion), () =>
          document
            .querySelector('[data-testid="needs-you-card"]')
            ?.scrollIntoView({ block: 'center', behavior: 'smooth' })
        );
      }
      break;
    case 'paused':
      switch (reason?.kind) {
        case 'closed':
          add('resume', intl.formatMessage(w.resumeOneTick), () => void act('resume'), true);
          break;
        case 'you_stopped_tick':
          add('resume', intl.formatMessage(w.resume), () => void act('resume'), true);
          add('stop', intl.formatMessage(w.stopLoop), onStop);
          break;
        case 'check_could_not_run':
          add('edit', intl.formatMessage(w.edit), onEdit, true);
          add('resume', intl.formatMessage(w.resume), () => void act('resume'));
          break;
        case 'by_you':
        case 'finishing_elsewhere':
          add('resume', intl.formatMessage(w.resume), () => void act('resume'), true);
          break;
        default:
          add('resume', intl.formatMessage(w.resume), () => void act('resume'), true);
          add('edit', intl.formatMessage(w.edit), onEdit);
      }
      break;
    case 'ended':
      add('new', intl.formatMessage(w.startNewLoop), onStartNew, true);
      break;
    case 'elsewhere':
      add('pause', intl.formatMessage(w.pause), () => void act('pause'));
      add('stop', intl.formatMessage(w.stopLoop), onStop);
      break;
  }

  return (
    <div className="flex flex-col gap-2">
      <p
        data-testid="loop-now"
        className={cx(
          'whitespace-pre-wrap text-lz-body',
          sentence.ok ? 'text-lz-ink' : 'text-lz-err'
        )}
      >
        {text}
      </p>
      {buttons.length > 0 && <div className="flex flex-wrap gap-1.5">{buttons}</div>}
    </div>
  );
}

type StateFileRead =
  | { kind: 'unknown' }
  | { kind: 'written' }
  | { kind: 'missing' }
  | { kind: 'unreadable'; error: string };

function absoluteStateFile(path: string, workingDir?: string): string | null {
  if (path.startsWith('/')) return path;
  if (!workingDir) return null;
  return `${workingDir.replace(/\/+$/, '')}/${path}`;
}

/**
 * "State file: {path} [Open]", or "· not written yet" — a read of the file on disk, never inferred
 * from the Changes (a shell command that wrote it leaves no diff, §4.5).
 */
function StateFileLine({
  path,
  workingDir,
  ticksEnded,
}: {
  path: string;
  workingDir?: string;
  ticksEnded: number;
}) {
  const intl = useIntl();
  const absolute = absoluteStateFile(path, workingDir);
  const [read, setRead] = useState<StateFileRead>({ kind: 'unknown' });

  useEffect(() => {
    let live = true;
    const readFile = window.electron?.readFile;
    if (!absolute || !readFile) {
      setRead({ kind: 'unknown' });
      return undefined;
    }
    readFile(absolute)
      .then((got) => {
        if (!live) return;
        if (got.found) setRead({ kind: 'written' });
        else if (!got.error || /no such file/i.test(String(got.error)))
          setRead({ kind: 'missing' });
        else setRead({ kind: 'unreadable', error: String(got.error).trim() });
      })
      .catch((error: unknown) => {
        if (live) setRead({ kind: 'unreadable', error: String(error) });
      });
    return () => {
      live = false;
    };
  }, [absolute, ticksEnded]);

  return (
    <div
      data-testid="loop-state-file"
      className="flex min-w-0 items-center gap-2 text-xs text-lz-ink-2"
    >
      <span className="min-w-0 truncate" title={absolute ?? path}>
        {intl.formatMessage(w.headerStateFile, { path })}
        {read.kind === 'missing' && ` · ${intl.formatMessage(w.headerStateFileNotWritten)}`}
        {read.kind === 'unreadable' &&
          ` · ${intl.formatMessage(w.headerStateFileUnreadable, { error: read.error })}`}
      </span>
      {read.kind === 'written' && absolute && (
        <button
          type="button"
          onClick={() => void window.electron.revealInFinder(absolute)}
          className="inline-flex h-6 shrink-0 items-center rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-xs font-lz-medium text-lz-ink hover:bg-lz-surface-2"
        >
          {intl.formatMessage(w.open)}
        </button>
      )}
    </div>
  );
}

function StopDialog({
  open,
  running,
  after,
  onKeep,
  onStop,
}: {
  open: boolean;
  running: boolean;
  after: number | null;
  onKeep: () => void;
  onStop: () => void;
}) {
  const intl = useIntl();
  const body =
    after === null
      ? intl.formatMessage(w.stopBeforeFirst)
      : running
        ? intl.formatMessage(w.stopRunning, { n: after })
        : intl.formatMessage(w.stopIdle, { n: after });
  return (
    <OverlayDialog
      open={open}
      onClose={onKeep}
      panelClassName={cx('flex w-[26rem] flex-col gap-3 p-5', SURFACE.overlay)}
    >
      <div data-testid="loop-stop-dialog" className="flex flex-col gap-3">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>{intl.formatMessage(w.stopTitle)}</h2>
        </OverlayDialogTitle>
        <p className={TYPE.body}>{body}</p>
        <div className="flex justify-end gap-2">
          <Button onClick={onKeep}>{intl.formatMessage(w.stopKeep)}</Button>
          <Button variant="destructive" onClick={onStop}>
            {intl.formatMessage(w.stopLoop)}
          </Button>
        </div>
      </div>
    </OverlayDialog>
  );
}
