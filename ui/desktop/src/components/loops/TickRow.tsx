import { useId, useMemo, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { useIntl } from '../../i18n';
import { FOCUS, MOTION, SURFACE, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { ChangedFileList } from '../changes/ChangesRail';
import { DiffCounts } from '../changes/DiffView';
import { sessionChanges } from '../changes/fileDiff';
import { loopWords as w } from './loopWords';
import {
  answerIn,
  chipOf,
  firstLine,
  hm,
  isQuietTick,
  lastAssistantLine,
  revealTickMarker,
  tickChipKind,
  tickDurationSeconds,
  type TickSlice,
} from './loopView';
import { durationWords, type LoopCheckRun, type LoopRecord, type LoopTickRecord } from './model';

const LINK = cx(
  'inline-flex h-6 items-center rounded-lz-control border border-lz-border-strong bg-lz-surface px-2 text-xs font-lz-medium text-lz-ink hover:bg-lz-surface-2',
  FOCUS,
  MOTION
);

/**
 * One tick of the ledger (§8.4 "Tick row"). Collapsed: number, time and duration, the outcome chip
 * and the first line of what it did. Expanded: the whole summary, the files goose wrote or edited
 * IN THIS TICK — `sessionChanges` over the tick's own messages, the Changes tab's derivation and
 * rows — the check, the next step, and a way back to the tick in the chat.
 */
export function TickRow({
  slice,
  prev,
  record,
  nowMs,
}: {
  slice: TickSlice;
  prev: LoopTickRecord | undefined;
  record: LoopRecord;
  nowMs: number;
}) {
  const intl = useIntl();
  const { tick, messages } = slice;
  const [open, setOpen] = useState(false);
  const bodyId = useId();
  const kind = tickChipKind(tick, record);
  const chip = kind ? chipOf(kind) : null;
  const quiet = isQuietTick(tick, prev, record);
  const time = hm(tick.startedAt) ?? '';
  const seconds = tickDurationSeconds(tick, nowMs);
  const when =
    seconds === null
      ? time
      : intl.formatMessage(w.tickWhen, { time, duration: durationWords(seconds) });
  const changes = useMemo(() => (messages ? sessionChanges(messages) : null), [messages]);
  const headline = quiet
    ? intl.formatMessage(w.tickQuiet, { time, nextStep: tick.report?.nextStep ?? '' })
    : collapsedLine(tick, intl.formatMessage);

  return (
    <li
      data-testid="loop-tick-row"
      data-tick={tick.n}
      data-quiet={quiet || undefined}
      className={cx('border-b last:border-b-0', SURFACE.hairline)}
    >
      <button
        type="button"
        aria-expanded={open}
        aria-controls={bodyId}
        aria-label={intl.formatMessage(open ? w.tickCollapse : w.tickExpand, { n: tick.n })}
        onClick={() => setOpen(!open)}
        className={cx(
          'flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left hover:bg-lz-surface-2',
          FOCUS,
          MOTION
        )}
      >
        <span
          className={cx('w-6 shrink-0 text-right text-sm', WEIGHT.semibold, TNUM, 'text-lz-ink')}
        >
          {tick.n}
        </span>
        {quiet ? (
          <span
            data-testid="loop-tick-quiet"
            className="min-w-0 flex-1 truncate text-xs text-lz-ink-2"
          >
            {headline}
          </span>
        ) : (
          <>
            <span className={cx('shrink-0 text-xs text-lz-ink-2 max-[560px]:hidden', TNUM)}>
              {when}
            </span>
            {chip && (
              <span
                data-testid="loop-tick-chip"
                data-tone={chip.tone}
                className={cx(
                  'inline-flex h-5 shrink-0 items-center rounded-lz-pill px-2 text-[11px] font-lz-semibold',
                  TONE_FILL[chip.tone]
                )}
              >
                {intl.formatMessage(chip.label)}
              </span>
            )}
            <span className="min-w-0 flex-1 truncate text-xs text-lz-ink">{headline}</span>
          </>
        )}
        <ChevronRight
          aria-hidden
          className={cx('ml-auto size-4 shrink-0 text-lz-ink-2', open && 'rotate-90')}
        />
      </button>
      {open && (
        <div
          id={bodyId}
          data-testid="loop-tick-body"
          className="flex flex-col gap-2 px-3 pb-3 pl-11"
        >
          {tick.report?.summary && (
            <p className={cx(TYPE.body, 'whitespace-pre-wrap')}>{tick.report.summary}</p>
          )}
          <OutcomeLines tick={tick} messages={messages} />
          {messages === null ? (
            <p data-testid="loop-tick-removed" className="text-xs text-lz-ink-2">
              {intl.formatMessage(w.tickRemoved)}
            </p>
          ) : changes && changes.files.length > 0 ? (
            <div
              data-testid="loop-tick-files"
              className={cx('overflow-hidden rounded-lz-control border', SURFACE.hairline)}
            >
              <div
                className={cx(
                  'flex items-center gap-2 border-b bg-lz-surface-2 px-3 py-1.5',
                  SURFACE.hairline
                )}
              >
                <span className="text-xs font-lz-semibold text-lz-ink">
                  {intl.formatMessage(w.tickFiles)}
                </span>
                <DiffCounts added={changes.added} removed={changes.removed} />
              </div>
              <ChangedFileList files={changes.files} />
            </div>
          ) : (
            <p className="text-xs text-lz-ink-2">{intl.formatMessage(w.tickNoFiles)}</p>
          )}
          {tick.check && <CheckLine check={tick.check} />}
          {tick.report?.nextStep && (
            <p className="text-xs text-lz-ink">
              {intl.formatMessage(w.tickNext, { step: tick.report.nextStep })}
            </p>
          )}
          {tick.report?.nextIn && (
            <p className="text-xs text-lz-ink-2">
              {tick.report.nextReason
                ? intl.formatMessage(w.tickSelfPaced, {
                    interval: tick.report.nextIn,
                    reason: tick.report.nextReason,
                  })
                : intl.formatMessage(w.tickSelfPacedNoReason, { interval: tick.report.nextIn })}
            </p>
          )}
          <MetaLine tick={tick} />
          {messages !== null && (
            <div>
              <button
                type="button"
                data-testid="loop-tick-show-in-chat"
                onClick={() => revealTickMarker(tick.firstMessageId)}
                className={LINK}
              >
                {intl.formatMessage(w.showInChat)}
              </button>
            </div>
          )}
        </div>
      )}
    </li>
  );
}

type Format = ReturnType<typeof useIntl>['formatMessage'];

/** The collapsed row's words: the report's first line, else what the outcome itself says. */
function collapsedLine(tick: LoopTickRecord, format: Format): string {
  const outcome = tick.outcome;
  switch (outcome?.kind) {
    case 'yielded':
      return format(w.tickYielded, { chat: outcome.toChat });
    case 'failed':
      return format(w.tickFailed, { error: firstLine(outcome.error) });
    case 'no_report':
      return format(w.tickNoReport);
    case 'asked':
      return format(w.tickAsked, { question: outcome.question });
  }
  if (tick.report?.summary) return firstLine(tick.report.summary);
  if (outcome?.kind === 'stopped_by_you') return format(w.tickStoppedByYou);
  return '';
}

function OutcomeLines({
  tick,
  messages,
}: {
  tick: LoopTickRecord;
  messages: TickSlice['messages'];
}) {
  const intl = useIntl();
  const outcome = tick.outcome;
  if (!outcome) return null;
  const line = (testId: string, text: string) => (
    <p data-testid={testId} className="text-xs text-lz-ink">
      {text}
    </p>
  );
  switch (outcome.kind) {
    case 'yielded':
      return line('loop-tick-yielded', intl.formatMessage(w.tickYielded, { chat: outcome.toChat }));
    case 'failed':
      return line(
        'loop-tick-failed',
        intl.formatMessage(w.tickFailed, { error: firstLine(outcome.error) })
      );
    case 'no_report': {
      const last = messages ? lastAssistantLine(messages) : null;
      return (
        <>
          {line('loop-tick-no-report', intl.formatMessage(w.tickNoReport))}
          {last &&
            line('loop-tick-last-words', intl.formatMessage(w.tickLastWords, { line: last }))}
        </>
      );
    }
    case 'asked': {
      const answer = messages ? answerIn(messages) : null;
      return (
        <>
          {line('loop-tick-asked', intl.formatMessage(w.tickAsked, { question: outcome.question }))}
          {answer && line('loop-tick-answered', intl.formatMessage(w.tickAnswered, { answer }))}
        </>
      );
    }
    case 'stopped_by_you':
      return line('loop-tick-stopped', intl.formatMessage(w.tickStoppedByYou));
    default:
      return null;
  }
}

function lastTailLine(check: LoopCheckRun): string | null {
  const lines = (check.outputTail ?? '').split('\n').filter((l) => l.trim());
  return lines.length ? lines[lines.length - 1].trim() : null;
}

function CheckLine({ check }: { check: LoopCheckRun }) {
  const intl = useIntl();
  const command = check.command;
  let text: string;
  if (!check.endedAt) text = intl.formatMessage(w.tickCheckRunning, { command });
  else if (!check.ran) {
    text = intl.formatMessage(w.tickCheckCouldNotRun, {
      command,
      error: check.error ?? '',
    });
  } else if (check.exit === 0) text = intl.formatMessage(w.tickCheckPassed, { command });
  else if (check.exit == null) text = intl.formatMessage(w.tickCheckNoExit, { command });
  else {
    const tail = lastTailLine(check);
    text = tail
      ? intl.formatMessage(w.tickCheckExitedTail, { command, code: check.exit, line: tail })
      : intl.formatMessage(w.tickCheckExited, { command, code: check.exit });
  }
  const logPath = check.logPath;
  return (
    <div data-testid="loop-tick-check" className="flex flex-wrap items-center gap-2 text-xs">
      <span className="text-lz-ink">{text}</span>
      {logPath && (
        <button
          type="button"
          onClick={() => void window.electron.revealInFinder(logPath)}
          className={LINK}
        >
          {intl.formatMessage(w.openLog)}
        </button>
      )}
    </div>
  );
}

function MetaLine({ tick }: { tick: LoopTickRecord }) {
  const intl = useIntl();
  const parts: string[] = [];
  if (tick.served) parts.push(intl.formatMessage(w.tickOn, { node: tick.served.node }));
  if (tick.tokens) {
    parts.push(
      intl.formatMessage(w.tickTokens, {
        tokens: intl.formatNumber(tick.tokens.total, { notation: 'compact' }),
      })
    );
  }
  if (parts.length === 0) return null;
  return <p className={cx('text-xs text-lz-ink-3', TNUM)}>{parts.join(' · ')}</p>;
}
