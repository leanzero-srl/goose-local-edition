import React, { useState, useEffect, useMemo, useRef, useId } from 'react';
import { createPortal } from 'react-dom';
import {
  Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench,
  Search, ListChecks, Play, Pause, FlaskConical, RotateCcw, Gavel, Eye, FileText, Cpu,
  MessageCircleQuestion, Send, Gauge, AlertTriangle, FolderOpen, TrendingUp, Info, Braces,
  Circle, Minus, Terminal, FilePlus2, FilePenLine, Hammer,
  MessageSquare, Bot, Bug,
} from 'lucide-react';
import {
  useSwarmRun,
  deriveFleet,
  deriveNodeHistory,
  deriveTaskBoard,
  runAppName,
  classifyCall,
  callRowMeta,
  collapseRepeats,
  firstCallNeedingAttention,
  workCaption,
  workRows,
  elapsedSince,
  substantiveChunk,
  resolveActivityPath,
  supervisionRollingCaption,
  type TurnStatus,
  type TurnLane,
  type LiveChannel,
  type SwarmCall,
  type InflightCall,
  type FormingCall,
  type CallMeaning,
  type ActivityItem,
  type PlanTask,
  type RunSummary,
  type ConfidenceBreakdown,
  type PhaseTodo,
  type PhaseTodoItem,
  type TodoState,
  type BoardRow,
  type TaskBoard,
  type SupervisionSpan,
  type NodeHistoryEntry,
  type SwarmRunState,
  type ClarifyProxy,
  type RunOverview as RunOverviewData,
  cleanTaskTitle,
  isPlanningDigestKey,
  saidKindOf,
  splitTranscriptAttempts,
  type SaidKind,
  type SupersededSaid,
} from './useSwarmRun';
import { ZoneHeader } from './ZoneHeader';
import {
  Button,
  Chip,
  FOCUS,
  MOTION,
  NODE_DOT,
  Panel,
  RADIUS,
  SURFACE,
  Segmented,
  StatusDot,
  TNUM,
  TONE_DOT,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type NodeIndex,
  type Tone,
} from '../lz';
import { SWARM_LOG_MODES, useSwarmLogMode, type SwarmLogMode } from './useVerboseSwarm';
import { useFleetCorroboration } from './useFleetCorroboration';
import { useLmStudioFleetVisible } from '../../hooks/useLmStudioFleetVisible';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';
import InlineMarkdown from './InlineMarkdown';
import StructuredContent, { CodeBlock } from './StructuredContent';
import FormationRibbon, { type FormationActiveTone } from './FormationRibbon';
import { isPlanningPhase, planningLanesFor, type PhaseLaneGroup } from './phaseList';
import {
  CHIP_RADIUS,
  EYEBROW_CLASS,
  SWARM_STATUS,
  nextRevealedText,
  usePrefersReducedMotion,
  usePageVisible,
} from './formationVisualState';
import { engineLiveness, isEngineSilent } from './swarmRunLiveness';

/**
 * Tip — a hover explainer for an icon/glyph, reusing the app's Radix tooltip so every swarm-panel affordance
 * says what it means (the icons were previously unlabelled). `label` is the short explanation; `children` is
 * the single element it wraps. Rendered in a portal, so it is safe inside the lane's toggle button.
 */
const Tip: React.FC<{ label: React.ReactNode; children: React.ReactElement }> = ({ label, children }) => (
  <Tooltip>
    <TooltipTrigger asChild>{children}</TooltipTrigger>
    <TooltipContent className="max-w-xs">{label}</TooltipContent>
  </Tooltip>
);

/**
 * Goose Local Edition — LIVE swarm run turn loops, verbose. One expandable lane per task: the node
 * identity chip, device, model, and status in the header; on expand, the worker's full reasoning and
 * the actual TOOL CALLS it made (the shell line, edited path, query…) with per-call ok/err. Fed by
 * useSwarmRun (reads <cwd>/.swarm). Running lanes auto-expand so live activity is visible at a glance.
 * A run whose files have gone quiet for STALE_MS is shown as interrupted rather than a live spinner.
 * Sharp full-border card (never a left rail), solid saturated status colors. Renders nothing when idle.
 */

// The LeanZero status palette, one definition (formationVisualState). The plain entries are foreground /
// border colors; the solid* entries are FILLS that carry white text. Nothing here is a tint or an opacity
// fade — every value is fully saturated, and a theme token swaps it between canvases.
const STATUS_COLOR: Record<TurnStatus, string> = {
  running: SWARM_STATUS.running,
  done: SWARM_STATUS.done,
  error: SWARM_STATUS.error,
};
/** The same triad as Studio text classes — for every glyph coloured by a lane's status. */
const STATUS_TONE: Record<TurnStatus, Tone> = { running: 'warn', done: 'ok', error: 'err' };
const CALL_OK = SWARM_STATUS.done;
const CALL_ERR = SWARM_STATUS.error;
const CALL_PENDING = SWARM_STATUS.stopped;
const AMBER = SWARM_STATUS.running;
const BLUE = SWARM_STATUS.action;
// Body colour for MODEL-GENERATED text (live generations, reasoning). The primary text token — solid, never
// a tint or an opacity fade. Chrome (labels, counts, hints) deliberately stays on the secondary token.
const GEN_TEXT = 'var(--color-text-primary)';

/** A node's slot on the six-hue ramp — IDENTITY ONLY, the same slot the ribbon and the fleet use. */
const nodeSlot = (index: number): NodeIndex => ((index % 6) + 1) as NodeIndex;

/** Node identity: a small SOLID dot in the node's ramp hue beside its name — the same identity the
 *  old lettered hexagon glyph and the avatar squares carried, with less sticker. The letter survives
 *  in the aria-label so a reader still hears "node A". The hue is the `bg-lz-node-N` token utility. */
const NodeDot: React.FC<{ index: number; letter: string; size?: 8 | 10; className?: string }> = ({
  index,
  letter,
  size = 8,
  className = '',
}) => (
  <span
    role="img"
    aria-label={`node ${letter}`}
    data-testid="node-dot"
    className={cx(
      'inline-block shrink-0',
      size === 10 ? 'size-2.5' : 'size-2',
      RADIUS.pill,
      NODE_DOT[nodeSlot(index)],
      className
    )}
  />
);

// Human duration from minutes: seconds under a minute, else "Nm Ss".
function fmtDuration(min: number): string {
  const totalSec = Math.max(0, Math.round(min * 60));
  if (totalSec < 60) return `${totalSec}s`;
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return s ? `${m}m ${s}s` : `${m}m`;
}

/** Stable node letter/hue per device so the same node keeps its identity across rows and polls. */
function deviceIndex(device: string, order: string[]): number {
  const i = order.indexOf(device);
  return i < 0 ? order.length : i;
}

export function ago(mtime: number | null): string {
  if (!mtime) return '';
  const s = Math.max(0, Math.round((Date.now() - mtime) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h}h ago` : `${Math.round(h / 24)}d ago`;
}

// Colour a call by what it MEANS, not just ok/fail: an app-error (a command that ran and reported an issue
// while the worker tests) is AMBER (informative), not the red of a genuine malformed tool call.
const CALL_KIND_COLOR: Record<CallMeaning['kind'], string> = {
  ok: CALL_OK,
  'app-error': AMBER,
  // Exit 0 but the OUTPUT proves nothing ran (a `| head` pipe swallowed pytest's failure exit) — solid
  // amber, never the green the exit code lied its way toward.
  'ran-nothing': AMBER,
  malformed: CALL_ERR,
  pending: CALL_PENDING,
};
// A short pill word per kind so the row reads at a glance.
const CALL_KIND_PILL: Record<CallMeaning['kind'], string> = {
  ok: '',
  'app-error': 'app output',
  'ran-nothing': 'ran nothing',
  malformed: 'retried',
  pending: '',
};
const CallTypeIcon: React.FC<{ icon: CallMeaning['icon']; color: string }> = ({ icon, color }) => {
  const cls = 'h-3 w-3 shrink-0';
  const s = { color };
  switch (icon) {
    case 'terminal':
      return <Terminal className={cls} style={s} />;
    case 'test':
      return <FlaskConical className={cls} style={s} />;
    case 'build':
      return <Hammer className={cls} style={s} />;
    case 'run':
      return <Play className={cls} style={s} />;
    case 'write':
      return <FilePlus2 className={cls} style={s} />;
    case 'edit':
      return <FilePenLine className={cls} style={s} />;
    case 'read':
      return <FileText className={cls} style={s} />;
    case 'search':
      return <Search className={cls} style={s} />;
    default:
      return <Wrench className={cls} style={s} />;
  }
};

// A machine-emitted block (shell stdout/stderr, a printed value) rendered TRUE MONOSPACE with alignment
// preserved, capped with a Show-all escape hatch (never truncate-and-lose) and a copy button.
const MonoOutput: React.FC<{ text: string; failed?: boolean }> = ({ text, failed }) => {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const lineCount = text.split('\n').length;
  const big = lineCount > 24;
  return (
    <div>
      {/* Shell/tool output through the SHARED code surface, so it cannot drift from the other code blocks
          again. wrap=true is right here (long prose-ish lines), unlike a JSON payload which must scroll. */}
      <CodeBlock
        text={text}
        wrap
        tone={failed ? 'error' : 'normal'}
        className={!expanded && big ? 'max-h-64 overflow-hidden' : ''}
      />
      <div className="flex items-center gap-3 mt-0.5">
        {big && (
          <button
            onClick={() => setExpanded((e) => !e)}
            className={cx('text-lz-meta text-lz-ink-3 hover:text-lz-ink', MOTION)}
          >
            {expanded ? 'Show less' : `Show all (${lineCount} lines)`}
          </button>
        )}
        <button
          onClick={() => {
            void navigator.clipboard?.writeText(text);
            setCopied(true);
            setTimeout(() => setCopied(false), 1200);
          }}
          className={cx('text-lz-meta text-lz-ink-3 hover:text-lz-ink', MOTION)}
        >
          {copied ? 'copied' : 'copy'}
        </button>
      </div>
    </div>
  );
};

// One tool call as an action→observation unit: the command/target line (always), its full output on
// expand. Each call is EXPLAINED — a tool-type icon, a plain-English intent ("Ran the tests"), and an honest
// outcome ("found a failing test — iterating") so an app-error reads as productive work, not a scary failure.
// A genuine malformed call auto-expands so the reason is zero clicks away.
const CallRow: React.FC<{ call: SwarmCall; defaultOpen?: boolean; ordinal?: number | null }> = ({
  call,
  defaultOpen,
  ordinal,
}) => {
  const m = classifyCall(call);
  const color = CALL_KIND_COLOR[m.kind];
  const pill = CALL_KIND_PILL[m.kind];
  const hasOutput = !!call.result && call.result.trim().length > 0;
  const [open, setOpen] = useState(defaultOpen ?? false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  // A tool-call summary carries the ABSOLUTE path it acted on (e.g. "cat /Users/…/go.mod", "ls /Users/…/miner/").
  // Extract it (file OR directory) so a right-click reveals it in Finder — these rows previously fell through to
  // the native Cut/Copy menu with no Reveal option, which is why it looked "missing everywhere".
  const callPath = ((call.summary ?? '').match(/(\/[^\s'"`]+[^\s'"`:;,.)])/) ?? [])[1] ?? null;
  return (
    <div className="py-0.5 border-b border-lz-border last:border-0">
      <button
        type="button"
        onClick={() => hasOutput && setOpen((o) => !o)}
        onContextMenu={
          callPath
            ? (e) => {
                e.preventDefault();
                setMenu({ x: e.clientX, y: e.clientY });
              }
            : undefined
        }
        className={cx('flex w-full items-start gap-2 text-left', hasOutput ? cx('cursor-pointer', SURFACE.hover) : 'cursor-default', MOTION)}
      >
        <span className="mt-0.5">
          <CallTypeIcon icon={m.icon} color={color} />
        </span>
        <span className="flex-1 min-w-0">
          <span className="flex items-center gap-1.5 flex-wrap">
            <span className="text-xs font-lz-medium text-lz-ink">{m.action}</span>
            {pill ? <Chip tone={m.kind === 'malformed' ? 'err' : 'warn'}>{pill}</Chip> : null}
            {m.kind !== 'ok' ? (
              <span className={cx('text-lz-meta', m.kind === 'malformed' ? TONE_TEXT.err : 'text-lz-ink-3')}>
                {m.outcome}
              </span>
            ) : null}
          </span>
          {call.summary ? (
            <span className="block font-mono text-lz-mono text-lz-ink-2 break-words mt-px" title={call.summary}>
              {call.summary}
            </span>
          ) : null}
        </span>
        {/* WHERE THIS CALL SITS IN THE LANE'S WHOLE HISTORY. The engine sends the last 60 records, so an
            array index is not a position; this makes "last 60 of 69" checkable and gives a reader a
            bearing inside a 60-row scroll. A numeral in the row's own flow — no rail, no chip. */}
        {ordinal != null ? (
          <span className={cx('shrink-0 mt-0.5 font-mono text-lz-mono text-lz-ink-3', TNUM)}>
            #{ordinal}
          </span>
        ) : null}
        {hasOutput &&
          (open ? (
            <ChevronDown className="h-3 w-3 shrink-0 text-lz-ink-3 mt-0.5" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0 text-lz-ink-3 mt-0.5" />
          ))}
      </button>
      {hasOutput && open && (
        <div className="ml-5 mt-1">
          <MonoOutput text={call.result!.trim()} failed={m.kind === 'malformed'} />
        </div>
      )}
      {menu && (
        <ActivityContextMenu
          x={menu.x}
          y={menu.y}
          path={callPath}
          lineText={call.summary ?? m.action}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
};

// The worker's full narration, rendered as readable PROSE (not mono), capped with a Show-all escape hatch.
// In developer mode it starts fully expanded (no cap).
//
// LIVE generations FOLLOW the output. While a node is still generating, a top-anchored clip shows the
// OLDEST text and hides the part being written — you watch a node think and read a stale paragraph. When
// live the box scrolls itself to the newest line instead, so the thing moving is the thing you can see.
const ReasoningBlock: React.FC<{
  text: string;
  forceOpen?: boolean;
  label?: string;
  live?: boolean;
  /** Honest provenance beside the label — e.g. REASONING_CLIP_NOTE when the body is the digest's
   *  24k-clipped tail rather than a durable log. */
  note?: string;
  /** True (the default) when the body is the model's REASONING/GENERATION channel — the one surface that
   *  carries the Studio's secondary accent on its label. A task spec rendered through the same block
   *  passes false: the spec is not the reasoning channel. */
  reasoning?: boolean;
}> = ({ text, forceOpen, label, live, note, reasoning = true }) => {
  const [expandedState, setExpanded] = useState(false);
  const expanded = expandedState || !!forceOpen;
  const words = text.split(/\s+/).filter(Boolean).length;
  const big = text.length > 1200;
  const bodyRef = useRef<HTMLDivElement | null>(null);
  // FINDING 19: collapseRepeats' block scan is O(lines × totalChars) — measured 354ms per call at the
  // 400KB think.log tail cap (822ms at 630KB, 7ms at 40KB), at up to 2Hz on the renderer main thread.
  // Two of this block's feeders (laneNarrative → fullTranscript) are unbounded, so the scan runs over
  // the newest REPEAT_SCAN_CHARS only. Memoized on the LENGTH, deliberately: both channels are
  // append-only, so an unchanged length means unchanged text, while the 500ms poll re-materializes a
  // fresh string identity every tick and keying on the string would defeat the memo.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const collapsed = useMemo(() => collapseRepeats(tailOf(text, REPEAT_SCAN_CHARS)), [text.length]);
  useEffect(() => {
    if (!live) return;
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text, live]);
  const capped = !expanded && big;
  return (
    <div>
      <div className="mb-1.5 flex items-center gap-1.5">
        <span
          className={cx('text-lz-meta', WEIGHT.medium, reasoning ? TONE_TEXT.secondary : 'text-lz-ink-2')}
        >
          {label || 'Reasoning'}
        </span>
        {live ? (
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.ok)}>live</span>
        ) : null}
        {note ? <span className="text-lz-meta text-lz-ink-3">{note}</span> : null}
      </div>
      <div
        ref={bodyRef}
        className={cx(
          'break-words border border-lz-border bg-lz-surface px-3 py-2 text-lz-body text-lz-ink',
          RADIUS.control,
          capped && (live ? 'max-h-[22rem] overflow-y-auto' : 'max-h-[22rem] overflow-hidden')
        )}
        style={{ lineHeight: 1.65 }}
      >
        {/* Prose gets the markdown path; a STRUCTURED payload gets a code path. The plan skeleton used to
            arrive here as raw JSON and markdown both reflowed it into an unreadable wall AND corrupted it —
            `__init__.py` reads as bold syntax, so the file list rendered as **init**.py. */}
        <StructuredContent content={collapsed} />
      </div>
      {big && (
        <button
          onClick={() => setExpanded((e) => !e)}
          className={cx('mt-0.5 text-lz-meta text-lz-ink-3 hover:text-lz-ink', MOTION)}
        >
          {expanded ? 'Show less' : `Show all (${words} words)`}
        </button>
      )}
    </div>
  );
};

const LaneRow: React.FC<{
  lane: TurnLane;
  deviceOrder: string[];
  stale: boolean;
  open: boolean;
  mode: SwarmLogMode;
  dev?: boolean;
  /** True when the fleet runs MORE THAN ONE model, which is the only case where a per-row model id
   *  tells the reader something the node letter does not. */
  heterogeneous?: boolean;
  onToggle: () => void;
}> = ({ lane, deviceOrder, stale, open, mode, dev, heterogeneous, onToggle }) => {
  const idx = deviceIndex(lane.device, deviceOrder);
  const letter = String.fromCharCode(65 + (idx % 26));

  const live = lane.status === 'running' && !stale;
  const interrupted = lane.status === 'running' && stale;
  const Icon = interrupted ? CircleSlash : lane.status === 'done' ? Check : lane.status === 'error' ? X : Loader2;
  const iconTone = interrupted ? TONE_TEXT.stopped : TONE_TEXT[STATUS_TONE[lane.status]];

  const { completed: calls, running } = workRows(lane.calls, lane.inflight);
  // FINDING 23: `calls` is the engine's SLIDING last-60 window, so past 60 resolved calls every new
  // call re-aims each array index at a DIFFERENT call — an index key reassigns row identity under the
  // cursor (an expanded output silently becomes a neighbour's; a context menu targets the wrong
  // path). callRowMeta keys by the absolute ordinal, which survives the slide because tool_calls and
  // the window advance in the same digest write — exactly as WorkPane already does.
  const callMeta = callRowMeta(calls, lane.toolCalls);
  // THE DURABLE TRANSCRIPT FIRST — see `laneNarrative`, which is the one copy of this chain. It was
  // written out here and again on the board row, and the board's copy had already lost its `lastText`
  // fallback, which is how a rule with N copies reads on the day someone edits N-1 of them.
  const rawReasoning = laneNarrative(lane);
  const reasoning = rawReasoning.length >= 8 && /[a-zA-Z]{3,}/.test(rawReasoning) ? rawReasoning : '';
  const failLike = lane.status === 'error' || interrupted;
  const laneError = failLike && lane.error ? lane.error.trim() : '';
  // SAID provenance on the card: the same chips the inspector's SAID surface shows, so a retried
  // lane says "from attempt N · superseded"/"error → retried" here too instead of passing a dead
  // attempt's text off as current. Chips only — the expandable superseded bodies live in the inspector.
  const said = laneSaidState(lane);
  const saidChips = said.superseded.length > 0 || said.live.kind === 'error';
  const hasBody =
    reasoning.length > 0 ||
    calls.length > 0 ||
    (lane.forming?.length ?? 0) > 0 ||
    (lane.recent?.length ?? 0) > 0 ||
    laneError.length > 0 ||
    saidChips;
  // The first call NEEDING ATTENTION auto-expands so the reason is zero clicks away. One definition —
  // this site used `c.ok === false`, which opens on productive app-errors (the worker testing), while the
  // board row two thousand lines down used the classifier. Two rules for one question is how they drift.
  const firstFailIdx = firstCallNeedingAttention(calls);
  // Compact mode's single high-level line: the freshest activity, else the last line of reasoning.
  // A call FORMING leads: its argument bytes are being emitted right now (II-11c), newer than any
  // finished activity or thought — the same rank laneLiveLine gives it.
  const compactLine =
    formingLiveLine(lane.forming) ||
    (lane.recent && lane.recent.length ? lane.recent[lane.recent.length - 1] : '') ||
    (reasoning ? reasoning.split('\n').map((l) => l.trim()).filter(Boolean).slice(-1)[0] ?? '' : '') ||
    lane.lastText?.trim() ||
    '';

  // Human-readable labels for the row's glyphs — so every icon says what it means on hover.
  const secs = typeof lane.elapsedMs === 'number' ? Math.round(lane.elapsedMs / 1000) : null;
  const attemptSuffix =
    typeof lane.attempts === 'number' && lane.attempts > 1 ? ` after ${lane.attempts} attempts` : '';
  const statusTip = interrupted
    ? 'Stalled — no update for over 5 minutes (the run may have been stopped or crashed)'
    : lane.status === 'done'
      ? `Completed${secs != null ? ` in ${secs}s` : ''}${attemptSuffix}`
      : lane.status === 'error'
        ? `This task failed${attemptSuffix}${laneError ? ` — ${laneError.slice(0, 160)}` : ''}`
        : 'Running now';

  return (
    <div data-testid="turn-lane">
      <button
        type="button"
        onClick={onToggle}
        className={cx('flex w-full items-center gap-2 px-3 py-2 text-left', SURFACE.hover, MOTION)}
      >
        {hasBody ? (
          open ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-lz-ink-3" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-lz-ink-3" />
          )
        ) : (
          <span className="w-3.5 shrink-0" />
        )}
        <Tip
          label={
            <span>
              Node {letter} — <span className="font-mono">{lane.device}</span>
              {lane.model ? (
                <>
                  <br />
                  model <span className="font-mono">{lane.model}</span>
                </>
              ) : null}
            </span>
          }
        >
          <NodeDot index={idx} letter={letter} />
        </Tip>
        <Tip label={<span className="font-mono">{lane.device}</span>}>
          <span className="w-16 shrink-0 truncate font-mono text-lz-mono text-lz-ink-3">{lane.device}</span>
        </Tip>
        {/* Readable name: the architect's description ("Tokenize the template source") is the primary label;
            the terse id ("lexer") drops to a mono sub-tag so it's still identifiable but no longer cryptic. */}
        <Tip
          label={
            <span>
              {lane.description || lane.taskId}
              {lane.description ? (
                <>
                  <br />
                  <span className="font-mono">{lane.taskId}</span>
                </>
              ) : null}
            </span>
          }
        >
          {/* THE ID IS NOT A SECOND LINE. "Slice · approval-workflow-outbox" over
              "slice-approval-workflow-outbox" is the same string twice, on every row of every group —
              Mihai: "a lot of this UI is self duplicating information". It stays in the hover tip, where
              it is available without costing a line. */}
          <span className="min-w-0 flex-1 truncate text-lz-body text-lz-ink">
            {lane.description || lane.taskId}
          </span>
        </Tip>
        {/* THE MODEL ID IS THE SAME ON EVERY ROW. A homogeneous fleet runs one model, so
            "workhorse-qwen3.8-27b-br…" truncated identically on fourteen rows says nothing the node
            letter has not already said. It stays in the row's tip, and the FLEET zone still shows the
            full id per node. Kept only when a row's model differs from the rest — a heterogeneous or
            cloud fleet, where it is the whole point. */}
        {lane.model && heterogeneous && (
          <Tip label={<span className="font-mono">{lane.model}</span>}>
            <span className="hidden sm:inline shrink-0 max-w-[9rem] truncate font-mono text-lz-mono text-lz-ink-3">
              {lane.model}
            </span>
          </Tip>
        )}
        <span className={cx('flex shrink-0 items-center gap-2 text-lz-meta text-lz-ink-3', TNUM)}>
          {/* THE JUDGE'S OWN ESTIMATE, shown only while the call is still running — the one number on this
              screen produced by something that READ the work rather than extrapolated from item counts.
              Solid amber, because it is a live claim that will be revised, not a settled fact. */}
          {lane.status === 'running' && typeof lane.judgeEtaMins === 'number' ? (
            <Tip
              label={`The supervisor read this call and estimates ~${lane.judgeEtaMins} more minute${
                lane.judgeEtaMins === 1 ? '' : 's'
              } of work`}
            >
              <span className="inline-flex shrink-0">
                <Chip tone="warn">~{lane.judgeEtaMins}m</Chip>
              </span>
            </Tip>
          ) : null}
          {typeof lane.toolCalls === 'number' && lane.toolCalls > 0 ? (
            <Tip label={`${lane.toolCalls} tool call${lane.toolCalls === 1 ? '' : 's'} in this task`}>
              <span className="flex items-center gap-0.5">
                <Wrench className="h-3 w-3" />
                {lane.toolCalls}
              </span>
            </Tip>
          ) : null}
          {lane.errors ? (
            <Tip label={`${lane.errors} tool call${lane.errors === 1 ? '' : 's'} errored`}>
              <span className={TONE_TEXT.err}>{lane.errors}✕</span>
            </Tip>
          ) : null}
          {secs != null ? (
            <Tip label={lane.status === 'running' ? `Running — ${secs}s so far` : `Took ${secs}s`}>
              <span>{secs}s</span>
            </Tip>
          ) : null}
        </span>
        <Tip label={statusTip}>
          <span className="shrink-0 inline-flex">
            <Icon
              size={15}
              strokeWidth={3}
              className={cx(live && 'animate-spin', iconTone)}
            />
          </span>
        </Tip>
      </button>

      {open && hasBody && (
        <div className="px-3 pb-3 pl-9 space-y-2">
          {laneError ? (
            <div>
              <div className={cx('mb-1 text-lz-meta', TONE_TEXT.err)}>
                {interrupted ? 'Last error before it stalled' : 'Why it failed'}
              </div>
              <MonoOutput text={laneError} failed />
            </div>
          ) : null}
          {mode === 'compact' ? (
            // Compact: a single high-level line of what this node is doing now — no reasoning dump, no calls.
            compactLine ? (
              <div className="text-xs text-lz-ink-2 truncate">{compactLine}</div>
            ) : null
          ) : (
            <>
              {saidChips ? (
                <div className="flex items-center gap-2 flex-wrap" data-testid="lane-said-chips">
                  {said.live.attempt != null ? (
                    <SaidChip kind={said.live.kind} tone={said.live.kind === 'error' ? 'err' : 'ok'}>
                      {`${attemptLabel(said.live.attempt)} · ${said.live.kind === 'error' ? 'error' : 'live'}`}
                    </SaidChip>
                  ) : null}
                  {said.superseded.map((seg, i) => (
                    <SaidChip
                      key={`${seg.attempt ?? 'x'}-${i}`}
                      kind={seg.kind}
                      tone={seg.kind === 'error' ? 'err' : 'stopped'}
                      title={seg.retried ? `retried: ${seg.retried}` : undefined}
                    >
                      {`from ${attemptLabel(seg.attempt)} · ${seg.kind === 'error' ? 'error → retried' : 'superseded'}`}
                    </SaidChip>
                  ))}
                </div>
              ) : null}
              {reasoning && (
                <ReasoningBlock
                  text={reasoning}
                  live={live}
                  forceOpen={dev || live}
                  // Developer: name the model so it's unmistakable WHOSE generation this is.
                  label={dev ? `${live ? 'Generating' : 'Reasoning'} · ${lane.model ?? lane.device}` : undefined}
                  note={narrativeClipNote(lane) ?? undefined}
                />
              )}
              {calls.length > 0 || running.length > 0 || (lane.forming?.length ?? 0) > 0 ? (
                <div>
                  <div className={cx('mb-1.5 text-lz-meta text-lz-ink-2', WEIGHT.medium)}>
                    Tool calls · {lane.toolCalls ?? calls.length}
                    {running.length > 0 ? ` · ${running.length} running` : ''}
                    {formingNote(lane.forming)}
                  </div>
                  <div className={cx('border border-lz-border bg-lz-surface px-2 py-1', RADIUS.control)}>
                    <FormingRows forming={lane.forming} />
                    <InflightRows running={running} />
                    {calls.map((c, i) => (
                      // Developer mode opens every call's output; otherwise only the first failure.
                      <CallRow
                        key={callMeta[i].key}
                        ordinal={callMeta[i].ordinal}
                        call={c}
                        defaultOpen={dev || i === firstFailIdx}
                      />
                    ))}
                  </div>
                </div>
              ) : lane.recent && lane.recent.length > 0 ? (
                <div className="text-xs text-lz-ink-2 font-mono break-words">
                  {lane.recent.join(' · ')}
                </div>
              ) : null}
            </>
          )}
        </div>
      )}
    </div>
  );
};

const ACTIVITY_ICON: Record<ActivityItem['kind'], React.ComponentType<{ size?: number; strokeWidth?: number; className?: string; style?: React.CSSProperties }>> = {
  note: MessageSquare,
  phase: Search,
  plan: ListChecks,
  dispatch: Play,
  done: Check,
  fail: X,
  retry: RotateCcw,
  retarget: TrendingUp,
  review: FlaskConical,
  judge: Eye,
  'judge-act': Gavel,
  prereview: Eye,
  smoke: FlaskConical,
  brief: FileText,
  config: Cpu,
};
// One CLASS per KIND of engine event — the status triad for outcomes, the accent for the rows that CHANGED
// the run, ink for the rows that only watched it. No node hue: the log is chrome, not a node (the old
// map painted plan/dispatch/review/judge in ramp hues, which is node identity on things that are not nodes).
const ACTIVITY_CLASS: Record<ActivityItem['kind'], string> = {
  // The user's own words landing in the build — solid amber so it stands apart from engine chatter.
  note: TONE_TEXT.warn,
  phase: TONE_TEXT.accent,
  plan: 'text-lz-ink-2',
  dispatch: 'text-lz-ink-2',
  done: TONE_TEXT.ok,
  fail: TONE_TEXT.err,
  retry: TONE_TEXT.warn,
  retarget: TONE_TEXT.accent,
  review: 'text-lz-ink-2',
  // An observation recedes; an ACTION is the solid accent, because the whole question the log has to
  // answer at a glance is which rows changed the run and which only watched it.
  judge: 'text-lz-ink-2',
  'judge-act': TONE_TEXT.accent,
  prereview: 'text-lz-ink-2',
  smoke: TONE_TEXT.ok,
  brief: 'text-lz-ink-3',
  config: 'text-lz-ink-3',
};
const TONE_CLASS: Record<NonNullable<ActivityItem['tone']>, string> = {
  info: 'text-lz-ink-3',
  good: TONE_TEXT.ok,
  warn: TONE_TEXT.warn,
  bad: TONE_TEXT.err,
};

// Right-click menu for an activity line — "Reveal in Finder" (when the line references a file), Copy path,
// and Copy. Custom (NOT native) per the no-native-chrome rule: a solid, sharp-cornered, full-bordered menu
// portaled at the cursor, dismissed on outside-click or Escape.
const ActivityContextMenu: React.FC<{
  x: number;
  y: number;
  path: string | null;
  lineText: string;
  onClose: () => void;
}> = ({ x, y, path, lineText, onClose }) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  const itemCls = cx(
    'flex w-full items-center gap-2 px-3 py-1.5 text-left text-lz-body text-lz-ink',
    SURFACE.hover,
    MOTION
  );
  return createPortal(
    <>
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        className={cx('fixed z-50 min-w-[168px] py-1', SURFACE.overlay)}
        style={{ left: x, top: y }}
      >
        {path && (
          <button
            className={itemCls}
            onClick={() => void window.electron.revealInFinder(path).finally(onClose)}
          >
            <FolderOpen size={13} /> Reveal in Finder
          </button>
        )}
        {path && (
          <button
            className={itemCls}
            onClick={() => {
              void navigator.clipboard.writeText(path);
              onClose();
            }}
          >
            Copy path
          </button>
        )}
        <button
          className={itemCls}
          onClick={() => {
            void navigator.clipboard.writeText(lineText);
            onClose();
          }}
        >
          Copy
        </button>
      </div>
    </>,
    document.body
  );
};

/** The EVENT LOG gutter's clock: the engine's RFC3339 `ts` (ActivityItem.at) as a local HH:MM:SS.
 *  Null when there is no `at` or it does not parse — the row shows its ordinal then; a time is never
 *  invented. */
export function eventClock(at: string | undefined): string | null {
  if (!at) return null;
  const ms = Date.parse(at);
  if (Number.isNaN(ms)) return null;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** Gutter widths: the ordinal fits three digits; a clock needs eight mono characters. ONE width per
 *  list (ActivityFeed decides), so mixed rows stay aligned. */
const EVENT_LOG_GUTTER_ORDINAL = 'w-9';
const EVENT_LOG_GUTTER_CLOCK = 'w-16';

// One line of the EVENT LOG ticker: the event's clock (the engine's own `ts`, local HH:MM:SS) — or its
// ordinal when the row carried no time — in a tabular gutter, the kind's icon in its tone, the text in
// ink-2, the detail in ink-3 — the mono register, dense, on the surface-2 well. The log is the
// subordinate narrative record, never competing with the zones above it (quiet = solid ink-3, NOT an
// opacity wash). Right-click reveals a menu — "Reveal in Finder" when the line references a file path,
// plus Copy.
const ActivityLine: React.FC<{ it: ActivityItem; wrap?: boolean; workingDir?: string; gutter: string }> = ({
  it,
  wrap,
  workingDir,
  gutter,
}) => {
  const Icon = ACTIVITY_ICON[it.kind];
  const color = it.tone ? TONE_CLASS[it.tone] : ACTIVITY_CLASS[it.kind];
  const clock = eventClock(it.at);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [open, setOpen] = useState(false);
  const lineText = `${it.text}${it.sub ? ` — ${it.sub}` : ''}`;
  // Thread the run's cwd so a RELATIVE path in an activity line (the common case, e.g. "Wrote src/foo.ts")
  // resolves to an absolute path — without it resolveActivityPath returned null and the context menu silently
  // dropped "Reveal in Finder" + "Copy path" for every relative-path line.
  const path = resolveActivityPath(lineText, workingDir);
  // A line is clickable-to-expand when it carries a sub detail — in the compact feed that detail is truncated,
  // so a click reveals the full text (wrapped, nothing invented — just what the event already carried).
  const expandable = !!it.sub;
  return (
    <li className="flex flex-col" data-kind={it.kind}>
      <div
        className={cx(
          'flex min-h-6 items-start gap-2 px-3 py-0.5',
          expandable && 'cursor-pointer',
          SURFACE.hover,
          MOTION
        )}
        onClick={expandable ? () => setOpen((o) => !o) : undefined}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        {clock ? (
          <time dateTime={it.at} className={cx(gutter, 'shrink-0 text-right text-lz-ink-3', TNUM)}>
            {clock}
          </time>
        ) : (
          <span className={cx(gutter, 'shrink-0 text-right text-lz-ink-4', TNUM)} aria-hidden>
            {it.seq}
          </span>
        )}
        <Icon size={12} strokeWidth={2.5} className={cx('mt-[3px] shrink-0', color)} />
        <span
          className={cx('shrink-0', it.kind === 'judge-act' ? cx(color, WEIGHT.semibold) : 'text-lz-ink-2')}
        >
          {it.text}
        </span>
        {it.sub && !open && (
          <span
            className={cx(
              'text-lz-ink-3',
              wrap ? 'break-words' : 'truncate',
              it.kind === 'brief' ? 'line-clamp-3' : wrap ? 'line-clamp-2' : ''
            )}
          >
            — {it.sub}
          </span>
        )}
        {expandable &&
          (open ? (
            <ChevronDown size={12} className="ml-auto mt-[3px] shrink-0 text-lz-ink-3" />
          ) : (
            <ChevronRight size={12} className="ml-auto mt-[3px] shrink-0 text-lz-ink-3" />
          ))}
      </div>
      {it.sub && open && (
        <div
          className={cx(
            'mb-1 ml-[68px] mr-3 mt-0.5 whitespace-pre-wrap break-words border border-lz-border bg-lz-surface px-2 py-1.5 text-lz-ink-2',
            RADIUS.control
          )}
        >
          {it.sub}
        </div>
      )}
      {menu && (
        <ActivityContextMenu
          x={menu.x}
          y={menu.y}
          path={path}
          lineText={lineText}
          onClose={() => setMenu(null)}
        />
      )}
    </li>
  );
};

// A per-NODE live strip: one row per physical device the run has used, showing what each node is doing RIGHT
// NOW (its running task) or "idle". The existing lanes are per-TASK, so parallel work across the fleet — and,
// just as importantly, an IDLE node during a serial tail — was invisible; this makes the real fleet occupancy
// (and any starvation) legible at a glance. Derived from the run's OWN device set (no fleet-endpoint join, so
// no host-id vs model-id naming mismatch). deviceOrder is the same sorted set the lanes color by, so a node's
// letter/hue is identical here and in its lane rows.
// Smoothly REVEAL streamed text. The activity digest is polled in discrete ~500ms chunks, so showing each
// snapshot raw makes the live generation jump a paragraph at a time — choppy, "slowly polled". Instead we type
// toward the latest snapshot at a steady rate, so the text flows between polls. When the snapshot APPENDS (the
// common case — the model still generating), we keep typing the new chars; when it changes shape (a new tool
// call, or the tail window slid past what we've shown), we resync to the longest shared prefix and continue.
/**
 * TYPE THE NEW TEXT; NEVER CHASE A BACKLOG.
 *
 * The reveal only ever appends, so a target that arrives far ahead of what is shown is typed out from the
 * beginning at 110 chars/sec — a strip cell fed the durable log's 2,400-char tail would sit twenty seconds
 * behind the model on its very first frame, and a node's freshest line is the whole point of the cell.
 * Past one poll's worth of text the reveal SNAPS, so smoothing keeps only the case it was written for:
 * the handful of characters that landed between two polls.
 */
export const MAX_REVEAL_BACKLOG_CHARS = 240;

export function revealStep(args: {
  target: string;
  current: string;
  charsPerSec: number;
  deltaSeconds: number;
  reduceMotion: boolean;
}): string {
  if (args.target.length - args.current.length > MAX_REVEAL_BACKLOG_CHARS) return args.target;
  return nextRevealedText(args);
}

/**
 * THE TYPEWRITER MUST NOT OUTLIVE ITS ANIMATION LOOP. A hidden/occluded window suspends rAF entirely,
 * and `shown` only advanced inside rAF — so the painted text froze at its last frame (measured over CDP
 * on the live r0 benchmark, 2026-08-30: the committed `text` prop moved from a 1,253-char paragraph to
 * "💭 Hmm wait, …" across 10s while the DOM held 507 stale chars ending mid-word for the whole sampled
 * window). The React path was healthy; the last link lied. A hidden page is treated exactly like
 * reduced motion: deliver the target directly, and resume typing from it when the page is seen again.
 * Exported for the fixture test that pins this.
 */
export function useSmoothText(target: string, charsPerSec = 110): string {
  const [shown, setShown] = useState('');
  const reduceMotion = usePrefersReducedMotion();
  const pageVisible = usePageVisible();
  const snap = reduceMotion || !pageVisible;
  const targetRef = useRef(target);
  targetRef.current = target;
  const shownRef = useRef('');

  useEffect(() => {
    if (!snap) return;
    shownRef.current = target;
    setShown(target);
  }, [snap, target]);

  useEffect(() => {
    if (snap) return;
    let raf = 0;
    let last = Date.now();
    const tick = () => {
      const now = Date.now();
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      const tgt = targetRef.current || '';
      const cur = revealStep({
        target: tgt,
        current: shownRef.current,
        charsPerSec,
        deltaSeconds: dt,
        reduceMotion: false,
      });
      if (cur !== shownRef.current) {
        shownRef.current = cur;
        setShown(cur);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [charsPerSec, snap]);
  return snap ? target : shown;
}

// The per-node live-generation line — typewriter-smoothed so it flows instead of jumping every poll, and
// anchored to the BOTTOM (auto-scrolled) so the NEWEST generation is always visible. A line-clamp from the top
// would freeze on the oldest text once the stream grew past a few lines.
const NodeLiveText: React.FC<{ text: string; lines: number }> = ({ text, lines }) => {
  const shown = useSmoothText(text);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [shown]);
  return (
    <div
      ref={ref}
      data-testid="fleet-node-gen"
      className="mt-0.5 break-words whitespace-pre-wrap"
      style={{ maxHeight: lines * 16, lineHeight: '16px', overflow: 'hidden', color: GEN_TEXT }}
    >
      {/* Model-authored text arrives with `backticks` and **bold**; rendered raw it showed its own
          asterisks. InlineMarkdown gives code fragments a chip so they read differently from the prose,
          and tolerates a half-streamed marker (an unclosed ** is just text). */}
      <InlineMarkdown content={shown || text} />
    </div>
  );
};

/**
 * ONE FOLLOW IMPLEMENTATION, for every scroller in the inspector.
 *
 * WHY A SENTINEL AND NOT `scrollTop = scrollHeight`. The old assignment ran in an effect, and any append
 * landing before layout settled satisfied the `scrollHeight - scrollTop - clientHeight < 40` at-bottom test
 * in `onScroll` — so follow latched OFF while the user believed it was on, and the pane sat still while the
 * node generated. Mihai: *"the right side you can't even scroll, it jumps down but does not actively show
 * what is being generated"*. Observing the END ELEMENT cannot race a growing list: it is either in view or
 * it is not.
 *
 * BOTH feature guards are load-bearing — jsdom has neither `IntersectionObserver` nor `scrollIntoView`, and
 * the panel's smoke tests render this tree.
 */
const FollowScroll: React.FC<{
  dep: unknown;
  className?: string;
  /** Full-log views only: they land scrolled to the END (the follow idiom), and Mihai's original
   *  complaint about them was "it cuts the BEGINNING" — so a durable whole-file body offers a named
   *  jump to its start, with the existing follow button as the way back to the end. Live tails keep
   *  the plain follow behavior: their beginning is not on screen to jump to. */
  jumpToStart?: boolean;
  children: React.ReactNode;
}> = ({ dep, className, jumpToStart, children }) => {
  const boxRef = useRef<HTMLDivElement>(null);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);

  useEffect(() => {
    if (typeof window.IntersectionObserver === 'undefined') return;
    const io = new window.IntersectionObserver(([e]) => setFollow(e.isIntersecting), {
      root: boxRef.current,
      rootMargin: '0px 0px 40px 0px',
    });
    if (sentinelRef.current) io.observe(sentinelRef.current);
    return () => io.disconnect();
  }, []);

  useEffect(() => {
    if (follow) sentinelRef.current?.scrollIntoView?.({ block: 'end' });
  }, [dep, follow]);

  return (
    <div className="relative flex flex-col min-h-0 h-full">
      <div
        ref={boxRef}
        className={`flex-1 min-h-0 overflow-y-auto ${className ?? ''}`}
        style={{ scrollPaddingBlockEnd: 8 }}
      >
        {children}
        <div ref={sentinelRef} aria-hidden style={{ height: 1 }} />
      </div>
      {jumpToStart ? (
        <button
          onClick={() => {
            setFollow(false);
            if (boxRef.current) boxRef.current.scrollTop = 0;
          }}
          aria-label="Jump to the start of this log"
          className={cx(
            'absolute right-3 top-2 h-6 px-2 text-lz-meta',
            WEIGHT.medium,
            RADIUS.control,
            'border border-lz-border-strong bg-lz-surface text-lz-ink hover:bg-lz-surface-2',
            MOTION
          )}
        >
          ↑ start
        </button>
      ) : null}
      {!follow ? (
        <button
          onClick={() => {
            setFollow(true);
            sentinelRef.current?.scrollIntoView?.({ block: 'end' });
          }}
          aria-label={jumpToStart ? 'Back to the end of this log' : 'Follow the newest text'}
          className={cx(
            'absolute bottom-2 right-3 h-6 px-2 text-lz-meta',
            WEIGHT.semibold,
            RADIUS.control,
            TONE_FILL.accent,
            MOTION
          )}
        >
          {jumpToStart ? '↓ end' : '↓ follow'}
        </button>
      ) : null}
    </div>
  );
};

// The expanded full-generation box — auto-scrolls to the newest text as the stream grows (like a terminal),
// but only when the user is already near the bottom, so scrolling up to read stays put.
/**
 * A live text stream you can actually READ.
 *
 * `fill` is the difference between an inline row and a modal pane, and getting it wrong made the node
 * inspector nearly useless. MEASURED 2026-08-28: this box was written for the collapsed row — hard
 * `maxHeight: 300` and `ml-6` margins — and then reused inside a full-screen modal, so the text stopped a
 * third of the way down a 950px pane and two-thirds of the window was dead space. Mihai: *"the content
 * does not cover the full estate, the output rolls and it does not save into a cohesive unit."*
 *
 * FOLLOW IS A CHOICE, NOT A BEHAVIOUR. Auto-scrolling only when already at the bottom sounds right, but on
 * a stream that is appending constantly you are ALWAYS at the bottom, so it yanks forever and reading is
 * impossible. Following is now explicit and stops the moment you scroll away — scroll back down to resume.
 */
const NodeExpandBox: React.FC<{ text: string; fill?: boolean; jumpToStart?: boolean }> = ({
  text,
  fill,
  jumpToStart,
}) => {
  if (fill) {
    return (
      <FollowScroll
        dep={text}
        jumpToStart={jumpToStart}
        className="whitespace-pre-wrap break-words px-3 py-2 font-mono text-lz-mono text-lz-ink-2"
      >
        {text}
      </FollowScroll>
    );
  }
  return (
    <div className="contents">
      <div
        className={cx(
          'mb-1 ml-6 mt-1 whitespace-pre-wrap break-words border border-lz-border bg-lz-surface-2 p-2 font-mono text-lz-mono text-lz-ink-2',
          RADIUS.control
        )}
        style={{ maxHeight: 300, overflowY: 'auto' }}
      >
        {text}
      </div>
    </div>
  );
};

/**
 * NODE INSPECTOR — one node's full stream, in a window big enough to read it.
 *
 * WHY. The fleet strip put a node's generation as loose text under its name, clipped by whatever height
 * the row happened to have. Mihai: *"it's sort of useless what it shows now… it's just throwing in some
 * floating text… here it usually gets truncated"*.
 *
 * The two channels are shown SEPARATELY because they are separate in the protocol and answer different
 * questions. THINKING is the reasoning channel — what the model is working out. OUTPUT is what it actually
 * emitted: the tool calls it made and the text it returned. A node reasoning hard while emitting nothing is
 * the precise state that has cost this project whole runs, and it is invisible the moment the two are
 * concatenated into one blob.
 *
 * Both panes reuse NodeExpandBox, so each follows the newest text like a terminal while a scroll-up to
 * read stays where it was put.
 */
/// WHAT THE OUTPUT PANE SHOWS. The mirror of `inspectorThinkingText`, and the same bug in the other
/// channel: `fullTranscript` is the durable `<task>.log` — every chunk of the ANSWER channel, appended,
/// no clip — while `lastText` is the digest's ROLLING view of that same stream.
///
/// Mihai, on `approval-workflow`: the OUTPUT pane ended mid-sentence at "currency", having scrolled away
/// its own beginning. `main.ts` had been supplying `full_transcript` on every digest the whole time and
/// NOTHING in the UI read it — zero references outside main.ts.
///
/// TOOL CALLS ARE NOT TEXT AND ARE NO LONGER PREPENDED HERE. `recent` is `calls` with the information
/// removed — the engine builds it as six literal `"<name> ok|ERR"` strings (swarm.rs `recent`), so a lane
/// that made 51 calls rendered the six words "shell ok" and nothing about what it ran or what came back.
/// The inspector now renders `calls` themselves (WorkPane), where `summary` is the command and `result`
/// is its output; this function is only what the model SAID. `StreamLane.recent` stays in the type —
/// `laneLiveLine` still reads it as a last-resort live line.
/**
 * The stream fields of a lane, as every renderer needs them.
 *
 * `TurnLane` satisfies it and a raw activity digest can be mapped onto it, which is the point: the rules
 * below are written ONCE against this shape instead of being re-typed inline at each render site, where
 * they diverged into a strip that read only the rolling windows and a card that read only the 24k clip.
 */
export type StreamLane = {
  fullTranscript?: string;
  fullThinking?: string;
  fullReasoning?: string;
  reasoning?: string;
  lastText?: string;
  lastThinking?: string;
  thinkingChars?: number;
  recent?: string[];
  liveChannel?: LiveChannel;
  inflight?: InflightCall[];
  forming?: FormingCall[];
};

/** The fleet cell's line for a node waiting on a tool: what it is running, not what it last thought. */
export function inflightLiveLine(running: InflightCall[] | undefined): string {
  if (!running || running.length === 0) return '';
  const newest = running[running.length - 1];
  return `running: ${newest.args}${running.length > 1 ? ` +${running.length - 1}` : ''}`;
}

/** The live line for a call still FORMING (II-11c): the named call, the honest byte count, and the
 *  freshest LINE of the argument bytes streamed so far — the generation transforming into the tool
 *  call, on the one line a cell has. A zero-byte forming call (name announced, no argument bytes
 *  yet) returns nothing and the caller falls through: with no bytes there is no line to hand a row,
 *  and its FormingRow already shows the name, the spinner and the clock. */
export function formingLiveLine(forming: FormingCall[] | undefined): string {
  if (!forming || forming.length === 0) return '';
  const newest = forming[forming.length - 1];
  if (!newest.args_bytes || !newest.args_preview) return '';
  const tool = newest.name.replace(/^developer__/, '');
  const tail = lastSubstantiveLine(newest.args_preview);
  return `forming ${tool} · ${newest.args_bytes.toLocaleString()} bytes${tail ? ` · ${tail}` : ''}`;
}

/**
 * The LIVE attempt's slice of the durable answer log — never a superseded attempt's text.
 *
 * `<task>.log` is append-only ACROSS attempts, so after a retry its tail is still the dead attempt's
 * final words. Measured on r0's ledger-core-tests: attempt 0's "Network error … Please resend your
 * message" stood as the pane's current answer for 24+ minutes while attempt 1 was thinking. The engine
 * now writes an attempt-marker line at every dispatch; everything before the last marker belongs to a
 * previous attempt and is rendered as SUPERSEDED (SaidSection), never as the live body. A legacy log
 * with no markers comes back whole, exactly as before.
 */
function liveTranscript(lane: { fullTranscript?: string }): string {
  return splitTranscriptAttempts(lane.fullTranscript).live.text.trim();
}

export function inspectorOutputText(lane: StreamLane): string {
  return liveTranscript(lane) || lane.lastText?.trim() || '';
}

/** One attempt's SAID text as the pane renders it. */
export interface SaidSegmentView {
  attempt: number | null;
  text: string;
  kind: SaidKind;
  /** The task_retry failure text that followed this segment's attempt, when known. */
  retried?: string;
}

/** What the SAID surface shows: the live attempt plus everything a retry superseded. */
export interface SaidState {
  live: SaidSegmentView;
  superseded: SaidSegmentView[];
}

/**
 * SAID provenance for a lane, from BOTH channels: the durable log split at its attempt markers
 * (preferred — full text), falling back to the digest's `superseded` list (a mirror lane has no log
 * twin; main.ts's 200k tail read can clip an old attempt's marker away). The error/said classification
 * is the same deterministic rule the engine stamps (`saidKindOf`), so a legacy digest with no
 * `said_kind` still classifies its transport errors correctly.
 */
export function laneSaidState(lane: {
  fullTranscript?: string;
  lastText?: string;
  attempt?: number;
  saidKind?: SaidKind;
  superseded?: SupersededSaid[];
  error?: string;
}): SaidState {
  const split = splitTranscriptAttempts(lane.fullTranscript);
  const liveText = split.live.text.trim() || lane.lastText?.trim() || '';
  const liveAttempt = split.live.attempt ?? lane.attempt ?? null;
  // A superseded segment with the SAME attempt number as the live one is a previous CALL on a reused
  // lane key (REVIEW reuses keys every round, always at attempt 0) — "from attempt 1" beside
  // "attempt 1 · live" would read as a contradiction, so it renders as "earlier call" instead.
  const sameAsLive = (n: number | null | undefined): number | null =>
    n != null && n === liveAttempt ? null : (n ?? null);
  const fromTranscript: SaidSegmentView[] = split.superseded.map((s) => ({
    attempt: sameAsLive(s.attempt),
    text: s.text.trim(),
    kind: saidKindOf(s.text),
  }));
  const fromDigest: SaidSegmentView[] = (lane.superseded ?? [])
    .filter((s) => (s.last_text ?? '').trim().length > 0)
    .map((s) => ({
      attempt: sameAsLive(s.attempt),
      text: (s.last_text ?? '').trim(),
      kind: s.said_kind ?? saidKindOf(s.last_text),
    }));
  const superseded = fromTranscript.length > 0 ? fromTranscript : fromDigest;
  if (superseded.length > 0 && lane.error) {
    superseded[superseded.length - 1].retried = lane.error;
  }
  return {
    live: {
      attempt: liveAttempt,
      text: liveText,
      kind: saidKindOf(liveText),
    },
    superseded,
  };
}

/**
 * THE ANSWER CHANNEL'S BLANK RUNS ARE AN ARTIFACT, NOT SPACING.
 *
 * `<task>.log` is `texts[already..].join("")` over raw stream deltas (append_reasoning_transcript in
 * swarm.rs) — nobody authored the 13-line gaps, the chunking produced them, and `whitespace-pre-wrap`
 * renders every one of them. MEASURED on lane `apptest-advertised-surface`: 2,300 bytes over 143 lines,
 * 123 of them blank, runs of up to 13 — a pane 86% empty.
 *
 * The TRAILING trim is not cosmetic. Follow scrolls to the end, and if the last 13 lines are blank it
 * lands the viewport on nothing, which is half of "it jumps down but does not actively show what is
 * being generated".
 *
 * DELIBERATELY NOT folded into `collapseRepeats`, which preserves blank runs on purpose and is shared
 * with the reasoning surfaces; the two channels differ in kind and the comment there must keep saying why.
 */
export function squeezeBlankRuns(text: string): string {
  if (!text) return text;
  const lines = text.split('\n');
  const isBlank = (l: string) => l.trim().length === 0;
  let start = 0;
  let end = lines.length;
  while (start < end && isBlank(lines[start])) start += 1;
  while (end > start && isBlank(lines[end - 1])) end -= 1;
  const out: string[] = [];
  for (let i = start; i < end; i += 1) {
    if (isBlank(lines[i])) {
      // Keep the FIRST line of the run verbatim rather than substituting '': a lone whitespace-only line
      // is not a run and must survive byte-identical.
      if (out.length && isBlank(out[out.length - 1])) continue;
      out.push(lines[i]);
      continue;
    }
    out.push(lines[i]);
  }
  return out.join('\n');
}

/** COUNT WHAT IS ON SCREEN — and say when the screen is holding less than the log did. Kept separate from
 *  `thinkingCaption` so that caption's signature, and its tests, stay exactly as they are. */
export function squeezeNote(before: string, after: string): string {
  const blanks = (t: string) => t.split('\n').filter((l) => l.trim().length === 0).length;
  const n = blanks(before) - blanks(after);
  return n > 0 ? ` · ${n} blank lines collapsed` : '';
}

/// WHICH TEXT THE INSPECTOR SHOWS. Exported so the rule can be tested without rendering, because this pane
/// has now been wrong twice: once truncated (the header counted the real transcript while the body fell
/// back to the rolling window) and once DOUBLED (the log and the window concatenated).
///
/// `fullThinking` is the durable `<task>.think.log`. `lastThinking` is the digest's 2,400-char ROLLING
/// WINDOW over that same stream — a suffix of the log by construction. They must never be joined.
export function inspectorThinkingText(lane: StreamLane): string {
  const durable =
    lane.fullThinking?.trim() || lane.fullReasoning?.trim() || lane.reasoning?.trim() || '';
  return durable || (lane.lastThinking?.trim() ?? '');
}

/**
 * How much of a durable log an INLINE surface takes.
 *
 * The logs are unbounded (main.ts hands over up to 400,000 chars); a two-line strip cell rendering all of
 * it makes the typewriter reveal chase a multi-KB backlog and makes every poll re-parse a novel. The tail
 * is the end a reader is following, and 2,400 matches the rolling window it replaces, so the strip shows
 * no less than it did — it just stops CLEARING between polls.
 */
export const INLINE_TAIL_CHARS = 2400;

/**
 * The block-repeat scan's input bound (finding 19). collapseRepeats re-joins the whole prefix per
 * candidate block size — O(lines × totalChars) — and the unclipped feeders hand it up to 400KB at
 * poll cadence. 24,000 matches the digest clip's own budget (taskGenReasoning already caps there);
 * the durable logs on disk stay complete.
 */
export const REPEAT_SCAN_CHARS = 24_000;

/** The preview cards show a lot more than a strip cell, but still not a whole log — this is the same
 *  volume the old 24,000-char `full_reasoning` clip put on screen, now taken from the durable log. */
export const CARD_TAIL_CHARS = 24_000;

export function tailOf(text: string, max: number): string {
  return text.length > max ? text.slice(-max) : text;
}

/**
 * THE REASONING RUN A NODE IS ON, for any inline surface.
 *
 * Prefers the durable `<task>.think.log`; the digest's `lastThinking` is a ROLLING WINDOW the engine
 * rewrites ~2.5x a second, which is why a cell fed from it clears and refills instead of advancing.
 * The window is still the fallback for a lane whose log has not appeared yet, and it keeps its
 * `thinkingChars` gate: without a live counter behind it the window may be a stale leftover.
 */
export function laneThinkingRun(lane: StreamLane): string {
  const durable = lane.fullThinking?.trim() ?? '';
  const windowed = (lane.thinkingChars ?? 0) > 0 ? (lane.lastThinking?.trim() ?? '') : '';
  return collapseRepeats(tailOf(durable || windowed, INLINE_TAIL_CHARS));
}

/**
 * THE ONE LIVE LINE — what this node is saying RIGHT NOW — shared by the fleet strip and the task board.
 *
 * Both used to compute it themselves off `reasoning` / `lastThinking` / `lastText`, every one of which is
 * a digest window rewritten in place, and the board's copy had already diverged to two branches. So the
 * expanded lane view was fixed to read the durable logs and the two surfaces a reader looks at FIRST were
 * left rolling. The durable transcript leads here, tail-bounded; the digest fields remain as fallbacks for
 * a lane with no log yet.
 *
 * The SUBSTANCE gate stays: the text channel emits single-token fragments ("m", ".", " with"), and a busy
 * node rendered as one meaningless letter is worse than falling through to the next source.
 *
 * THE FRESHEST LINE, NOT THE FRESHEST BLOCK. This took `tailOf(..., 2400)` and handed the whole 2,400-char
 * block to a single-line row, which renders its BEGINNING — so the live line showed narration from 2,400
 * characters ago and only advanced when the whole block rolled past. That is "the output rolls" again, in
 * the two surfaces a reader looks at first, after it had been fixed in the expanded view. The tail bound
 * stays (it is what keeps this cheap on a megabyte transcript); the last substantive LINE within it is
 * what a live line actually means.
 */
export function lastSubstantiveLine(text: string): string {
  const lines = text.split('\n');
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    const chunk = substantiveChunk(lines[i]);
    if (chunk) return chunk;
  }
  // No line clears the substance gate on its own — a single unbroken run of output, or pure fragments.
  // Fall back to the tail of the whole block rather than showing nothing.
  return substantiveChunk(text);
}

export function laneLiveLine(lane: StreamLane): string {
  // A CALL STILL FORMING OUTRANKS EVERYTHING: its argument bytes are what the model is emitting at
  // this instant — both channels are frozen while a tool call's JSON forms (r5's OPEN read as dead
  // for 5 minutes while 28 KB of arguments streamed), so any other source is older by construction.
  const forming = formingLiveLine(lane.forming);
  if (forming) return forming;
  // A TOOL CALL IN FLIGHT OUTRANKS BOTH CHANNELS. While the node waits on a tool neither channel moves,
  // so the freshest thought is what it thought BEFORE it acted; the request is newer by construction and
  // it is the one thing the node is doing. The channel rule below still decides between the two streams
  // the moment the result lands and the row drops.
  const running = inflightLiveLine(lane.inflight);
  if (running) return running;
  // THE CHANNEL THAT MOVED LAST LEADS. A fixed transcript-first order showed a REVIEW lane's round-1
  // answer for the whole of round 2's thinking (measured on r1: cell text unchanged across two 10-minute
  // ticks while thinking_chars climbed past 24,000), because the lane key is reused every round and
  // `<task>.log` still holds the previous answer. The hook says which channel grew in the latest poll;
  // when it is the thinking, the freshest thought is the live line and the answer chain is the fallback.
  if (lane.liveChannel === 'thinking') {
    const thought = thinkingLiveLine(lane);
    if (thought) return thought;
  }
  return (
    lastSubstantiveLine(tailOf(liveTranscript(lane), INLINE_TAIL_CHARS)) ||
    substantiveChunk(lane.reasoning) ||
    // THE THINKING PATH NEEDED THE SAME TREATMENT AND DID NOT GET IT.
    //
    // The transcript branch above was changed to the freshest LINE; this one still handed
    // `fleetThinkingLine`'s 2,400-character BLOCK to a single-line row, which renders its beginning. And
    // this is the branch that matters: OPEN and RESEARCH are pure reasoning, so every lane in them has
    // no transcript at all and falls through to here. Caught by the tick on a named lane -- "workhorse
    // (slice-boot-wrapper): its digest ADVANCED and its cell text did NOT" -- which is the complaint
    // itself, still live in the half of the fix I did not apply.
    thinkingLiveLine(lane) ||
    substantiveChunk(lane.lastText) ||
    (lane.recent && lane.recent.length > 0
      ? substantiveChunk(lane.recent[lane.recent.length - 1])
      : '')
  );
}

/// THE COMPACT SIBLING LINE NAMES THE LANE; THE BOARD DESCRIBES IT.
///
/// A planning digest label is an identity plus a caption — "Coverage 1 · what the request names that
/// nothing owns" — and the WORK board paints the whole of it on that lane's own row, where it has the
/// width. Under a fleet cell the same string was painted again into 40% of a row, so every coverage lane
/// read "Coverage 1 · what the request na…" beside its live text: the caption cost the live line its
/// space and said nothing the board had not (measured live, 2026-08-29: three lanes, six paints). The
/// identity is the join a reader needs here.
export function laneSiblingTitle(lane: Pick<TurnLane, 'taskId' | 'description'>): string {
  const title = cleanTaskTitle(lane.description ?? lane.taskId, lane.taskId);
  if (!isPlanningDigestKey(lane.taskId)) return title;
  const cut = title.indexOf(' · ');
  return cut > 0 ? title.slice(0, cut) : title;
}

/// The reasoning run reduced to ONE line, for the inline surfaces. `fleetThinkingLine` keeps returning
/// the whole block because the expand box wants all of it; a row that shows one line must not be handed
/// a block and left to render whichever end it happens to render.
export function thinkingLiveLine(lane: StreamLane | undefined): string {
  const last = lane ? lastSubstantiveLine(laneThinkingRun(lane)) : '';
  return last ? `💭 ${last}` : '';
}

/// THE FLEET CELL'S THINKING LINE — the reasoning run, marked as reasoning.
///
/// It was written out twice inline, once for the visible line and once for the expandable text, so the
/// two could disagree about whether a cell has thinking at all.
export function fleetThinkingLine(lane: StreamLane | undefined): string {
  const think = lane ? laneThinkingRun(lane) : '';
  return think ? `💭 ${think}` : '';
}

/// WHAT A FLEET CELL CAN EXPAND, and therefore whether its row is clickable at all: the durable narration
/// plus the thinking line. Exported, and mirrored onto the cell as `data-gen-len`/`data-expandable`, so an
/// out-of-repo instrument reads the rule's OUTPUT instead of re-deriving it — tick_ui.mjs declared a row
/// unclickable whenever `full_transcript` was empty, so it called every thinking-only lane dead and every
/// silent transcript-holder live.
///
/// A THINKING-ONLY model must never produce an unclickable row: that is the node you most need to open.
export function fleetExpandText(lane: StreamLane | undefined): string {
  const said = (lane ? liveTranscript(lane) : '') || lane?.fullReasoning?.trim() || '';
  return [said, fleetThinkingLine(lane)].filter(Boolean).join('\n\n');
}

/** The narration a ROW renders in its expanded body: the durable answer channel, then the clipped digests. */
export function laneNarrative(lane: StreamLane): string {
  return (
    liveTranscript(lane) ||
    lane.fullReasoning?.trim() ||
    lane.reasoning?.trim() ||
    lane.lastText?.trim() ||
    ''
  );
}

/// A tail read that cut a multi-byte character in half costs at most three bytes of the decoded text, so a
/// log within this of its own file size has not been clipped.
const TAIL_DECODE_SLACK_BYTES = 8;

const utf8Bytes = (text: string): number => new TextEncoder().encode(text).length;

/**
 * IS THE PANE SHOWING ONLY THE END OF A LONGER LOG? One rule, both channels.
 *
 * main.ts reads a bounded tail of each durable log and knows the byte budget it read with, so where it has
 * already answered the question (`transcript_clipped`) that answer wins. Where it has not, the comparison
 * is made in BYTES against the durable text — never against what is on screen, which is counted in UTF-16
 * units and may have been collapsed or prefixed by another channel first. The OUTPUT caption did exactly
 * that (`transcriptBytes > outText.length + 1024`) and would call a complete non-ASCII log a tail.
 */
export function isClippedTail(durable?: string, bytes?: number, clipped?: boolean): boolean {
  if (typeof clipped === 'boolean') return clipped;
  if (typeof bytes !== 'number' || !durable) return false;
  return utf8Bytes(durable) + TAIL_DECODE_SLACK_BYTES < bytes;
}

/** The caption suffix that admits a pane is a tail — silence when it is showing the whole log. */
export function streamTailNote(durable?: string, bytes?: number, clipped?: boolean): string {
  if (!isClippedTail(durable, bytes, clipped)) return '';
  return ` · tail of ${Math.round((bytes ?? 0) / 1024).toLocaleString()}KB`;
}

/**
 * THE THINKING CAPTION, including the case where there is NO durable log.
 *
 * `streamTailNote` goes silent when it cannot prove a clip, which is right for the Output pane but wrong
 * here: a lane with no `<task>.think.log` yet is rendering the digest's ROLLING 2,400-char window, and
 * captioning that as a bare "2,400 chars" states that the reader is seeing everything. They are seeing
 * the newest 2,400 characters of a stream that may be twenty times that, with the rest already
 * overwritten. That is the same truncation lie this pane has now shipped twice, just with the count
 * telling it instead of the body.
 *
 * `thinkingChars` is not a size — it is the engine's per-stream counter and it RESETS on a re-stream —
 * but it is a lower bound on what the stream has produced, so when it exceeds what is on screen it is
 * enough to say the window is partial. `thinkingBytes` (the real file size) is preferred whenever the
 * durable log exists.
 */
export function thinkingCaption(
  shown: string,
  durable?: string,
  bytes?: number,
  engineChars?: number
): string {
  const n = shown.length.toLocaleString();
  const note = streamTailNote(durable, bytes);
  if (note) return `${n} chars${note}`;
  if (!durable && typeof engineChars === 'number' && engineChars > shown.length) {
    return `${n} of ${engineChars.toLocaleString()} chars · rolling window, no durable log yet`;
  }
  return `${n} chars`;
}

/**
 * The text the per-task LIVE GENERATION card shows.
 *
 * It reached for `full_reasoning` — the engine's 24,000-char TAIL CLIP — first, and never looked at the
 * durable logs sitting unread in the same digest object, so this surface still showed a clipped
 * transcript after every other reader had been moved onto the append-only logs.
 */
export function taskGenReasoning(digest: Record<string, unknown>): string {
  const str = (k: string) => (typeof digest[k] === 'string' ? (digest[k] as string) : undefined);
  const lane: StreamLane = {
    fullThinking: str('full_thinking'),
    fullReasoning: str('full_reasoning'),
    reasoning: str('reasoning'),
    lastThinking: str('last_thinking'),
    fullTranscript: str('full_transcript'),
    lastText: str('last_text'),
  };
  // BOTH DURABLE LOGS OUTRANK EVERY CLIPPED VIEW. Taking the inspector's chain whole would put
  // `full_reasoning` — the clip this card is here to stop showing — ahead of `full_transcript`.
  const durable = lane.fullThinking?.trim() || liveTranscript(lane) || '';
  const text = durable || inspectorThinkingText(lane) || laneNarrative(lane);
  return tailOf(text, CARD_TAIL_CHARS);
}

/** The honest words for a body that is `full_reasoning` — the digest's 24,000-char clipped tail. */
export const REASONING_CLIP_NOTE = 'last 24k chars — archived digest; full log unavailable';

/**
 * THE FALLBACK STAYS; THE CAPTION CLOSES THE ITEM (agenda item V's residue). Archived runs whose
 * durable logs are gone still carry `full_reasoning` in the digest — a 24k TAIL CLIP — and every
 * surface that falls back to it used to present the clip as the whole record. These predicates say,
 * per chain, whether the body a surface is about to show IS that clip, so the caption can say so.
 *
 * `narrativeClipNote` matches laneNarrative's chain (transcript first, then the clip);
 * `taskGenClipNote` matches taskGenReasoning's (both durable logs outrank it).
 */
export function narrativeClipNote(lane: StreamLane): string | null {
  return !liveTranscript(lane) && (lane.fullReasoning?.trim() ?? '') ? REASONING_CLIP_NOTE : null;
}

export function taskGenClipNote(digest: Record<string, unknown>): string | null {
  const str = (k: string) => (typeof digest[k] === 'string' ? (digest[k] as string) : undefined);
  const lane: StreamLane = {
    fullThinking: str('full_thinking'),
    fullReasoning: str('full_reasoning'),
    fullTranscript: str('full_transcript'),
  };
  if (lane.fullThinking?.trim() || liveTranscript(lane)) return null;
  return lane.fullReasoning?.trim() ? REASONING_CLIP_NOTE : null;
}

/** The inspector THINKING pane's variant: its chain (inspectorThinkingText) reaches the clip only
 *  when the durable think.log is absent. Returned pre-joined for the pane's `count` caption. */
export function thinkingClipNote(lane: StreamLane): string {
  return !lane.fullThinking?.trim() && (lane.fullReasoning?.trim() ?? '')
    ? ` · ${REASONING_CLIP_NOTE}`
    : '';
}

/**
 * ONE PANE OF THE INSPECTOR — declared at MODULE SCOPE, and that is the whole point.
 *
 * This was a `Pane` const declared INSIDE `NodeInspector`'s body. A component defined in a render body is a
 * NEW COMPONENT TYPE on every render, so React unmounted and remounted the entire subtree — at the 500 ms
 * poll (`useSwarmRun(dir, pollMs = 500)`), twice a second, forever. Every remount reset `follow` to true and
 * re-fired the jump-to-bottom, which is exactly "it jumps down but does not actively show what is being
 * generated", and no amount of fixing the TEXT could reach it because it was never in the text pipeline.
 */
const InspectorPane: React.FC<{
  title: string;
  count: string;
  empty: string;
  isEmpty: boolean;
  /** A header control (the live pane's "show all N KB" / "live tail" toggle) — beside the count. */
  action?: React.ReactNode;
  /** 'reasoning' = the thinking channel: its title carries the Studio secondary accent, the ONE surface
   *  that does. Every other pane title is the zone register in ink-2. */
  channel?: 'reasoning';
  children: React.ReactNode;
}> = ({ title, count, empty, isEmpty, action, channel, children }) => (
  <div className={cx('flex min-h-0 flex-1 flex-col overflow-hidden border border-lz-border', RADIUS.control)}>
    <div className="flex h-8 shrink-0 items-center justify-between border-b border-lz-border bg-lz-surface-2 px-3">
      <span
        className={cx(EYEBROW_CLASS, channel === 'reasoning' ? TONE_TEXT.secondary : 'text-lz-ink-2')}
        data-testid="pane-title"
      >
        {title}
      </span>
      <span className="flex items-center gap-2">
        {action ?? null}
        <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>{count}</span>
      </span>
    </div>
    <div className="min-h-0 flex-1 overflow-hidden">
      {isEmpty ? <div className="px-3 py-2 text-lz-body text-lz-ink-3">{empty}</div> : children}
    </div>
  </div>
);

/** Re-renders once a second so a running call's elapsed time moves between digest polls. */
function useSecondTick(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [active]);
  return now;
}

// A tool request whose result has not landed, as a RUNNING row: the same tool-type icon and intent verb a
// finished call gets, the engine's argument preview, a solid amber RUNNING pill with a spinner, and the time
// since the request. Mihai, watching lane service-boot: the WORK pane listed a call only after its result,
// while THINKING streamed — a two-minute write was invisible for two minutes.
const INFLIGHT_VERB: Record<string, string> = {
  write: 'Writing',
  edit: 'Editing',
  str_replace: 'Editing',
  text_editor: 'Editing',
  read: 'Reading',
  view: 'Reading',
  shell: 'Running',
  bash: 'Running',
  tree: 'Listing',
};

const InflightRow: React.FC<{ call: InflightCall; now: number }> = ({ call, now }) => {
  const m = classifyCall({ name: call.tool, summary: call.args, ok: null });
  const tool = call.tool.replace(/^developer__/, '').toLowerCase();
  const verb = INFLIGHT_VERB[tool] ?? call.tool;
  const color = SWARM_STATUS.running;
  return (
    <div
      className="py-0.5 border-b border-lz-border last:border-0"
      data-testid="inflight-row"
      data-call-id={call.id}
    >
      <div className="w-full flex items-start gap-2 text-left">
        <span className="mt-0.5">
          <CallTypeIcon icon={m.icon} color={color} />
        </span>
        <span className="flex-1 min-w-0">
          <span className="flex items-center gap-1.5 flex-wrap">
            <span className={cx('text-lz-body text-lz-ink', WEIGHT.medium)}>{verb}</span>
            <Chip tone="warn" icon={<Loader2 className="animate-spin" />}>
              running
            </Chip>
            <span className={cx('font-mono text-lz-mono tabular-nums', TONE_TEXT.warn)}>
              {elapsedSince(call.since, now)}
            </span>
          </span>
          <span className="block font-mono text-lz-mono text-lz-ink-2 break-words">{call.args}</span>
        </span>
      </div>
    </div>
  );
};

/** II-11b/c: a tool call the stream has NAMED whose argument body is still streaming. It precedes
 *  the RUNNING state (request not yet complete) and the engine removes the sidecar the moment the
 *  call completes. Since II-11c the sidecar carries `args_bytes`/`args_preview`, so the row shows
 *  the generation transforming into the call: the honest byte count and the tail of the arguments
 *  streamed so far, captioned as exactly that. A zero-byte row (a provider that ships the whole
 *  body in one terminal delta, or a call named a moment ago) stays a name, a spinner and a clock —
 *  no progress is pretended. */
const FormingRow: React.FC<{ call: FormingCall; now: number }> = ({ call, now }) => {
  const tool = call.name.replace(/^developer__/, '').toLowerCase();
  const verb = INFLIGHT_VERB[tool] ?? call.name;
  const color = SWARM_STATUS.running;
  const s = Math.max(0, Math.round((now - call.since_ms) / 1000));
  const clock = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  const bytes = call.args_bytes ?? 0;
  const preview = bytes > 0 ? call.args_preview : undefined;
  return (
    <div
      className="py-0.5 border-b border-lz-border last:border-0"
      data-testid="forming-row"
      data-call-id={call.id}
    >
      <div className="w-full flex items-start gap-2 text-left">
        <span className="mt-0.5">
          <CallTypeIcon icon="tool" color={color} />
        </span>
        <span className="flex-1 min-w-0">
          <span className="flex items-center gap-1.5 flex-wrap">
            <span className={cx('text-lz-body text-lz-ink', WEIGHT.medium)}>{verb}</span>
            <Chip tone="warn" icon={<Loader2 className="animate-spin" />}>
              forming…
            </Chip>
            <span className={cx('font-mono text-lz-mono tabular-nums', TONE_TEXT.warn)}>{clock}</span>
            {bytes > 0 ? (
              <span className={cx('font-mono text-lz-mono tabular-nums', WEIGHT.semibold, TONE_TEXT.warn)}>
                {bytes.toLocaleString()} bytes of arguments
              </span>
            ) : null}
          </span>
          {preview ? (
            <span className="block" data-testid="forming-preview">
              <span className={cx('block text-lz-meta', TONE_TEXT.warn)}>
                forming — last {preview.length} chars of the arguments so far
              </span>
              <span className="block font-mono text-lz-mono text-lz-ink-2 whitespace-pre-wrap break-words">
                {preview}
              </span>
            </span>
          ) : (
            <span className="block font-mono text-lz-mono text-lz-ink-3 break-words">
              the model is still generating this call's arguments
            </span>
          )}
        </span>
      </div>
    </div>
  );
};

/** The header suffix admitting the forming rows the body shows — a forming call is neither a
 *  resolved record nor a running request, so `workCaption`'s buckets cannot carry it. */
export function formingNote(forming: FormingCall[] | undefined): string {
  const n = forming?.length ?? 0;
  return n > 0 ? ` · ${n} forming` : '';
}

/** Forming rows sit ABOVE the running rows: a call is named before its request is complete. */
const FormingRows: React.FC<{ forming?: FormingCall[] }> = ({ forming }) => {
  const now = useSecondTick((forming?.length ?? 0) > 0);
  if (!forming || forming.length === 0) return null;
  return (
    <>
      {forming.map((c) => (
        <FormingRow key={c.id} call={c} now={now} />
      ))}
    </>
  );
};

/** The running rows, ABOVE the completed ones wherever a call list is drawn. */
const InflightRows: React.FC<{ running: InflightCall[] }> = ({ running }) => {
  const now = useSecondTick(running.length > 0);
  if (running.length === 0) return null;
  return (
    <>
      {running.map((c) => (
        <InflightRow key={c.id} call={c} now={now} />
      ))}
    </>
  );
};

/**
 * WHAT THE NODE ACTUALLY DID — the call list first, the narration second, in ONE scroller.
 *
 * The pane used to be a string: six literal "shell ok" summaries joined onto the answer log. A lane that
 * made 51 tool calls therefore displayed six words of status and a wall of blank space, under a header
 * counting 42 tool calls — the header named the work and the body showed none of it, while every record
 * needed to render it (`summary` = the command, `result` = its output) sat unread in the same object.
 *
 * NO MODE SWITCH AND NO LANE-KIND GUESS. A tool lane has ~1 character of prose, so it renders as calls; a
 * planning lane has no calls and 30 KB of prose, so it renders as prose. The window cannot guess wrong
 * because it never guesses.
 */
/** The solid provenance chip of the SAID surface — same register as the header's "being reviewed" chip.
 *  Solid saturated fill, white text; never a tint, never a left accent stripe. */
const SaidChip: React.FC<{ tone: Tone; kind: SaidKind; title?: string; children: React.ReactNode }> = ({
  tone,
  kind,
  title,
  children,
}) => (
  // The lz Chip's filled recipe, on an element that carries `data-said-kind` (the provenance tests and the
  // per-tick instrument read it off the chip itself, which the Chip primitive does not forward).
  <span
    className={cx(
      'inline-flex h-5 shrink-0 items-center whitespace-nowrap px-1.5 text-lz-meta',
      WEIGHT.semibold,
      TNUM,
      RADIUS.control,
      TONE_FILL[tone]
    )}
    title={title}
    data-said-kind={kind}
  >
    {children}
  </span>
);

/** 1-based for humans, matching the event feed's "(attempt N)" convention (`attempt + 1` at the
 *  task_dispatched verbose line). `null` is text with no marker — an earlier call on a reused lane key,
 *  or bytes that predate the provenance fields. */
const attemptLabel = (n: number | null): string => (n == null ? 'earlier call' : `attempt ${n + 1}`);

/** One retried attempt's text: a solid chip saying WHOSE it is and HOW it ended, the retry's failure
 *  text when known, and the body collapsed behind a custom toggle (it is context, not the answer). */
const SupersededSaidBlock: React.FC<{ seg: SaidSegmentView }> = ({ seg }) => {
  const [open, setOpen] = useState(false);
  const isError = seg.kind === 'error';
  return (
    <div className="mt-2" data-testid="superseded-said" data-said-kind={seg.kind}>
      <div className="flex items-center gap-2 flex-wrap">
        <SaidChip
          kind={seg.kind}
          tone={isError ? 'err' : 'stopped'}
          title={
            isError
              ? 'This attempt ended in a transport/agent error — not something the model said — and was retried.'
              : 'A newer attempt superseded this text; it is kept here as history.'
          }
        >
          {`from ${attemptLabel(seg.attempt)} · ${isError ? 'error → retried' : 'superseded'}`}
        </SaidChip>
        {seg.retried ? (
          <span className={cx('font-mono text-lz-mono', TONE_TEXT.err)}>retried: {seg.retried}</span>
        ) : null}
        <button
          type="button"
          className={cx('text-lz-meta text-lz-ink-3 underline hover:text-lz-ink', MOTION)}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? 'hide' : 'show'}
        </button>
      </div>
      {open ? (
        <div
          className={cx(
            'mt-1 whitespace-pre-wrap break-words font-mono text-lz-mono',
            isError ? TONE_TEXT.err : 'text-lz-ink-2'
          )}
        >
          {squeezeBlankRuns(seg.text)}
        </div>
      ) : null}
    </div>
  );
};

/**
 * THE SAID SURFACE WITH ITS STATE. The owner, watching r0's ledger-core-tests: attempt 0's "Network
 * error … Please resend your message" stood in this pane for 24+ minutes while attempt 1 ran — "I can't
 * see if it's current or happened and resolved, there's no state to any of this." So the pane now says
 * WHOSE text it is showing: a LIVE chip for the current attempt (red when the text is an agent error,
 * green otherwise), an explicit "processing the prompt…" body while the new attempt has said nothing —
 * never the dead attempt's error — and each superseded attempt collapsed below with its own chip. A lane
 * with no provenance (a legacy run) renders exactly the old body, chipless.
 */
export const SaidSection: React.FC<{ said: SaidState; narration: string; processing: boolean }> = ({
  said,
  narration,
  processing,
}) => {
  const hasProvenance = said.live.attempt != null || said.superseded.length > 0;
  const liveIsError = said.live.kind === 'error';
  return (
    <div data-testid="said-section">
      {hasProvenance ? (
        <div className="flex items-center gap-2 mb-1">
          <SaidChip
            kind={said.live.kind}
            tone={liveIsError ? 'err' : 'ok'}
            title={
              liveIsError
                ? 'The current attempt ended in a transport/agent error — this text is not the model’s answer.'
                : 'This text belongs to the attempt that is live right now.'
            }
          >
            {`${attemptLabel(said.live.attempt)} · ${liveIsError ? 'error' : 'live'}`}
          </SaidChip>
        </div>
      ) : null}
      {narration.length > 0 ? (
        <div
          className={cx(
            'whitespace-pre-wrap break-words font-mono text-lz-mono',
            liveIsError ? TONE_TEXT.err : 'text-lz-ink-2'
          )}
        >
          {narration}
        </div>
      ) : processing && hasProvenance ? (
        <div className="text-lz-meta italic text-lz-ink-3">processing the prompt…</div>
      ) : null}
      {said.superseded.map((seg, i) => (
        <SupersededSaidBlock key={`${seg.attempt ?? 'x'}-${i}`} seg={seg} />
      ))}
    </div>
  );
};

const WorkPane: React.FC<{
  calls: SwarmCall[];
  running: InflightCall[];
  forming?: FormingCall[];
  toolCalls?: number;
  narration: string;
  said: SaidState;
  processing: boolean;
}> = ({ calls, running, forming, toolCalls, narration, said, processing }) => {
  const meta = callRowMeta(calls, toolCalls);
  const attention = firstCallNeedingAttention(calls);
  const hasSaid = narration.length > 0 || said.superseded.length > 0 || said.live.attempt != null;
  return (
    <FollowScroll
      dep={`${forming?.length ?? 0}:${running.length}:${calls.length}:${narration.length}:${said.superseded.length}`}
      className="px-3 py-2"
    >
      {calls.length > 0 || running.length > 0 || (forming?.length ?? 0) > 0 ? (
        <div>
          <FormingRows forming={forming} />
          <InflightRows running={running} />
          {calls.map((c, i) => (
            <CallRow
              key={meta[i].key}
              ordinal={meta[i].ordinal}
              call={c}
              defaultOpen={i === attention}
            />
          ))}
        </div>
      ) : null}
      {hasSaid ? (
        <div>
          {calls.length > 0 || running.length > 0 ? (
            <div className="mt-3 pt-2 border-t border-lz-border text-lz-meta text-lz-ink-3">
              Said
            </div>
          ) : null}
          <SaidSection said={said} narration={narration} processing={processing} />
        </div>
      ) : null}
    </FollowScroll>
  );
};

/** The small solid header-control button both inspector panes and history rows share. */
const PaneActionButton: React.FC<{
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}> = ({ label, onClick, children }) => (
  <Button
    variant="secondary"
    size="sm"
    className="h-6 px-2 text-lz-meta"
    onClick={(e) => {
      e.stopPropagation();
      onClick();
    }}
    aria-label={label}
  >
    {children}
  </Button>
);

/**
 * One channel of an expanded history entry: the WHOLE durable file, loaded on expand.
 *
 * The caption is the workCaption law applied to a full read: "all N bytes" states that the body is the
 * entire durable log (blank-run collapses admitted by squeezeNote), where every live surface can only
 * ever say "last N of M". A null log renders the NAMED absence — a missing file and a failed read are
 * different facts and say different things.
 */
const HistoryChannelPane: React.FC<{
  title: string;
  log: { text: string; bytes: number } | null;
  failed: boolean;
  empty: string;
  channel?: 'reasoning';
}> = ({ title, log, failed, empty, channel }) => {
  const squeezed = useMemo(() => squeezeBlankRuns(log?.text ?? ''), [log]);
  return (
    <InspectorPane
      title={title}
      channel={channel}
      count={log ? `all ${log.bytes.toLocaleString()} bytes${squeezeNote(log.text, squeezed)}` : ''}
      empty={
        failed
          ? 'Reading the durable log FAILED — not the same as it not existing. Retry by collapsing and expanding this entry.'
          : empty
      }
      isEmpty={!log || squeezed.length === 0}
    >
      <NodeExpandBox text={squeezed} fill jumpToStart />
    </InspectorPane>
  );
};

/**
 * ONE FINISHED CALL, FOLDED TO A LINE — Mihai's "when it finishes a phase it folds it into a line
 * visually and then moves on, and this way we can have a proper log all in a place".
 *
 * The folded line's sizes are the DURABLE byte sizes (`thinkingBytes` / `transcriptBytes`, the true
 * file sizes main.ts stats beside every digest) — never `thinkingChars`, which is a per-stream counter
 * that RESETS on a restream: the r5 opener measured 38,780 stream chars against 128,270 durable bytes,
 * and captioning the counter as the size claims two-thirds of the record does not exist.
 *
 * Expanding reads the WHOLE durable pair over `read-swarm-activity-log` — the on-demand IPC, never the
 * poll — and RELEASES it on collapse. That is the whole memory strategy: a durable log is hundreds of
 * KB and a run has dozens of lanes, so holding every expanded transcript would pin megabytes in
 * renderer state for no benefit, while a re-expand is one IPC read of a local file the OS page cache
 * already holds. No LRU — the cache an LRU would manage already exists in the filesystem.
 */
const NodeHistoryRow: React.FC<{ entry: NodeHistoryEntry; runDir: string }> = ({
  entry,
  runDir,
}) => {
  const [open, setOpen] = useState(false);
  const [logs, setLogs] = useState<{
    think: { text: string; bytes: number } | null;
    said: { text: string; bytes: number } | null;
  } | null>(null);
  const [failed, setFailed] = useState(false);
  const lane = entry.lane;
  const title = cleanTaskTitle(lane.description ?? lane.taskId, lane.taskId);
  // Per LANE KEY, not per round: REVIEW reuses one key every round, so the durable files span every
  // call the key ever ran and the line must say so instead of claiming to be one call.
  const priorCalls = lane.superseded?.length ?? 0;
  const durationLabel = (() => {
    if (typeof lane.elapsedMs === 'number') return fmtDuration(lane.elapsedMs / 60000);
    const started = lane.dispatchedAt ? Date.parse(lane.dispatchedAt) : NaN;
    if (!Number.isNaN(started) && entry.lastWriteMs != null && entry.lastWriteMs > started) {
      // dispatchedAt is stamped per ATTEMPT, so on a reused lane this spans only the last call.
      return `${fmtDuration((entry.lastWriteMs - started) / 60000)}${priorCalls > 0 ? ' (last call)' : ''}`;
    }
    return null;
  })();
  const sizes: string[] = [];
  if (typeof lane.thinkingBytes === 'number')
    sizes.push(`thought ${lane.thinkingBytes.toLocaleString()} B`);
  if (typeof lane.transcriptBytes === 'number')
    sizes.push(`said ${lane.transcriptBytes.toLocaleString()} B`);
  const toggle = () => {
    if (open) {
      setOpen(false);
      setLogs(null);
      setFailed(false);
      return;
    }
    setOpen(true);
    setFailed(false);
    void Promise.all([
      window.electron.readSwarmActivityLog(runDir, lane.taskId, 'thinking'),
      window.electron.readSwarmActivityLog(runDir, lane.taskId, 'transcript'),
    ])
      .then(([think, said]) => setLogs({ think, said }))
      .catch(() => {
        setFailed(true);
        setLogs({ think: null, said: null });
      });
  };
  return (
    <div
      className={cx('border border-lz-border', RADIUS.control)}
      data-testid="node-history-entry"
      data-task={lane.taskId}
    >
      <div
        role="button"
        tabIndex={0}
        aria-expanded={open}
        aria-label={`${open ? 'Collapse' : 'Expand'} the full durable log of ${title}`}
        data-testid="node-history-row"
        className={cx('flex min-h-8 cursor-pointer items-center gap-2 px-2 py-1 text-lz-body', SURFACE.hover, MOTION)}
        onClick={toggle}
        onKeyDown={(e: React.KeyboardEvent) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            toggle();
          }
        }}
      >
        {/* INTERRUPTED is not FAILED: the digest went quiet without a completion stamp — a liveness
            fact, not an engine verdict — and painting it as a failure would claim a verdict nobody
            reached. The stopped slate, its own word. */}
        <Chip tone={lane.interrupted ? 'stopped' : lane.status === 'error' ? 'err' : 'ok'}>
          {lane.interrupted ? 'interrupted' : lane.status === 'error' ? 'failed' : 'finished'}
        </Chip>
        <span className="min-w-0 flex-1 truncate text-lz-ink">{title}</span>
        {lane.interrupted ? (
          <span className={cx('shrink-0 text-lz-meta', TONE_TEXT.stopped)}>
            went quiet mid-call — no completion stamp
          </span>
        ) : null}
        {(() => {
          // The judge's ROLLING lane: superseded entries are its earlier LOOKS, so "N calls on this
          // lane" would be true but say less than the honest rolling caption.
          const rolling = supervisionRollingCaption(lane);
          if (rolling)
            return <span className="shrink-0 text-lz-meta text-lz-ink-3">{rolling}</span>;
          return priorCalls > 0 ? (
            <span className="shrink-0 text-lz-meta text-lz-ink-3">
              {priorCalls + 1} calls on this lane
            </span>
          ) : null;
        })()}
        {durationLabel ? (
          <span className={cx('shrink-0 text-lz-meta text-lz-ink-3', TNUM)}>{durationLabel}</span>
        ) : null}
        <span className={cx('shrink-0 font-mono text-lz-mono text-lz-ink-3', TNUM)}>
          {sizes.length > 0 ? sizes.join(' · ') : 'no durable log'}
        </span>
        {open ? (
          <ChevronDown size={12} className="shrink-0 text-lz-ink-3" />
        ) : (
          <ChevronRight size={12} className="shrink-0 text-lz-ink-3" />
        )}
      </div>
      {open ? (
        logs == null ? (
          <div className="px-3 py-2 text-lz-body text-lz-ink-3">Reading the durable logs…</div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-2 p-2" style={{ height: 340 }}>
            <HistoryChannelPane
              title="Thinking"
              channel="reasoning"
              log={logs.think}
              failed={failed}
              empty="No .think.log on disk — this call never wrote the reasoning channel."
            />
            <HistoryChannelPane
              title="Said"
              log={logs.said}
              failed={failed}
              empty="No .log on disk — this call never wrote the answer channel."
            />
          </div>
        )
      ) : null}
    </div>
  );
};

const NodeInspector: React.FC<{
  device: string;
  letter: string;
  /** The node's ramp slot (deviceOrder index) — identity only. */
  index: number;
  lane?: TurnLane;
  nodeState?: string;
  /** Every FINISHED call this node ran this run (deriveNodeHistory) — the cumulative folded log. */
  history: NodeHistoryEntry[];
  /** The RESOLVED run dir (readSwarmRun's `dir`) — where the on-demand full-log reads aim. */
  runDir: string;
  onClose: () => void;
}> = ({ device, letter, index, lane, nodeState, history, runDir, onClose }) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // THE LIVE PANE'S "SHOW ALL" SNAPSHOT — the whole durable think.log, read once on click over the
  // on-demand IPC. The poll keeps handing this modal a bounded tail (main.ts reads the last 400k
  // bytes; the repeat scan takes the last 24k chars), so the BEGINNING of a long call was
  // unreachable from here — the r5 opener's 128,270-byte log started partway through forever.
  // `taskId` rides in the snapshot so a different lane opened in the same modal can never inherit it.
  const [fullThink, setFullThink] = useState<{ taskId: string; text: string; bytes: number } | null>(
    null
  );
  const [fullThinkFailed, setFullThinkFailed] = useState(false);

  // PREFER THE DURABLE THINKING LOG. `fullReasoning` is built from the ANSWER channel and the digest's
  // thinking is a 2,400-char rolling window, so without this the pane clears and refills as the model
  // streams instead of accumulating a readable document.
  // NEVER CONCATENATE THE LOG WITH THE ROLLING WINDOW — THEY ARE THE SAME STREAM.
  //
  // `fullThinking` is the durable `<task>.think.log`; `lastThinking` is the digest's 2,400-char ROLLING
  // WINDOW over that same stream, so the window is a SUFFIX of the log by construction. Appending it
  // rendered every lane's reasoning TWICE.
  //
  // Mihai caught it on `slice-ledgerd-core`: the same ~2,000-char block shown twice, the second copy
  // starting mid-word at "me analyze my slice" — the window's cut head. The engine had one copy
  // (thinking_chars 2,003, think.log 2,009 bytes, recur_rate 0.0), so it looked like a MODEL loop and was
  // a render bug. And the duplication is what made the pane look truncated: double the text overflows,
  // the box auto-scrolls to the bottom, and the real beginning is pushed out of view.
  //
  // The window is only a FALLBACK, for a lane whose durable log has not appeared yet.
  // BLANK RUNS ARE AN ARTIFACT OF STREAM CHUNKING, on both channels — see squeezeBlankRuns. The measured
  // lane was 123 blank lines out of 143, rendered verbatim by whitespace-pre-wrap.
  // FINDING 19: bounded + memoized like ReasoningBlock — this feeder is the 400KB think.log tail, the
  // exact input measured at 354ms per scan. Length is identity on an append-only channel; the lane key
  // rides the deps so another lane's equal length can never serve a stale collapse.
  const thinkSource = inspectorThinkingText(lane ?? {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const rawThink = useMemo(
    () => collapseRepeats(tailOf(thinkSource, REPEAT_SCAN_CHARS)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [lane?.taskId, thinkSource.length]
  );
  const thinkText = squeezeBlankRuns(rawThink);
  const showingFullThink = fullThink != null && fullThink.taskId === lane?.taskId;
  const fullThinkSqueezed = useMemo(
    () => (fullThink ? squeezeBlankRuns(fullThink.text) : ''),
    [fullThink]
  );
  // The control renders only when there IS more than the body shows: a durable log main.ts clipped,
  // or one longer than the 24k repeat-scan bound. A "show all" over a body already showing all of it
  // would claim hidden text that does not exist.
  const thinkShowAll =
    !!lane &&
    typeof lane.thinkingBytes === 'number' &&
    lane.thinkingBytes > 0 &&
    (isClippedTail(lane.fullThinking, lane.thinkingBytes) ||
      thinkSource.length > REPEAT_SCAN_CHARS);
  const loadFullThink = () => {
    if (!lane) return;
    const taskId = lane.taskId;
    setFullThinkFailed(false);
    void window.electron
      .readSwarmActivityLog(runDir, taskId, 'thinking')
      .then((log) => {
        if (log) setFullThink({ taskId, ...log });
        else setFullThinkFailed(true);
      })
      .catch(() => setFullThinkFailed(true));
  };
  // THE WORK PANE'S SYMMETRIC "SHOW ALL" — the same on-demand IPC, aimed at `<task>.log`. The poll
  // hands this pane a 200k-byte TAIL (main.ts MAX), so a long call's answer channel had the same
  // unreachable beginning the thinking pane was fixed for, with no way to ask for the rest.
  const [fullWork, setFullWork] = useState<{ taskId: string; text: string; bytes: number } | null>(
    null
  );
  const [fullWorkFailed, setFullWorkFailed] = useState(false);
  const showingFullWork = fullWork != null && fullWork.taskId === lane?.taskId;
  const fullWorkSqueezed = useMemo(
    () => (fullWork ? squeezeBlankRuns(fullWork.text) : ''),
    [fullWork]
  );
  // Same honesty rule as thinkShowAll: offer "show all" only when main.ts says the tail IS a tail —
  // `transcriptClipped` is its explicit answer, `isClippedTail` the byte comparison for older payloads.
  const workShowAll =
    !!lane &&
    typeof lane.transcriptBytes === 'number' &&
    lane.transcriptBytes > 0 &&
    isClippedTail(lane.fullTranscript, lane.transcriptBytes, lane.transcriptClipped);
  const loadFullWork = () => {
    if (!lane) return;
    const taskId = lane.taskId;
    setFullWorkFailed(false);
    void window.electron
      .readSwarmActivityLog(runDir, taskId, 'transcript')
      .then((log) => {
        if (log) setFullWork({ taskId, ...log });
        else setFullWorkFailed(true);
      })
      .catch(() => setFullWorkFailed(true));
  };
  const { completed: calls, running, tallies } = workRows(lane?.calls, lane?.inflight);
  const rawNarration = inspectorOutputText(lane ?? {});
  const narration = squeezeBlankRuns(rawNarration);
  // SAID provenance: whose text the pane is showing, and what a retry superseded. A retried lane whose
  // new attempt has said nothing yet has narration === '' AND a superseded list — the pane must render
  // the list (with the old attempt's error labeled as such), not collapse.
  const said = laneSaidState(lane ?? {});
  const saidProcessing = !!lane && lane.status === 'running';
  // THE PREDICATE HAD TO MOVE IN THE SAME COMMIT AS THE `recent` REMOVAL. With `recent` gone from
  // `inspectorOutputText`, the measured lane (60 calls, last_text one character) yields narration === '',
  // and the old `outText` grid predicate would collapse the column and take all 60 calls with it.
  // A FORMING call is work: without it the pane says "Still thinking" over the exact moment the model
  // is generating a tool call — r5's OPEN, visually frozen for 5 minutes while 28 KB of arguments
  // streamed.
  const hasWork =
    calls.length > 0 ||
    running.length > 0 ||
    (lane?.forming?.length ?? 0) > 0 ||
    narration.length > 0 ||
    said.superseded.length > 0;
  // The lane on top is live estate; a finished lane opened FROM history renders up there through the
  // exact live-pane machinery, so its own folded line would be a duplicate row below itself.
  const shownHistory = history.filter((h) => h.lane.taskId !== lane?.taskId);

  return createPortal(
    <>
      <div className="fixed inset-0 z-40 bg-black/60" onClick={onClose} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Node ${letter}, ${device}`}
        data-task={lane?.taskId ?? ''}
        className={cx('fixed inset-4 z-50 flex flex-col text-lz-body md:inset-8', SURFACE.overlay)}
      >
        <div className="flex h-12 shrink-0 items-center gap-2 border-b border-lz-border px-4">
          <NodeDot index={index} letter={letter} size={10} />
          <span className={cx('font-mono text-lz-mono text-lz-ink', WEIGHT.medium)}>{device}</span>
          {nodeState && (
            <Chip
              tone={
                nodeState === 'generating' ? 'ok' : nodeState === 'processingPrompt' ? 'warn' : 'stopped'
              }
            >
              {nodeState === 'processingPrompt' ? 'processing prompt' : nodeState}
            </Chip>
          )}
          {/* A SECOND MODEL IS READING THIS CALL. It used to also mean "the counters below are frozen",
              because the engine buffered the worker's stream during a probe — it no longer does, so the
              lane keeps moving while this is up and the badge is context, not an excuse for stillness.
              Next to GENERATING it read as a contradiction; it says who is doing what now. */}
          {lane?.judging && (
            <Chip
              tone="warn"
              title="A supervisor model is reading this call's reasoning to decide whether to redirect it. The worker keeps running."
            >
              {'being reviewed'}
            </Chip>
          )}
          {/* THE LANE'S OWN END, event-driven: task_completed/failed or the digest's phase stamp.
              Since the modal no longer clears when a phase ends, a reader mid-scroll needs the fact
              that the call under them ENDED stated, not implied by counters going still. */}
          {lane && lane.status !== 'running' && (
            <span className="inline-flex shrink-0" data-testid="inspector-lane-ended">
              <Chip tone={lane.status === 'done' ? 'ok' : 'err'}>
                {lane.status === 'done' ? 'finished' : 'failed'}
              </Chip>
            </span>
          )}
          {/* An r6 supervision lane says so — the class's solid violet, and for the judge's rolling
              lane the honest semantics: look N (1-based), earlier looks folded into superseded. */}
          {lane?.supervision === true && (
            <span className="inline-flex shrink-0" data-testid="inspector-supervision">
              <Chip tone="accent">supervision</Chip>
            </span>
          )}
          {(() => {
            const rolling = lane ? supervisionRollingCaption(lane) : null;
            return rolling ? (
              <span className={cx('shrink-0 text-lz-meta', TONE_TEXT.accent, WEIGHT.semibold)}>{rolling}</span>
            ) : null;
          })()}
          {lane?.description && (
            <span className="truncate text-lz-meta text-lz-ink-3">{lane.description}</span>
          )}
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto"
            onClick={onClose}
            aria-label="Close"
            icon={<X />}
          />
        </div>

        {/* ONE PANE WHEN THERE IS ONE THING TO SHOW.
            A fixed 50/50 split meant the whole OPEN and RESEARCH stretch -- where the model does nothing
            but reason -- rendered half this modal as a dead box captioned "Nothing emitted yet", while
            the reasoning it WAS producing was squeezed into the other half. Mihai, having opened it to
            watch a node work: "what is generating cause I can't see shit in it". Output earns its column
            when it has something in it. */}
        {/* An idle node with only history skips the live grid — the history IS the content then. */}
        {lane || shownHistory.length === 0 ? (
        <div
          className={`flex-1 min-h-0 grid gap-3 p-3 ${
            hasWork ? 'grid-cols-1 lg:grid-cols-2' : 'grid-cols-1'
          }`}
        >
          <InspectorPane
            title="Thinking"
            channel="reasoning"
            // COUNT WHAT IS ON SCREEN, and say so when it is a clipped tail. The header used to report
            // the engine's `thinkingChars` while the body rendered something else entirely, so a pane
            // showing 2,000 characters could be captioned 22,150 — which reads as "the UI is hiding
            // things" and, before the lane-path fix, actually was.
            //
            // `thinkingChars` IS NOT THE DENOMINATOR. It is the engine's per-stream counter and resets on
            // a re-stream, so it says nothing about the size of `<task>.think.log`; the only number that
            // does is `thinkingBytes`, which main.ts has attached to every digest and nothing has read.
            count={
              showingFullThink
                ? `all ${fullThink.bytes.toLocaleString()} bytes${squeezeNote(fullThink.text, fullThinkSqueezed)}`
                : `${thinkingCaption(
                    thinkText,
                    lane?.fullThinking,
                    lane?.thinkingBytes,
                    lane?.thinkingChars
                  )}${thinkingClipNote(lane ?? {})}${squeezeNote(rawThink, thinkText)}`
            }
            action={
              showingFullThink ? (
                <PaneActionButton
                  label="Back to the live tail of the reasoning channel"
                  onClick={() => setFullThink(null)}
                >
                  live tail
                </PaneActionButton>
              ) : thinkShowAll ? (
                <PaneActionButton
                  label={`Show the whole ${(lane?.thinkingBytes ?? 0).toLocaleString()}-byte reasoning log`}
                  onClick={loadFullThink}
                >
                  {fullThinkFailed
                    ? 'read failed — retry'
                    : `show all ${Math.max(1, Math.round((lane?.thinkingBytes ?? 0) / 1024)).toLocaleString()} KB`}
                </PaneActionButton>
              ) : null
            }
            empty="Nothing on the reasoning channel yet — the node has been dispatched but has not produced a token."
            isEmpty={showingFullThink ? !fullThinkSqueezed : !thinkText}
          >
            <NodeExpandBox
              text={showingFullThink ? fullThinkSqueezed : thinkText}
              fill
              jumpToStart={showingFullThink}
            />
          </InspectorPane>
          {/* OUTPUT EARNS ITS COLUMN — and its pane. The grid already collapsed to one column when the
              answer channel had nothing, but the empty Work pane still rendered STACKED under Thinking:
              a dead box captioned "Still thinking" spending half the reasoning's vertical estate. With
              nothing to show it renders nothing; the pane (and its show-all action) appears with the
              first call/forming/narration byte. showingFullWork keeps it mounted while the full durable
              log is open, and workShowAll keeps the door to a durable `<task>.log` that HAS bytes on
              disk even when the live tail squeezed to nothing (the durable channel outranks the tail). */}
          {(hasWork || showingFullWork || workShowAll) && (
          <InspectorPane
            title="Work"
            // THE HEADER STOPS CONFLATING TWO NUMBERS. `tool_calls` counts RESOLVED records; `calls` is the
            // last 60 of those plus the in-flight ones, so the two are not the same quantity and the header
            // said one while the body showed the other. workCaption labels the difference ("last 60 of 69")
            // and every other figure it prints is over the rows actually on screen.
            //
            // SAY WHEN THE TRANSCRIPT IS A TAIL — main.ts already computed the answer, in the one place
            // that knows both the file size and the budget it read with. See streamTailNote.
            count={
              showingFullWork
                ? `all ${fullWork.bytes.toLocaleString()} bytes${squeezeNote(fullWork.text, fullWorkSqueezed)}`
                : `${workCaption(calls.length + running.length, lane?.toolCalls, tallies)}${formingNote(lane?.forming)}${streamTailNote(lane?.fullTranscript, lane?.transcriptBytes, lane?.transcriptClipped)}${squeezeNote(rawNarration, narration)}`
            }
            action={
              showingFullWork ? (
                <PaneActionButton
                  label="Back to the live view of the answer channel"
                  onClick={() => setFullWork(null)}
                >
                  live view
                </PaneActionButton>
              ) : workShowAll ? (
                <PaneActionButton
                  label={`Show the whole ${(lane?.transcriptBytes ?? 0).toLocaleString()}-byte answer log`}
                  onClick={loadFullWork}
                >
                  {fullWorkFailed
                    ? 'read failed — retry'
                    : `show all ${Math.max(1, Math.round((lane?.transcriptBytes ?? 0) / 1024)).toLocaleString()} KB`}
                </PaneActionButton>
              ) : null
            }
            empty={
              thinkText
                ? 'Still thinking — this fills with tool calls and written text once it starts acting.'
                : 'Nothing yet on either channel.'
            }
            isEmpty={showingFullWork ? !fullWorkSqueezed : !hasWork}
          >
            {showingFullWork ? (
              // The raw durable `<task>.log`, whole — attempt markers and all. The structured call/said
              // view has no full-file mode; this is the honest one: the bytes on disk, captioned as such.
              <NodeExpandBox text={fullWorkSqueezed} fill jumpToStart />
            ) : (
              <WorkPane
                calls={calls}
                running={running}
                forming={lane?.forming}
                toolCalls={lane?.toolCalls}
                narration={narration}
                said={said}
                processing={saidProcessing}
              />
            )}
          </InspectorPane>
          )}
        </div>
        ) : null}

        {/* THE CUMULATIVE LOG — every finished call this node ran this run, folded to a line each.
            This section is why NOTHING CLEARS ON PHASE END any more: a lane that finishes drops out
            of the live maps above but lands here, discovered from the digests and durable logs that
            persist on disk, and expands back into its whole transcript on demand. */}
        {shownHistory.length > 0 ? (
          <div
            data-testid="node-history"
            className={cx(
              'flex min-h-0 flex-col border-t border-lz-border',
              lane ? 'max-h-[45%] shrink-0' : 'flex-1'
            )}
          >
            <div className="flex h-8 shrink-0 items-center justify-between px-4">
              <span className={cx(EYEBROW_CLASS, 'text-lz-ink-2')} data-testid="pane-title">
                {lane ? 'Earlier calls on this node' : 'Calls this node ran'}
              </span>
              <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
                {shownHistory.length} finished
              </span>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-3 space-y-1">
              {shownHistory.map((h) => (
                <NodeHistoryRow key={h.lane.taskId} entry={h} runDir={runDir} />
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </>,
    document.body
  );
};

/** The FLEET table's column header — the lz DataTable's zone register, mirrored here because every row
 *  carries the per-tick instrument's anchors (`data-testid="fleet-node"`, data-device/-task/-expandable/
 *  -gen-len) on the ROW element itself, which the DataTable owns for its own row testid. Same classes,
 *  same look, same rhythm. */
const FLEET_TH = 'whitespace-nowrap px-3 text-left align-middle text-lz-zone uppercase text-lz-ink-3';

/** Thinking volume for the numeric column: characters, k-scaled past a thousand. */
function fmtChars(n: number | undefined): string {
  if (typeof n !== 'number' || n <= 0) return '';
  if (n < 1000) return String(n);
  return n < 10_000 ? `${(n / 1000).toFixed(1)}k` : `${Math.round(n / 1000)}k`;
}

/** The node's STATE for the fleet table — ONE dot tone and one word, from the same facts the old glyph
 *  read: a supervising lane is the accent (the judge acts on the run), a worker lane the warn amber of
 *  running work, a node LM Studio reports busy with no lane on disk is the ok green of a live generation,
 *  and a dead/ended run freezes every node on the stopped slate — never a spinner on a non-live run. */
function fleetNodeState(
  lane: TurnLane | undefined,
  live: boolean,
  lmsState: string | undefined
): { tone: Tone; label: string; live: boolean } {
  if (!live) return { tone: 'stopped', label: lane ? 'stopped' : 'idle', live: false };
  if (lane) {
    if (lane.phase === 'supervision' || lane.supervision === true)
      return { tone: 'accent', label: 'supervising', live: true };
    if (lane.phase === 'processing') return { tone: 'warn', label: 'processing prompt', live: true };
    return { tone: 'warn', label: 'working', live: true };
  }
  if (lmsState === 'generating' || lmsState === 'processingPrompt')
    return { tone: 'ok', label: 'generating', live: true };
  return { tone: 'stopped', label: 'idle', live: false };
}

/** LM Studio's OWN word for a node (lms ps --json), independent of goose's digest. */
function lmsStateLabel(st: string): { tone: Tone; label: string } {
  return st === 'generating'
    ? { tone: 'ok', label: 'generating' }
    : st === 'processingPrompt'
      ? { tone: 'warn', label: 'processing prompt' }
      : { tone: 'stopped', label: 'idle' };
}

const FleetStrip: React.FC<{
  /** Every node the run's RESOLVED POOL carries (idle ones included) + any lane device — see deriveFleet. */
  deviceOrder: string[];
  /** node -> its live lane (task lifecycle, open activity digest, or supervision span), from deriveFleet. */
  runningByDevice: Map<string, TurnLane>;
  /** The node's OTHER live lanes. Nodes run PARALLEL: 2, so this is routinely non-empty. */
  alsoRunningByDevice: Map<string, TurnLane[]>;
  live: boolean;
  dev: boolean;
  /** LM Studio's own live status per node short-name (generating/processingPrompt/idle), for the truth dot. */
  nodeStatus: Record<string, string>;
  /** Open supervision spans deriveFleet could not pin to a busy node — still shown, never dropped. */
  unattributed: SupervisionSpan[];
  /** node -> every finished call it ran this run (deriveNodeHistory) — the inspector's cumulative log. */
  historyByDevice: Map<string, NodeHistoryEntry[]>;
  /** The RESOLVED run dir, for the inspector's on-demand full-log reads. */
  runDir: string;
}> = ({
  deviceOrder,
  runningByDevice,
  alsoRunningByDevice,
  live,
  dev,
  nodeStatus,
  unattributed,
  historyByDevice,
  runDir,
}) => {
  // The full stream opens in a MODAL. Inline it was clipped by whatever height the row happened to have,
  // which made the panel least readable exactly when a node was busiest.
  // WHICH LANE, not just which node. A node runs PARALLEL: 2, so "open gabee" is ambiguous — the primary
  // cell and the sibling row under it each open the inspector on THEIR task (measured r1 t+20m: the run's
  // largest lane, open-coverage-1 at 23,975 reasoning chars, sat under gabee's cell and could not be opened).
  const [inspect, setInspect] = useState<{ device: string; taskId: string } | null>(null);
  if (deviceOrder.length === 0) return null;
  const shortName = (device: string): string => device.match(/^([^-]+)/)?.[1] ?? device;
  return (
    <div className="bg-lz-surface">
      <div className="w-full overflow-x-auto">
        <table className="w-full border-collapse" aria-label="Fleet">
          <thead>
            <tr className="h-8">
              <th scope="col" className={FLEET_TH} style={{ width: 148 }}>
                Node
              </th>
              <th scope="col" className={FLEET_TH} style={{ width: 160 }}>
                State
              </th>
              <th scope="col" className={FLEET_TH}>
                Working on
              </th>
              <th scope="col" className={cx(FLEET_TH, 'text-right')} style={{ width: 72 }}>
                Calls
              </th>
              <th scope="col" className={cx(FLEET_TH, 'text-right')} style={{ width: 88 }}>
                Thinking
              </th>
            </tr>
          </thead>
          <tbody>
            {deviceOrder.map((device, i) => {
              const letter = String.fromCharCode(65 + (i % 26));
              const short = shortName(device);
              const lane = runningByDevice.get(device);
              // WHAT THIS NODE IS GENERATING RIGHT NOW. The chain lives in `laneLiveLine`, which reads the
              // DURABLE logs first: this row was built from `reasoning`/`lastThinking`/`lastText`, every one of
              // them a digest window the engine rewrites in place, so the cell CLEARED AND REFILLED instead of
              // advancing — Mihai's "the output rolls", fixed in the expanded lane view and left standing in
              // the surface he looks at first. The markers below stay here: they are lane STATE, not stream text.
              const liveGen =
                (lane ? laneLiveLine(lane) : '') ||
                (lane?.phase === 'processing' ? 'processing the prompt…' : '') ||
                (lane?.status === 'running' ? 'generating…' : '');
              const fullGen = fleetExpandText(lane);
              // A node with FINISHED calls is expandable even between tasks: the inspector is the cumulative
              // log now, so an idle node no longer means "nothing to open".
              const nodeHistory = historyByDevice.get(device) ?? [];
              const canExpand = (!!lane && fullGen.length > 0) || nodeHistory.length > 0;
              const lmsState = nodeStatus[short];
              const state = fleetNodeState(lane, live, lmsState);
              const supervising = !!lane && (lane.phase === 'supervision' || lane.supervision === true);
              const openPrimary = () => setInspect({ device, taskId: lane?.taskId ?? '' });
              const siblings = lane ? (alsoRunningByDevice.get(device) ?? []) : [];
              return (
                // THE INSTRUMENT'S ANCHOR. The per-tick frontend check reads the RENDERED lane text and the
                // RENDERED clickability off these attributes; without them it fell back to re-deriving both
                // from the IPC payload and reported a healthy render path while the renderer was dropping
                // every field. `data-task` is what joins a cell to its digest on the SAME object, so the
                // instrument compares a lane against its own data and never against a neighbour's.
                // The ROW is the click target (hover is the solid surface-2 step); the control inside the
                // task cell carries the accessible name and the keyboard, so `data-expandable` IS the
                // rendered clickability.
                <tr
                  key={device}
                  data-testid="fleet-node"
                  data-device={device}
                  data-task={lane?.taskId ?? ''}
                  data-expandable={canExpand ? 'true' : 'false'}
                  data-gen-len={fullGen.length}
                  className={cx(
                    'border-t align-top',
                    SURFACE.hairline,
                    canExpand && cx('cursor-pointer', SURFACE.hover),
                    MOTION
                  )}
                  onClick={canExpand ? openPrimary : undefined}
                >
                  <td className="px-3 py-2 align-top">
                    <span className="flex h-5 items-center gap-2">
                      <NodeDot index={i} letter={letter} />
                      <span className={cx('truncate font-mono text-lz-mono text-lz-ink', WEIGHT.medium)}>
                        {short}
                      </span>
                    </span>
                  </td>
                  <td className="px-3 py-2 align-top">
                    <span className="flex h-5 items-center gap-2 text-lz-body text-lz-ink-2">
                      <StatusDot tone={state.tone} live={state.live} label={state.label} />
                      <span className="truncate">{state.label}</span>
                      {/* LM Studio's OWN live state for this node (lms ps --json), independent of goose's
                          digest — the ground-truth "is it generating right now" Mihai asked for. A second
                          dot: green generating, amber prompt-processing, slate idle; nothing when LM Studio
                          is unreachable or the fleet display is off. */}
                      {lmsState
                        ? (() => {
                            const lms = lmsStateLabel(lmsState);
                            return (
                              <Tip label={`LM Studio: ${lms.label}`}>
                                <span className="inline-flex">
                                  <StatusDot tone={lms.tone} label={`LM Studio: ${lms.label}`} />
                                </span>
                              </Tip>
                            );
                          })()
                        : null}
                    </span>
                  </td>
                  <td className="min-w-0 px-3 py-2 align-top text-lz-body">
                    {lane ? (
                      <div className="min-w-0">
                        <div
                          className="min-w-0"
                          role={canExpand ? 'button' : undefined}
                          tabIndex={canExpand ? 0 : undefined}
                          aria-label={canExpand ? `Open the full stream from ${short}` : undefined}
                          onKeyDown={
                            canExpand
                              ? (e: React.KeyboardEvent) => {
                                  if (e.key === 'Enter' || e.key === ' ') {
                                    e.preventDefault();
                                    openPrimary();
                                  }
                                }
                              : undefined
                          }
                        >
                          <div className="flex min-h-5 items-center gap-1.5">
                            {/* SUPERVISION work is visually its own class: the accent gavel, never the
                                amber build spinner — a supervising node says what it is really doing
                                instead of reading "idle — no task". Two shapes land here: the span-derived
                                pseudo-lane (phase 'supervision', pre-r6 streams) and the r6 engine's REAL
                                supervision lanes, whose digests stamp `supervision: true`. */}
                            {supervising ? (
                              <Gavel size={12} className={cx('shrink-0', TONE_TEXT.accent)} />
                            ) : null}
                            <span
                              className={cx(
                                'truncate',
                                !live
                                  ? TONE_TEXT.stopped
                                  : supervising
                                    ? cx(TONE_TEXT.accent, WEIGHT.semibold)
                                    : 'text-lz-ink'
                              )}
                            >
                              {lane.description || lane.taskId}
                              {lane.phase === 'supervision' && typeof lane.elapsedMs === 'number'
                                ? ` · ${Math.round(lane.elapsedMs / 1000)}s`
                                : ''}
                              {(() => {
                                // The judge lane is ROLLING — one lane per supervised task, each look reseeding
                                // the digest — and the caption owes the reader that fact (look N, 1-based;
                                // earlier looks folded into superseded). Null everywhere else, pre-r6 included.
                                const rolling = supervisionRollingCaption(lane);
                                return rolling ? ` · ${rolling}` : '';
                              })()}
                            </span>
                            {canExpand ? (
                              <ChevronRight size={12} className="shrink-0 text-lz-ink-3" />
                            ) : null}
                          </div>
                          {dev && lane.calls && lane.calls.length > 0
                            ? (() => {
                                // What this node is DOING right now — its latest tool/MCP call (running… when in-flight).
                                const last = lane.calls![lane.calls!.length - 1];
                                const cm = classifyCall(last);
                                return (
                                  <div className="mt-0.5 flex items-center gap-1.5 text-lz-meta text-lz-ink-3">
                                    <CallTypeIcon icon={cm.icon} color={CALL_KIND_COLOR[cm.kind]} />
                                    <span className="truncate">
                                      {cm.action}
                                      {last.summary ? (
                                        <span className="font-mono text-lz-mono text-lz-ink-3"> · {last.summary}</span>
                                      ) : null}
                                    </span>
                                  </div>
                                );
                              })()
                            : null}
                          {liveGen ? (
                            live ? (
                              // LIVE: typewriter-smoothed so the stream flows instead of jumping every poll. dev = 5
                              // lines to fill the space, compact/verbose = 2. Click the row to expand the full stream.
                              <NodeLiveText text={liveGen} lines={dev ? 5 : 2} />
                            ) : (
                              // HISTORICAL/DEAD run: the last frozen snapshot, static and on ink-3 — NEVER animated,
                              // so an old session no longer looks like it is still streaming.
                              <div
                                data-testid="fleet-node-gen"
                                className="mt-0.5 whitespace-pre-wrap break-words text-lz-ink-3"
                                style={{
                                  display: '-webkit-box',
                                  WebkitLineClamp: dev ? 5 : 2,
                                  WebkitBoxOrient: 'vertical',
                                  overflow: 'hidden',
                                }}
                              >
                                {liveGen}
                              </div>
                            )
                          ) : null}
                        </div>
                        {/* THE NODE'S OTHER LIVE LANES. Every node runs PARALLEL: 2, so this is routinely non-empty
                            and the strip used to drop it: measured live, gabee ran open-coverage-1 at 68,393 reasoning
                            characters alongside slice-index-html and only the second had a cell. The two LARGEST lanes
                            in the run were invisible — "I cannot see what the nodes are doing" with a mechanism behind
                            it. Compact sibling lines rather than full cells: the point is that nothing a node is doing
                            is missing, not that every lane gets equal estate.

                            SIBLINGS OF THE PRIMARY CONTROL, NOT CHILDREN. They used to sit inside it, where a click
                            bubbled up and opened the PRIMARY lane's inspector, and where a nested role="button" is
                            presentational to assistive tech. Each row is its own control that opens the inspector
                            on ITS task (and stops the row's click from re-aiming it at the primary), with the same
                            anchors (`data-task`, `data-expandable`, `data-gen-len`) the per-tick instrument reads
                            off the primary cell, computed the same way. */}
                        {siblings.map((extra) => {
                          const extraFull = fleetExpandText(extra);
                          const extraCanExpand = extraFull.length > 0;
                          const title = laneSiblingTitle(extra);
                          const openExtra = () => setInspect({ device, taskId: extra.taskId });
                          return (
                            <div
                              key={extra.taskId}
                              data-testid="fleet-node-also"
                              data-task={extra.taskId}
                              data-expandable={extraCanExpand ? 'true' : 'false'}
                              data-gen-len={extraFull.length}
                              className={cx(
                                'mt-1 flex min-h-5 items-center gap-2 text-lz-meta text-lz-ink-3',
                                extraCanExpand ? 'cursor-pointer' : 'cursor-default'
                              )}
                              role={extraCanExpand ? 'button' : undefined}
                              tabIndex={extraCanExpand ? 0 : undefined}
                              aria-label={
                                extraCanExpand
                                  ? `Open the full stream of ${title} on ${short}`
                                  : undefined
                              }
                              onClick={
                                extraCanExpand
                                  ? (e: React.MouseEvent) => {
                                      e.stopPropagation();
                                      openExtra();
                                    }
                                  : (e: React.MouseEvent) => e.stopPropagation()
                              }
                              onKeyDown={
                                extraCanExpand
                                  ? (e: React.KeyboardEvent) => {
                                      if (e.key === 'Enter' || e.key === ' ') {
                                        e.preventDefault();
                                        openExtra();
                                      }
                                    }
                                  : undefined
                              }
                            >
                              <span className={cx('w-4 shrink-0 text-center', WEIGHT.semibold, 'text-lz-ink-3')}>
                                +
                              </span>
                              {/* An r6 supervision lane riding beside the worker lane keeps the class's accent
                                  here too, so a node's second row says WHAT KIND of work it is. */}
                              {extra.supervision === true ? (
                                <Gavel size={11} className={cx('shrink-0', TONE_TEXT.accent)} />
                              ) : null}
                              <span
                                className={cx(
                                  'max-w-[40%] shrink-0 truncate',
                                  extra.supervision === true
                                    ? cx(TONE_TEXT.accent, WEIGHT.semibold)
                                    : 'text-lz-ink'
                                )}
                              >
                                {title}
                              </span>
                              <span className="min-w-0 truncate">{laneLiveLine(extra)}</span>
                              {extraCanExpand ? (
                                <ChevronRight size={12} className="shrink-0 text-lz-ink-3" />
                              ) : null}
                            </div>
                          );
                        })}
                      </div>
                    ) : (() => {
                        // No lane, no span — but LM Studio's own status may still say the node is generating.
                        // The old text here GUESSED the work class ("review/test-gen call in flight") — a
                        // hardcoded label that misattributed a 43m52s replan call on r5. Since efa2014ab
                        // every supervision call mints a real lane at dispatch (judge-<task>, replan-rN,
                        // prereview-<task>, tail-review-<dim>), so a busy node normally renders that lane
                        // above, labeled from its key, and this branch is only the seed-write gap or an
                        // older engine's keyless call. Say what is KNOWN, no more.
                        const busy = live && (lmsState === 'generating' || lmsState === 'processingPrompt');
                        const body = busy ? (
                          <Tip label="LM Studio reports this node generating, but no lane digest is on disk for the call. Current engines write a lane for every call — supervision included — at dispatch; older engines ran supervision calls keyless, leaving only the event log.">
                            <span className={cx('flex min-h-5 items-center gap-1.5', TONE_TEXT.ok, WEIGHT.semibold)}>
                              <Eye size={12} className="shrink-0" />
                              generating — no lane on disk for this call yet
                            </span>
                          </Tip>
                        ) : (
                          <span className="flex min-h-5 items-center gap-1.5 text-lz-ink-3">
                            {live ? 'no task' : '—'}
                            {canExpand ? <ChevronRight size={12} className="shrink-0" /> : null}
                          </span>
                        );
                        // Between tasks the node's FINISHED calls are still openable — the inspector is
                        // the cumulative log — so the cell is a control named for what it opens.
                        return canExpand ? (
                          <div
                            role="button"
                            tabIndex={0}
                            aria-label={`Open the calls ${short} ran this run`}
                            onKeyDown={(e: React.KeyboardEvent) => {
                              if (e.key === 'Enter' || e.key === ' ') {
                                e.preventDefault();
                                openPrimary();
                              }
                            }}
                          >
                            {body}
                          </div>
                        ) : (
                          body
                        );
                      })()}
                  </td>
                  <td
                    className={cx(
                      'px-3 py-2 text-right align-top text-lz-body',
                      TNUM,
                      lane?.toolCalls ? 'text-lz-ink' : 'text-lz-ink-4'
                    )}
                  >
                    <span className="flex h-5 items-center justify-end">
                      {lane?.toolCalls ? lane.toolCalls : '—'}
                    </span>
                  </td>
                  <td
                    className={cx(
                      'px-3 py-2 text-right align-top text-lz-body',
                      TNUM,
                      fmtChars(lane?.thinkingChars) ? 'text-lz-ink' : 'text-lz-ink-4'
                    )}
                  >
                    <span className="flex h-5 items-center justify-end">
                      {fmtChars(lane?.thinkingChars) || '—'}
                    </span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {live && unattributed.length > 0 ? (
        // Judge spans with no busy node to pin them to — real work, shown unattributed rather than dropped.
        // ONE line for all of them: the caption is a fact about the class, not about each span, and a row
        // per span painted the same 54 characters four times in a column (measured live, 2026-08-29).
        <div
          data-testid="fleet-unattributed"
          className={cx('flex items-start gap-2 border-t px-3 py-2 text-lz-body', SURFACE.hairline)}
        >
          <Gavel size={12} className={cx('mt-[3px] shrink-0', TONE_TEXT.accent)} />
          <span className="min-w-0 text-lz-ink-3">
            {unattributed.map((s, i) => (
              <React.Fragment key={`sup-${s.kind}-${s.taskId}`}>
                {i > 0 ? <span> · </span> : null}
                <span className={cx(TONE_TEXT.accent, WEIGHT.semibold)}>{s.label}</span>
              </React.Fragment>
            ))}
            <span>
              {unattributed.length === 1
                ? ' — on an idle node (the verdict names it when it lands)'
                : ' — on idle nodes (each verdict names its node when it lands)'}
            </span>
          </span>
        </div>
      ) : null}
      {inspect
        ? (() => {
            const i = Math.max(deviceOrder.indexOf(inspect.device), 0);
            const primary = runningByDevice.get(inspect.device);
            const deviceHistory = historyByDevice.get(inspect.device) ?? [];
            // NOTHING CLEARS ON PHASE END. A lane that finishes while this modal is open leaves the
            // running maps, but its digest and durable logs persist — resolve it from history so the
            // reader keeps the call they were reading (now labeled finished) instead of the modal
            // silently jumping to whatever the node picked up next.
            const lane =
              [primary, ...(alsoRunningByDevice.get(inspect.device) ?? [])].find(
                (l) => l?.taskId === inspect.taskId
              ) ??
              deviceHistory.find((h) => h.lane.taskId === inspect.taskId)?.lane ??
              primary;
            return (
              <NodeInspector
                device={inspect.device}
                letter={String.fromCharCode(65 + (i % 26))}
                index={i}
                lane={lane}
                nodeState={nodeStatus[shortName(inspect.device)]}
                history={deviceHistory}
                runDir={runDir}
                onClose={() => setInspect(null)}
              />
            );
          })()
        : null}
    </div>
  );
};

// The chronological engine narrative — the body of the EVENT LOG zone: a monospace TICKER on the surface-2
// well, hairline-divided rows, a tabular ordinal gutter. Latest at the bottom; a pulsing dot tail while the
// run is live. In verbose mode it shows the FULL stream and wraps; compact keeps the last few rows.
const EVENT_LOG_COMPACT_ROWS = 8;

const ActivityFeed: React.FC<{ items: ActivityItem[]; live: boolean; verbose: boolean; workingDir?: string }> = ({ items, live, verbose, workingDir }) => {
  const shown = verbose ? items : items.slice(-EVENT_LOG_COMPACT_ROWS);
  if (items.length === 0) return null;
  const gutter = shown.some((it) => eventClock(it.at) !== null)
    ? EVENT_LOG_GUTTER_CLOCK
    : EVENT_LOG_GUTTER_ORDINAL;
  return (
    <ol
      className="divide-y divide-lz-border bg-lz-surface-2 py-1 font-mono text-lz-mono text-lz-ink"
      aria-label="Event log"
    >
      {shown.map((it) => (
        <ActivityLine key={it.seq} it={it} wrap={verbose} workingDir={workingDir} gutter={gutter} />
      ))}
      {live && (
        <li className="flex min-h-6 items-center gap-2 px-3 py-0.5 text-lz-ink-3">
          <span className={cx(gutter, 'shrink-0')} aria-hidden />
          <StatusDot tone="accent" live label="the engine is working" />
          <span>working…</span>
        </li>
      )}
    </ol>
  );
};

/**
 * EVENT LOG zone — the feed, explicitly named for what it is and visually subordinate. Collapsed by
 * default outside developer/verbose mode: judge verdicts and failures already surface on WORK-board rows,
 * so the log is the narrative/debugging record, not the primary read. Collapsed, it still shows the count
 * and the latest line so nothing feels hidden. The count pill counts the rows the body SHOWS (compact
 * shows the last few); the total rides beside it as meta.
 */
const EventLogZone: React.FC<{
  items: ActivityItem[];
  live: boolean;
  verbose: boolean;
  workingDir?: string;
}> = ({ items, live, verbose, workingDir }) => {
  const [override, setOverride] = useState<boolean | null>(null);
  if (items.length === 0) return null;
  const open = override ?? verbose;
  const last = items[items.length - 1];
  const shownCount = verbose ? items.length : Math.min(items.length, EVENT_LOG_COMPACT_ROWS);
  return (
    <div className="border-t border-lz-border">
      <ZoneHeader
        label="Event log"
        explain="everything the engine reported, in order"
        count={open ? shownCount : undefined}
        collapsed={!open}
        onToggle={() => setOverride((o) => !(o ?? verbose))}
        right={
          <>
            {!open && last ? (
              <span className="max-w-[18rem] truncate text-lz-meta text-lz-ink-3">{last.text}</span>
            ) : null}
            <span className={cx('shrink-0 text-lz-meta text-lz-ink-3', TNUM)}>
              {open && shownCount < items.length
                ? `last ${shownCount} of ${items.length} events`
                : `${items.length} events`}
            </span>
          </>
        }
      />
      {open ? (
        <ActivityFeed items={items} live={live} verbose={verbose} workingDir={workingDir} />
      ) : null}
    </div>
  );
};

/** The three verdict tones a confidence number can carry. Text, fills and bars are Studio classes
 *  (TONE_TEXT / TONE_FILL / TONE_DOT); the SVG gauge paints strokes and fills, so it has its own
 *  literal class maps; the token variable survives only for confColorVsFloor's pinned contract. */
type ConfTone = 'ok' | 'warn' | 'err';
const CONF_TONE_VAR: Record<ConfTone, string> = {
  ok: STATUS_COLOR.done,
  warn: AMBER,
  err: STATUS_COLOR.error,
};
const CONF_STROKE: Record<ConfTone, string> = {
  ok: 'stroke-lz-ok',
  warn: 'stroke-lz-warn',
  err: 'stroke-lz-err',
};
const CONF_FILL: Record<ConfTone, string> = { ok: 'fill-lz-ok', warn: 'fill-lz-warn', err: 'fill-lz-err' };

// Threshold tone for a confidence value: solid green >=70 (confident), amber 40-69 (unsure), red <40.
// Use this for the SUB-SIGNALS (agreement / spec-clarity), where the point is which one is lower — not
// whether the run may proceed. For the headline number use confToneVsFloor.
const confTone = (v: number): ConfTone => (v >= 70 ? 'ok' : v >= 40 ? 'warn' : 'err');

/** Colour for the HEADLINE confidence, against the engine's own bar.
 *
 *  The band above is a UI invention. When a floor is set, the engine has already made the go/no-go call:
 *  below the floor it ASKS instead of building. A 73 under a floor of 80 painted green said "good" in the
 *  one channel a user reads before any words — while the run had stopped and asked. confVerdict was fixed
 *  for exactly this and the colour was left behind, so the pill went on being green next to text saying
 *  "Below your bar of 80". No floor = that run never asks = there is no bar = the band is all we can say. */
export const confToneVsFloor = (v: number, floor: number | null): ConfTone => {
  if (floor == null) return confTone(v);
  if (v >= floor) return 'ok';
  // Under the bar. Amber = goose asked and is waiting; red = it is not close.
  return v >= floor - 20 ? 'warn' : 'err';
};
/** The same verdict as the palette's token variable — the contract confColorVsFloor.test.ts pins. */
export const confColorVsFloor = (v: number, floor: number | null): string =>
  CONF_TONE_VAR[confToneVsFloor(v, floor)];

// One signal (agreement / spec-clarity). The value leads at full weight in its own colour, the track is a
// real 6px bar with a SOLID fill (never a tint), and the engine's reason sits directly under it at full
// measure instead of being indented into a narrow gutter.
//
// The fill is keyed to the ENGINE'S ASK FLOOR, not a UI-invented 70/40 band: `final = min(agreement,
// specClarity)`, so a signal under the floor is precisely the one that made goose ask. Colouring it against
// a different threshold than the headline is how the headline went amber while its own cause read green.
const ConfSignal: React.FC<{
  label: string;
  value: number;
  reason?: string | null;
  binding: boolean;
  askFloor?: number | null;
}> = ({ label, value, reason, binding, askFloor = null }) => {
  const tone = confToneVsFloor(value, askFloor);
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline gap-2">
        <span className={TYPE.zone}>{label}</span>
        {binding ? (
          <span className={cx('px-1.5 py-px text-lz-meta', WEIGHT.semibold, RADIUS.control, TONE_FILL[tone])}>
            binding
          </span>
        ) : null}
        <span className={cx('ml-auto text-[17px] leading-none', WEIGHT.semibold, TNUM, TONE_TEXT[tone])}>
          {value}
        </span>
      </div>
      <div className={cx('h-1.5 overflow-hidden border border-lz-border bg-lz-surface-2', RADIUS.control)}>
        <div
          className={cx('h-full', TONE_DOT[tone])}
          style={{ width: `${Math.max(0, Math.min(100, value))}%`, transition: 'width 500ms ease-out' }}
        />
      </div>
      {reason ? <div className="text-lz-meta leading-snug text-lz-ink-2">{reason}</div> : null}
    </div>
  );
};

// Radial arc gauge for the headline plan confidence — a 270° sweep in the value's threshold color over a
// neutral track, the big number centered. Solid saturated stroke, sharp/flat (no soft glow or faded tint):
// the visual anchor of the confidence panel. Pure SVG, theme-aware via CSS vars.
const ConfGauge: React.FC<{ value: number; size?: number; askFloor?: number | null }> = ({
  value,
  size = 76,
  askFloor = null,
}) => {
  const v = Math.max(0, Math.min(100, Math.round(value)));
  const r = 32;
  const circ = 2 * Math.PI * r;
  const sweep = 0.75; // 270°
  const track = sweep * circ;
  const fill = (v / 100) * sweep * circ;
  const tone = confToneVsFloor(v, askFloor);
  return (
    <svg width={size} height={size} viewBox="0 0 80 80" className="shrink-0" role="img" aria-label={`plan confidence ${v} of 100${askFloor != null ? `, your bar is ${askFloor}` : ''}`}>
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        className="stroke-lz-border"
        strokeWidth="8"
        strokeDasharray={`${track} ${circ}`}
        transform="rotate(135 40 40)"
      />
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        className={CONF_STROKE[tone]}
        strokeWidth="8"
        strokeDasharray={`${fill} ${circ}`}
        transform="rotate(135 40 40)"
        style={{ transition: 'stroke-dasharray 500ms ease-out' }}
      />
      {/* YOUR BAR, drawn on the ring. Without it the colour is an unexplained hue; with it the number is
          visibly above or below the line the engine actually judged it against. Solid, never a tint. */}
      {askFloor != null ? (
        <rect
          x={40 + r - 6.5}
          y="38.6"
          width="13"
          height="2.8"
          className="fill-lz-ink"
          transform={`rotate(${135 + (Math.max(0, Math.min(100, askFloor)) / 100) * 270} 40 40)`}
        />
      ) : null}
      <text
        x="40"
        y="39"
        textAnchor="middle"
        dominantBaseline="middle"
        className={cx(CONF_FILL[tone], TNUM)}
        style={{ fontSize: 24, fontWeight: 800, letterSpacing: -0.6 }}
      >
        {v}
      </text>
      {/* Reads at fontSize 8 in DARK mode. --text-secondary at this size rendered near-black on the dark
          card — Mihai could not see it at all. Tint it with the gauge's own colour at full opacity instead
          of a dimmer grey: same hue as the number it belongs to, and solid, never a faded tint. */}
      <text
        x="40"
        y="55.5"
        textAnchor="middle"
        dominantBaseline="middle"
        className={CONF_FILL[tone]}
        style={{ fontSize: 9, fontWeight: 700, letterSpacing: 0.5 }}
      >
        /100
      </text>
    </svg>
  );
};

// Shared breakdown body — reused by the header-expand panel AND the ClarifyPrompt (one visual language). The
// min number, the two sub-bars, WHAT'S HOLDING IT BACK (the binding/lower signal), WHAT WOULD RAISE IT
// (honest, research-backed), and a climb trail/sparkline when the meter has moved.
/** The verdict on a confidence number, derived from the ENGINE's floor — never a band the UI invented.
 *  A hardcoded `>= 70 -> "Strong — ready to build"` told the user exactly that about a plan scoring 73
 *  against a floor of 80, which had burned all 4 retarget rounds, proceeded at the cap, and ASKED 3
 *  questions. The number was right there next to the claim and contradicted it. With no floor set the run
 *  never asks, so there is no bar to be under and the band is all we can honestly say. */
export function confVerdict(value: number, floor: number | null): string {
  if (floor == null) {
    return value >= 70 ? 'Strong' : value >= 40 ? 'Mixed — check below' : 'Low — needs your input';
  }
  if (value >= floor) return `At your bar (${floor}+) — ready to build`;
  return `Below your bar of ${floor} — goose asked before building`;
}

/// What would actually raise the score when AGREEMENT is the binding signal, tiered by how much room is
/// left. This was one static string that assumed agreement was LOW — so a real run at agreement 88 rendered
/// "The drafts disagree on how to structure the build" directly beneath the engine's own
/// "3 drafts agree: count spread 1, file-overlap 100% (role-normalized)". The panel contradicted the engine
/// on screen, and pitched "the retarget option" at a user who already had it switched on.
export function agreementAdvice(agreement: number): string {
  if (agreement >= 85) {
    return 'The drafts already landed on nearly the same structure, so there is little headroom here — what remains is the last of the naming and granularity spread. Re-drafting is unlikely to move it much.';
  }
  if (agreement >= 60) {
    return 'The drafts broadly agree but split on part of the structure. Re-drafting toward a consensus backbone (the retarget option) is what lifts this — though a small local fleet may not fully converge.';
  }
  return 'The drafts disagree on how to structure the build. The score reflects that drafted plan — it only lifts if goose re-drafts toward a consensus (the retarget option), and a small/weak fleet may still not fully agree.';
}

/// The binding signal's reason, framed so it reads as a CAP rather than a compliment.
///
/// `final = min(agreement, spec_clarity)`, so when agreement binds it is the ceiling — but its reason string
/// is phrased positively ("3 drafts agree: ..."), which read as nonsense under a "What's holding it back"
/// header. Name the cap, then give the engine's reason verbatim.
export function holdingBackText(
  agreement: number,
  bindingAgreement: boolean,
  agreementReason: string | null | undefined,
  productSpecified: boolean
): string {
  if (!bindingAgreement) {
    return productSpecified
      ? 'Some requirements are still ambiguous.'
      : "The product itself isn't fully specified yet.";
  }
  if (!agreementReason) return 'The planning drafts disagree on how to structure the build.';
  return `Agreement caps the score at ${agreement}. ${agreementReason}`;
}

const ConfidenceBreakdownBody: React.FC<{
  conf: ConfidenceBreakdown;
  trail?: number[];
  hasPendingQuestions: boolean;
  askFloor?: number | null;
}> = ({ conf, trail, hasPendingQuestions, askFloor = null }) => {
  const bindingAgreement = conf.agreement <= conf.specClarity;
  const showDecisions = !bindingAgreement && conf.openDecisions.length > 0;
  const holdingBack = holdingBackText(
    conf.agreement,
    bindingAgreement,
    conf.agreementReason,
    conf.productSpecified
  );
  const raiseIt = bindingAgreement
    ? agreementAdvice(conf.agreement)
    : hasPendingQuestions
      ? 'Answer the questions below — each resolves an open decision. Goose can also research the undecided points.'
      : 'Researching the undecided points to firm up the spec.';
  return (
    <div className="space-y-3.5">
      <div className="flex items-center gap-3.5">
        <ConfGauge value={conf.final} askFloor={askFloor} />
        <div className="min-w-0">
          <div className="text-lz-meta text-lz-ink-3">Plan confidence</div>
          <div
            className={cx(
              'mt-1 text-[15px] leading-snug',
              WEIGHT.semibold,
              TONE_TEXT[confToneVsFloor(conf.final, askFloor)]
            )}
          >
            {confVerdict(conf.final, askFloor)}
          </div>
          {askFloor != null ? (
            <div className="mt-1 text-lz-meta text-lz-ink-3">
              Your bar is <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>{askFloor}</span> — below it, goose
              asks you instead of guessing.
            </div>
          ) : null}
        </div>
      </div>
      {/* Full border, never a left rail. The two signals are one group because the LOWER of them IS the
          headline score — showing them apart hides that relationship. */}
      <div className={cx('space-y-4 border border-lz-border px-3 py-3', RADIUS.control)}>
        <ConfSignal
          label="Agreement"
          value={conf.agreement}
          reason={conf.agreementReason}
          binding={conf.agreement <= conf.specClarity}
          askFloor={askFloor}
        />
        <ConfSignal
          label="Spec clarity"
          value={conf.specClarity}
          reason={conf.specClarityReason}
          binding={conf.specClarity < conf.agreement}
          askFloor={askFloor}
        />
      </div>
      <div>
        <div className="mb-1.5 text-lz-meta text-lz-ink-3">
          What&apos;s holding it back
        </div>
        {showDecisions ? (
          <ul className="space-y-0.5">
            {conf.openDecisions.map((d, i) => (
              <li key={i} className="text-[12px] leading-relaxed text-lz-ink flex gap-1.5">
                <span className="text-lz-ink-3 shrink-0">·</span>
                <span>{d}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="text-[12px] leading-relaxed text-lz-ink">{holdingBack}</div>
        )}
      </div>
      <div>
        <div className="mb-1.5 text-lz-meta text-lz-ink-3">
          What would raise it
        </div>
        <div className="text-[12px] leading-relaxed text-lz-ink">{raiseIt}</div>
      </div>
      {trail && trail.length >= 2 ? (
        <div className="flex items-center gap-2">
          <div className="flex items-end gap-0.5 h-6">
            {trail.map((v, i) => (
              <div
                key={i}
                className={TONE_DOT[confTone(v)]}
                style={{ width: 4, height: `${Math.max(6, v * 0.24)}px`, borderRadius: 1 }}
              />
            ))}
          </div>
          <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
            {trail.map((v, i) => (
              <React.Fragment key={i}>
                {i > 0 ? ' → ' : ''}
                <span className={i === trail.length - 1 ? TONE_TEXT[confTone(v)] : undefined}>{v}</span>
              </React.Fragment>
            ))}
          </span>
        </div>
      ) : null}
    </div>
  );
};

// The PLANNING zone's compact confidence pill — solid fill in the floor-judged color, shown in the zone
// header so the number is present whether the zone is open or collapsed to its one-line summary.
const ConfPill: React.FC<{ value: number; askFloor?: number | null }> = ({ value, askFloor = null }) => (
  <Tip
    label={`Planner confidence in how it broke this app down — ${value}/100. ${confVerdict(value, askFloor)}.`}
  >
    <span
      className={cx(
        'flex shrink-0 items-center gap-1 px-1.5 py-0.5 text-lz-meta',
        TNUM,
        RADIUS.control,
        TONE_FILL[confToneVsFloor(value, askFloor)]
      )}
    >
      <Gauge className="h-2.5 w-2.5" />
      conf {value}
    </span>
  </Tip>
);

/** Wall-clock elapsed in the unit a person would say it in: seconds under a minute, "Nm Ss", "Nh Nm",
 *  and days once it passes two days ("49d 8h"). The minutes are the same fact; only the unit changes. */
export function fmtElapsed(min: number): string {
  const totalSec = Math.max(0, Math.round(min * 60));
  const d = Math.floor(totalSec / 86400);
  const h = Math.floor((totalSec % 86400) / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (d >= 2) return `${d}d ${h}h`;
  if (d > 0 || h > 0) return `${d * 24 + h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/** A minute RANGE, humanely: minutes under two hours, hours under two days, days beyond ("~8–33 d").
 *  Rounded for DISPLAY only — lo/hi are the honest estimate and are not touched. */
export function fmtRangeMin(lo: number, hi: number): string {
  const unit =
    hi >= 2 * 1440 ? { div: 1440, name: 'd' } : hi >= 120 ? { div: 60, name: 'h' } : { div: 1, name: 'min' };
  const a = Math.max(1, Math.round(lo / unit.div));
  const b = Math.max(a, Math.round(hi / unit.div));
  return a === b ? `~${a} ${unit.name}` : `~${a}–${b} ${unit.name}`;
}

// Live progress metrics inside the RUN HEADER band: TOTAL ELAPSED (a fact, ticking every second) + a rough,
// clearly-labeled ETA for the rest. The ETA is deliberately a WIDE range: the local fleet is slow and
// variable and the single-node verify sink dominates the tail, so a fake-precise figure would lie. It
// updates and shifts as tasks complete, and is suppressed until at least one task has finished. Merged into
// the header (was a floating strip) so identity, phase and timing read as ONE band.
const HeaderMetrics: React.FC<{
  startedAt: number | null;
  phaseTodo: PhaseTodo[];
}> = ({ startedAt, phaseTodo }) => {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  if (!startedAt) return null;
  const elapsedMin = (now - startedAt) / 60000;
  // ETA basis: ALL phase-checklist items done/total (research + plan + contracts + build + verify), NOT just
  // build tasks. A run can sit at 0/N build tasks for most of its life (long planning), which left this BLANK.
  // Counting every phase's items means it populates the moment research finishes and shifts as the run advances.
  const itemsDone = phaseTodo.reduce((n, p) => n + p.counts.done, 0);
  const itemsTotal = phaseTodo.reduce((n, p) => n + p.counts.total, 0);
  let etaLabel: string;
  if (itemsDone >= 1 && itemsTotal > itemsDone) {
    const perItem = elapsedMin / itemsDone;
    const remaining = itemsTotal - itemsDone;
    const parallel = Math.min(remaining, 2); // the fleet works a few tasks at once; the verify sink is single-node
    const mid = (perItem * remaining) / parallel;
    // Deliberately WIDE band: the local fleet is variable and the planning whipsaw makes any point estimate a
    // guess — a range that shifts is honest; a fake-precise minute count is not.
    const lo = Math.max(1, Math.round(mid * 0.5));
    const hi = Math.max(lo + 1, Math.round(mid * 2));
    etaLabel = `${fmtRangeMin(lo, hi)} left`;
  } else if (itemsTotal > 0 && itemsDone >= itemsTotal) {
    etaLabel = 'wrapping up';
  } else {
    // Never blank — before the first checklist item completes there is genuinely no basis to estimate from yet.
    etaLabel = 'estimating…';
  }
  return (
    <span className="flex items-center gap-3 shrink-0 tabular-nums">
      <Tip label="Total wall-clock time since the run started.">
        <span className="text-xs font-lz-semibold text-lz-ink">{fmtElapsed(elapsedMin)}</span>
      </Tip>
      <Tip label="A deliberately rough range — the local fleet is variable, so a precise figure would lie.">
        <span className="text-xs text-lz-ink-3">{etaLabel}</span>
      </Tip>
    </span>
  );
};

// Per-phase TODO. Every item's state is driven by an ENGINE event (see buildPhaseTodo) — the honesty is in
// the colors: 'done' (green) is a VERIFIED completion; 'unverified' is a SLATE check (built but the app was
// never run — must never look green); 'advisory' is info, never a check.
const TODO_COLOR: Record<TodoState, string> = {
  pending: 'var(--color-text-secondary)',
  running: AMBER,
  done: STATUS_COLOR.done,
  // The stopped slate — solid, deliberately NOT green: the work finished, the app was never run. Distinct
  // from pending (secondary text) by its check glyph and its darker ink; never a node hue.
  unverified: SWARM_STATUS.stopped,
  failed: STATUS_COLOR.error,
  judge_failed: AMBER,
  blocked: 'var(--color-text-secondary)',
  skipped: SWARM_STATUS.stopped,
  advisory: SWARM_STATUS.action,
};

/** The WORK board's dot tone per engine state — the same honesty as TODO_COLOR: a verified 'done' is the
 *  ok green; 'unverified' (built, never run) is the stopped slate and must never look green; failures are
 *  err; a judge intervention is the warn amber; advisory is the accent (information, never a check). */
const TODO_TONE: Record<TodoState, Tone> = {
  pending: 'stopped',
  running: 'warn',
  done: 'ok',
  unverified: 'stopped',
  failed: 'err',
  judge_failed: 'warn',
  blocked: 'stopped',
  skipped: 'stopped',
  advisory: 'accent',
};

const TodoGlyph: React.FC<{ state: TodoState }> = ({ state }) => {
  const c = TODO_COLOR[state];
  const cls = 'h-3.5 w-3.5 shrink-0';
  const s = { color: c };
  if (state === 'running') return <Loader2 className={`${cls} animate-spin`} style={s} />;
  if (state === 'done' || state === 'unverified') return <Check className={cls} strokeWidth={3} style={s} />;
  if (state === 'failed' || state === 'judge_failed') return <X className={cls} strokeWidth={3} style={s} />;
  if (state === 'blocked') return <CircleSlash className={cls} style={s} />;
  if (state === 'skipped') return <Minus className={cls} style={s} />;
  if (state === 'advisory') return <Info className={cls} style={s} />;
  return <Circle className={cls} style={s} />; // pending
};

/** A state WORD beside a row — the quiet Chip. The row's dot/glyph carries the tone; the word carries the
 *  meaning. Colouring the word too was a second register for one fact. */
const TodoPill: React.FC<{ text: string }> = ({ text }) => <Chip>{text}</Chip>;

// The judge's REASONING for a task — the diagnosis (verdict) + the exact corrective note it gave the worker.
// This is what Mihai wanted surfaced: not just "judge decision" but WHY.
const JudgeReason: React.FC<{ judge: NonNullable<PhaseTodoItem['judge']> }> = ({ judge }) => (
  <div className="text-[11px]">
    <div className="flex items-center gap-1.5 flex-wrap">
      <Gavel className={cx('size-3 shrink-0', TONE_TEXT.warn)} />
      <span className={cx(WEIGHT.semibold, 'text-lz-ink')}>Judge</span>
      {judge.verdict ? (
        <span className={TONE_TEXT.warn}>{judge.verdict.replace(/_/g, ' ')}</span>
      ) : null}
      <span className="text-lz-ink-3">→ {judge.action.replace(/_/g, ' ')}</span>
    </div>
    {judge.hint ? (
      <p className="text-text-secondary mt-0.5 leading-snug break-words">
        <InlineMarkdown content={judge.hint} />
      </p>
    ) : null}
  </div>
);

// Per-task LIVE GENERATION detail — what the model on THIS task actually produced: the tool-call breakdown (so
// over-reading is visible at a glance: many reads/shell, no write), how much it's thinking, the reasoning text,
// and which node/model. All of it is already collected per worker in .swarm/activity/<task>.json (the panel
// reads it as run.activity) — this is the surface that makes "what are the models actually doing / why so long"
// answerable instead of a blank two-line un-truncation.
const TaskGenDetail: React.FC<{ digest: Record<string, unknown> }> = ({ digest }) => {
  const num = (k: string) => (typeof digest[k] === 'number' ? (digest[k] as number) : 0);
  const str = (k: string) => (typeof digest[k] === 'string' ? (digest[k] as string) : '');
  const calls = Array.isArray(digest.calls) ? (digest.calls as Array<Record<string, unknown>>) : [];
  const byName: Record<string, number> = {};
  for (const c of calls) {
    const n = c && typeof c.name === 'string' ? c.name : 'other';
    byName[n] = (byName[n] ?? 0) + 1;
  }
  const rawModel = str('model');
  const model = rawModel.split('-').slice(0, 2).join('-') || rawModel;
  const toolCalls = num('tool_calls');
  const thinking = num('thinking_chars');
  const errors = num('errors');
  const malformed = num('malformed');
  const reasoning = taskGenReasoning(digest);
  const breakdown = Object.entries(byName)
    .map(([n, c]) => `${c} ${n}`)
    .join(' · ');
  const hasAny = toolCalls > 0 || thinking > 0 || model || reasoning.trim();
  if (!hasAny) return null;
  return (
    <div className="text-[11px] text-text-secondary space-y-1">
      <span className="font-lz-medium text-text-secondary">Live generation</span>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5">
        {model ? (
          <span>
            model <span className="font-mono text-text-primary">{model}</span>
          </span>
        ) : null}
        {toolCalls > 0 ? (
          <span>
            tool calls <span className="font-mono text-text-primary">{toolCalls}</span>
            {breakdown ? ` (${breakdown})` : ''}
          </span>
        ) : null}
        {thinking > 0 ? (
          <span>
            thinking <span className="font-mono text-text-primary">{thinking.toLocaleString()} ch</span>
          </span>
        ) : null}
        {errors > 0 ? (
          <span className={cx(WEIGHT.medium, TONE_TEXT.warn)}>
            {errors} app-error{malformed > 0 ? ` · ${malformed} malformed` : ''}
          </span>
        ) : null}
      </div>
      {reasoning.trim()
        ? (() => {
            // ITEM 2's residue: when both durable logs are absent this card's body IS the digest's
            // 24k clip — an archived run's leftover — and calling that "(live)" was the lie.
            const clip = taskGenClipNote(digest);
            return (
              <ReasoningBlock
                text={reasoning}
                label={clip ? 'Model reasoning' : 'Model reasoning (live)'}
                note={clip ?? undefined}
              />
            );
          })()
        : null}
    </div>
  );
};

// One task row: TITLE + short summary collapsed; the full spec, owned files, the judge's reasoning AND the live
// generation are tucked under an expand. A 'running' item on a STALE run is relabeled 'interrupted' (dead proc).
const PhaseTodoRow: React.FC<{
  item: PhaseTodoItem;
  deviceOrder: string[];
  stale: boolean;
  activity?: Record<string, unknown>;
  plan?: PlanTask[];
  workingDir?: string;
}> = ({ item, deviceOrder, stale, activity, plan, workingDir }) => {
  // build rows are `b-<taskid>`, the verify sink is `b-integrate-verify`; strip the prefix to key run.activity
  // (gen/app/miner/…) and run.plan. Non-task rows (r-start, p-conf, v-e2e…) simply won't match → no gen block.
  const taskId = item.id.replace(/^[bv]-/, '');
  const digest =
    activity && typeof activity[taskId] === 'object' && activity[taskId] !== null
      ? (activity[taskId] as Record<string, unknown>)
      : undefined;
  const planTask = plan?.find((t) => t.id === taskId);
  const revealFile = (rel: string) => {
    if (!workingDir) return;
    const base = workingDir.replace(/\/$/, '');
    void window.electron.revealInFinder(rel.startsWith('/') ? rel : `${base}/${rel}`);
  };
  const interrupted = stale && item.state === 'running';
  const c = interrupted ? CALL_PENDING : TODO_COLOR[item.state];
  const idx = item.device ? deviceIndex(item.device, deviceOrder) : -1;
  const hasDetail = !!(
    item.description ||
    (item.files && item.files.length) ||
    item.judge ||
    digest ||
    planTask
  );
  const [open, setOpen] = useState(false);
  return (
    <div className="min-w-0">
      <div
        {...(hasDetail
          ? {
              role: 'button' as const,
              tabIndex: 0,
              onClick: () => setOpen((o) => !o),
              onKeyDown: (e: React.KeyboardEvent) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setOpen((o) => !o);
                }
              },
            }
          : {})}
        className={cx(
          'flex min-h-7 w-full min-w-0 items-center gap-2 py-0.5 text-left text-lz-meta',
          hasDetail && 'cursor-pointer',
          hasDetail && SURFACE.hover,
          MOTION
        )}
      >
        {interrupted ? (
          <CircleSlash className="size-3.5 shrink-0" style={{ color: c }} />
        ) : (
          <TodoGlyph state={item.state} />
        )}
        {idx >= 0 ? <NodeDot index={idx} letter={String.fromCharCode(65 + (idx % 26))} /> : null}
        <span
          className={cx('shrink-0', WEIGHT.medium, item.state === 'pending' ? 'text-lz-ink-3' : 'text-lz-ink')}
        >
          {item.label}
        </span>
        {item.summary ? <span className="truncate text-lz-ink-3">· {item.summary}</span> : null}
        {interrupted ? <TodoPill text="interrupted" /> : null}
        {!interrupted && item.state === 'unverified' ? <TodoPill text="unverified" /> : null}
        {!interrupted && item.state === 'judge_failed' ? <TodoPill text="judge" /> : null}
        {!interrupted && item.state === 'blocked' ? <TodoPill text="blocked" /> : null}
        {item.detail ? <span className="truncate text-lz-ink-3">· {item.detail}</span> : null}
        {hasDetail ? (
          <span className="ml-auto shrink-0 text-lz-ink-3">
            {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          </span>
        ) : null}
      </div>
      {open && hasDetail ? (
        <div className="mb-1.5 ml-5 mt-1 space-y-1.5">
          {planTask && (planTask.difficulty || planTask.deps.length) ? (
            <div className="flex flex-wrap gap-x-3 text-lz-meta text-lz-ink-3">
              {planTask.difficulty ? (
                <span>
                  difficulty <span className="text-text-primary">{planTask.difficulty}</span>
                </span>
              ) : null}
              {planTask.deps.length ? (
                <span>
                  after <span className="font-mono text-text-primary">{planTask.deps.join(', ')}</span>
                </span>
              ) : null}
            </div>
          ) : null}
          {item.files && item.files.length ? (
            <div className="text-[11px] text-text-secondary break-words">
              <span className="font-lz-medium text-text-secondary">Files</span>{' '}
              {item.files.map((f, i) => (
                <span key={f}>
                  {i > 0 ? ', ' : ''}
                  <button
                    onClick={() => revealFile(f)}
                    disabled={!workingDir}
                    title={workingDir ? 'Reveal in Finder' : undefined}
                    className={`font-mono text-text-primary ${workingDir ? 'hover:underline cursor-pointer' : ''}`}
                  >
                    {f}
                  </button>
                </span>
              ))}
            </div>
          ) : null}
          {item.description ? <ReasoningBlock text={item.description} label="Full task spec" reasoning={false} /> : null}
          {digest ? <TaskGenDetail digest={digest} /> : null}
          {item.judge ? <JudgeReason judge={item.judge} /> : null}
        </div>
      ) : null}
    </div>
  );
};

// A WORK-board group header: solid state dot + the mono group name + count. The three groups
// (RUNNING / QUEUED / DONE) are the "what is ongoing, what is planned, what is done" Mihai asked for.
const BoardGroupHeader: React.FC<{
  label: string;
  tone: Tone;
  live?: boolean;
  count: number;
  extra?: React.ReactNode;
}> = ({ label, tone, live = false, count, extra }) => (
  <div className="flex h-8 items-center gap-2 px-3">
    <StatusDot tone={tone} live={live} label={`${label} tasks`} />
    <span className={cx('text-lz-meta text-lz-ink-2', WEIGHT.semibold)}>{label}</span>
    <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>{count}</span>
    {extra}
  </div>
);

// One WORK-board row: the task's engine-truth state, node, title + summary collapsed; RUNNING rows carry a
// live tool-call line. Click → the row's OWN card (full spec, tool calls, reasoning, judge) — the per-task
// detail that used to live in a parallel duplicate list.
const BoardTaskRow: React.FC<{
  row: BoardRow;
  deviceOrder: string[];
  stale: boolean;
  dev: boolean;
  workingDir?: string;
  digest?: Record<string, unknown>;
}> = ({ row, deviceOrder, stale, dev, workingDir, digest }) => {
  const interrupted = stale && row.state === 'running';
  const idx = row.device ? deviceIndex(row.device, deviceOrder) : -1;
  const lane = row.lane;
  const { completed: calls, running } = workRows(lane?.calls, lane?.inflight);
  // FINDING 23, same slide as the lane card: index keys over the engine's sliding last-60 window
  // reassign row identity on every digest write past 60 calls — key by the absolute ordinal.
  const callMeta = callRowMeta(calls, lane?.toolCalls);
  const reasoning = lane ? laneNarrative(lane) : '';
  const failLike = row.state === 'failed' || row.state === 'judge_failed' || interrupted;
  const laneError = failLike && lane?.error ? lane.error.trim() : '';
  const hasDetail = !!(
    row.description ||
    (row.files && row.files.length) ||
    row.judge ||
    calls.length > 0 ||
    reasoning ||
    laneError ||
    digest ||
    row.deps.length ||
    row.difficulty
  );
  const [open, setOpen] = useState(false);
  const expanded = open || (dev && row.state === 'running' && !interrupted);
  const judgeFlag =
    row.judge && row.judge.verdict && !['ok', 'accept', ''].includes(row.judge.verdict)
      ? row.judge.verdict.replace(/_/g, ' ')
      : null;
  // The live line: what this row's node is DOING right now — the latest tool call, else the freshest
  // substantive narration. Only for RUNNING rows; everything else states its outcome instead.
  const lastCall = calls.length ? calls[calls.length - 1] : undefined;
  const liveGen = lane ? laneLiveLine(lane) : '';
  const secs = typeof row.elapsedMs === 'number' ? Math.round(row.elapsedMs / 1000) : null;
  const revealFile = (rel: string) => {
    if (!workingDir) return;
    const base = workingDir.replace(/\/$/, '');
    void window.electron.revealInFinder(rel.startsWith('/') ? rel : `${base}/${rel}`);
  };
  const firstBadCall = firstCallNeedingAttention(calls);
  return (
    <div className="min-w-0" data-testid="board-row">
      <div
        {...(hasDetail
          ? {
              role: 'button' as const,
              tabIndex: 0,
              onClick: () => setOpen((o) => !o),
              onKeyDown: (e: React.KeyboardEvent) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  setOpen((o) => !o);
                }
              },
            }
          : {})}
        className={cx(
          'flex min-h-8 w-full min-w-0 items-center gap-2 px-3 text-left text-lz-body',
          hasDetail && 'cursor-pointer',
          hasDetail && SURFACE.hover,
          MOTION
        )}
      >
        {/* ONE dot per state: the colour is the mark, the label is the meaning; a running row pulses. */}
        <StatusDot
          tone={interrupted ? 'stopped' : TODO_TONE[row.state]}
          live={row.state === 'running' && !interrupted}
          label={interrupted ? 'interrupted' : row.state.replace(/_/g, ' ')}
        />
        {idx >= 0 ? <NodeDot index={idx} letter={String.fromCharCode(65 + (idx % 26))} /> : null}
        <span
          className={cx(
            'shrink-0',
            WEIGHT.medium,
            row.state === 'failed' ? TONE_TEXT.err : row.state === 'pending' ? 'text-lz-ink-3' : 'text-lz-ink'
          )}
        >
          {row.title}
        </span>
        {row.kind === 'repair' ? <TodoPill text="repair" /> : null}
        {!interrupted && row.state === 'skipped' ? <TodoPill text="skipped" /> : null}
        {row.summary ? <span className="truncate text-lz-ink-3">· {row.summary}</span> : null}
        {interrupted ? <TodoPill text="interrupted" /> : null}
        {!interrupted && row.state === 'unverified' ? <TodoPill text="unverified" /> : null}
        {!interrupted && row.state === 'judge_failed' ? <TodoPill text="judge" /> : null}
        {!interrupted && row.state === 'blocked' ? <TodoPill text="blocked" /> : null}
        {judgeFlag ? (
          <Tip label={`The judge intervened: ${judgeFlag}${row.judge?.hint ? ` — ${row.judge.hint.slice(0, 140)}` : ''}`}>
            <span className={cx('flex shrink-0 items-center gap-0.5', TONE_TEXT.warn)}>
              <Gavel className="size-3" /> {judgeFlag}
            </span>
          </Tip>
        ) : null}
        {typeof lane?.restreams === 'number' && lane.restreams > 0 ? (
          // FINDING 8: the judge wiped and re-streamed this call — its live reasoning and thinking
          // counter restart from nothing, which read as a glitch with no cause on screen. The durable
          // .think.log keeps every wiped chunk; this chip is the on-screen cause.
          <Tip
            label={`The judge wiped and re-streamed this call ${lane.restreams} time${lane.restreams === 1 ? '' : 's'} — the live reasoning and its counter restart; the durable thinking log keeps everything.`}
          >
            <span className="inline-flex shrink-0" data-testid="lane-restreamed">
              <Chip tone="stopped">re-streamed ×{lane.restreams}</Chip>
            </span>
          </Tip>
        ) : null}
        {row.detail ? <span className="truncate text-lz-ink-3">· {row.detail}</span> : null}
        <span className={cx('ml-auto flex shrink-0 items-center gap-2 text-lz-meta text-lz-ink-3', TNUM)}>
          {row.state === 'running' && lane?.toolCalls ? (
            <span className="flex items-center gap-0.5">
              <Wrench className="h-3 w-3" />
              {lane.toolCalls}
            </span>
          ) : null}
          {row.state === 'running' && lane?.errors ? (
            <span className={TONE_TEXT.err}>{lane.errors}✕</span>
          ) : null}
          {row.state !== 'running' && secs != null ? <span>{secs}s</span> : null}
          {typeof row.attempts === 'number' && row.attempts > 1 ? <span>×{row.attempts}</span> : null}
          {row.state === 'pending' && row.deps.length ? (
            <span className="font-mono truncate max-w-[12rem]">after {row.deps.join(', ')}</span>
          ) : null}
          {row.state === 'pending' && row.difficulty ? <span>{row.difficulty}</span> : null}
          {hasDetail ? (
            expanded ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />
          ) : null}
        </span>
      </div>
      {row.state === 'running' && !interrupted && !expanded ? (
        <div className="flex min-w-0 items-center gap-1.5 pb-1.5 pl-9 pr-3 text-lz-meta">
          {/* A call FORMING leads even over a running one: its bytes are this instant's generation. */}
          {formingLiveLine(lane?.forming) ? (
            <>
              <Loader2 size={10} className={cx('shrink-0 animate-spin', TONE_TEXT.warn)} />
              <span className={cx('truncate font-mono text-lz-mono', TONE_TEXT.warn)}>
                {formingLiveLine(lane?.forming)}
              </span>
            </>
          ) : running.length > 0 ? (
            <>
              <Loader2 size={10} className={cx('shrink-0 animate-spin', TONE_TEXT.warn)} />
              <span className={cx('truncate font-mono text-lz-mono', TONE_TEXT.warn)}>
                {inflightLiveLine(running)}
              </span>
            </>
          ) : lastCall ? (
            (() => {
              const cm = classifyCall(lastCall);
              return (
                <>
                  <CallTypeIcon icon={cm.icon} color={CALL_KIND_COLOR[cm.kind]} />
                  <span className="truncate text-lz-ink-3">
                    {cm.action}
                    {lastCall.summary ? <span className="font-mono text-lz-mono"> · {lastCall.summary}</span> : null}
                  </span>
                </>
              );
            })()
          ) : liveGen ? (
            <span className="truncate text-lz-ink-3">{liveGen}</span>
          ) : (
            <span className="text-lz-ink-3">generating…</span>
          )}
        </div>
      ) : null}
      {expanded && hasDetail ? (
        <div
          className={cx(
            'mb-2 ml-9 mr-3 space-y-2 border border-lz-border bg-lz-surface-2 px-3 py-2',
            RADIUS.control
          )}
        >
          {row.difficulty || row.deps.length || lane?.model ? (
            <div className="flex flex-wrap gap-x-3 text-lz-meta text-lz-ink-3">
              {row.difficulty ? (
                <span>
                  difficulty <span className="text-text-primary">{row.difficulty}</span>
                </span>
              ) : null}
              {row.deps.length ? (
                <span>
                  after <span className="font-mono text-text-primary">{row.deps.join(', ')}</span>
                </span>
              ) : null}
              {lane?.model ? (
                <span>
                  model <span className="font-mono text-text-primary">{lane.model}</span>
                </span>
              ) : null}
            </div>
          ) : null}
          {row.files && row.files.length ? (
            <div className="text-[11px] text-text-secondary break-words">
              <span className="font-lz-medium text-text-secondary">Files</span>{' '}
              {row.files.map((f, i) => (
                <span key={f}>
                  {i > 0 ? ', ' : ''}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      revealFile(f);
                    }}
                    disabled={!workingDir}
                    title={workingDir ? 'Reveal in Finder' : undefined}
                    className={`font-mono text-text-primary ${workingDir ? 'hover:underline cursor-pointer' : ''}`}
                  >
                    {f}
                  </button>
                </span>
              ))}
            </div>
          ) : null}
          {laneError ? (
            <div>
              <div className={cx('mb-1 text-lz-meta', TONE_TEXT.err)}>
                {interrupted ? 'Last error before it stalled' : 'Why it failed'}
              </div>
              <MonoOutput text={laneError} failed />
            </div>
          ) : null}
          {row.description ? <ReasoningBlock text={row.description} label="Task spec" reasoning={false} /> : null}
          {reasoning ? (
            <ReasoningBlock
              text={reasoning}
              live={row.state === 'running' && !interrupted}
              forceOpen={dev}
              label={row.state === 'running' ? 'Generating' : 'Reasoning'}
              note={(lane && narrativeClipNote(lane)) ?? undefined}
            />
          ) : null}
          {calls.length > 0 || running.length > 0 || (lane?.forming?.length ?? 0) > 0 ? (
            <div>
              <div className={cx('mb-1.5 text-lz-meta text-lz-ink-2', WEIGHT.medium)}>
                Tool calls · {lane?.toolCalls ?? calls.length}
                {running.length > 0 ? ` · ${running.length} running` : ''}
                {formingNote(lane?.forming)}
              </div>
              <div className={cx('border border-lz-border bg-lz-surface px-2 py-1', RADIUS.control)}>
                <FormingRows forming={lane?.forming} />
                <InflightRows running={running} />
                {calls.map((cl, i) => (
                  <CallRow
                    key={callMeta[i].key}
                    ordinal={callMeta[i].ordinal}
                    call={cl}
                    defaultOpen={dev || i === firstBadCall}
                  />
                ))}
              </div>
            </div>
          ) : null}
          {!lane && digest ? <TaskGenDetail digest={digest} /> : null}
          {row.judge && (row.judge.verdict || row.judge.hint) ? <JudgeReason judge={row.judge} /> : null}
        </div>
      ) : null}
    </div>
  );
};

/**
 * WORK zone — the ONE source of truth for "what is the plan, what is ongoing, what is done": the task
 * board (deriveTaskBoard) in three groups, each row expanding into its own detail card. This replaces the
 * old trio that showed the same tasks three ways (phase-checklist rows, a parallel lane list under a
 * "Drafting the plan" header that wasn't theirs, and feed lines) without saying which was authoritative.
 */
const WorkZone: React.FC<{
  board: TaskBoard;
  deviceOrder: string[];
  stale: boolean;
  dev: boolean;
  live: boolean;
  workingDir?: string;
  digests: Record<string, unknown>;
}> = ({ board, deviceOrder, stale, dev, live, workingDir, digests }) => {
  const total = board.running.length + board.queued.length + board.done.length;
  const failedCount = board.done.filter(
    (r) => r.state === 'failed' || r.state === 'judge_failed'
  ).length;
  const digestFor = (id: string) =>
    typeof digests[id] === 'object' && digests[id] !== null
      ? (digests[id] as Record<string, unknown>)
      : undefined;
  const rows = (list: BoardRow[]) => (
    <div className="divide-y divide-lz-border">
      {list.map((r) => (
        <BoardTaskRow
          key={r.id}
          row={r}
          deviceOrder={deviceOrder}
          stale={stale}
          dev={dev}
          workingDir={workingDir}
          digest={digestFor(r.id)}
        />
      ))}
    </div>
  );
  return (
    <div className="border-t border-lz-border">
      <ZoneHeader
        label="Work"
        explain="the plan as a live board — running, queued, done"
        count={total > 0 ? total : undefined}
        right={
          total > 0 ? (
            <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
              {board.running.length} running · {board.queued.length} queued · {board.done.length} done
              {board.addedByReplan > 0 ? ` · +${board.addedByReplan} re-planned` : ''}
            </span>
          ) : undefined
        }
      />
      {board.stuck ? (
        <div
          className={cx('mx-3 mb-2 flex items-center gap-1.5 px-2 py-1.5 text-lz-meta', TONE_FILL.err, RADIUS.control)}
        >
          <AlertTriangle className="size-3.5 shrink-0" />
          {board.stuck}
        </div>
      ) : null}
      {total === 0 ? (
        <div className="px-3 pb-2 text-lz-meta text-lz-ink-3">
          {live
            ? 'The plan lands here once it is agreed — tasks appear the moment the first one is dispatched.'
            : 'No tasks were dispatched in this run.'}
        </div>
      ) : (
        <>
          {board.running.length > 0 ? (
            <>
              <BoardGroupHeader label="Running" tone="warn" live={live} count={board.running.length} />
              {rows(board.running)}
            </>
          ) : null}
          {board.queued.length > 0 ? (
            <>
              <BoardGroupHeader
                label="Queued"
                tone="stopped"
                count={board.queued.length}
                extra={<span className="text-lz-meta text-lz-ink-3">— waiting on dependencies</span>}
              />
              {rows(board.queued)}
            </>
          ) : null}
          {board.done.length > 0 ? (
            <>
              <BoardGroupHeader
                label="Done"
                tone="ok"
                count={board.done.length}
                extra={
                  failedCount > 0 ? (
                    <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
                      · {failedCount} failed
                    </span>
                  ) : undefined
                }
              />
              {rows(board.done)}
            </>
          ) : null}
        </>
      )}
    </div>
  );
};

// When the planner's confidence is below the ask floor, the swarm BLOCKS and asks the user. This prompt is
// the interactive answer surface: the user types answers and we write them to the handshake file, which
// unblocks the run. Amber (solid, not faded) because the build is PAUSED waiting on the human.
/**
 * SEND A NOTE TO A BUILD THAT IS ALREADY RUNNING — the input half of the feature.
 *
 * The engine has read this inbox all along (swarm.rs read_user_notes: `.swarm/inbox/*.json`, sorted by an
 * epoch-ms filename, folded into the NEXT dispatched worker, never interrupting one already in flight, and
 * never deleted so a crash cannot lose one). It has 7 implementation sites and its own tests.
 * And the desktop had exactly TWO references to the feature: the type, and the settings toggle. There was
 * NO WAY TO SEND ONE. You could switch on "Let me add notes while it builds" and then discover the only
 * door was hand-writing JSON into a hidden directory. That is also why `user_notes` reads INERT in every
 * screened run — nobody could ever add a note.
 *
 * Claude Code's shape, because it is the right one: the box is always there while it works, you type, it
 * lands at the next turn boundary. No modal, no pause, no "are you sure".
 */
const NoteBox: React.FC<{ workingDir: string }> = ({ workingDir }) => {
  const [text, setText] = React.useState('');
  const [sent, setSent] = React.useState(0);
  const [busy, setBusy] = React.useState(false);
  const [failed, setFailed] = React.useState(false);
  const sendWhyId = useId();
  const sendWhy = busy
    ? 'Sending…'
    : text.trim()
      ? 'Send this note — it lands at the next task boundary'
      : 'Type a note to send';

  const send = async () => {
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    setFailed(false);
    // swarmAddNote, not writeFile: it mkdir -p's .swarm/inbox/, which NOTHING else creates — the
    // engine only reads it. A plain writeFile would fail on the very first note.
    const ok = await window.electron.swarmAddNote(workingDir, t).catch(() => false);
    setBusy(false);
    if (ok) {
      setText('');
      setSent((n) => n + 1);
    } else {
      setFailed(true);
    }
  };

  return (
    <div className="border-t border-lz-border px-3 py-2">
      <div className="flex items-center gap-2">
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              void send();
            }
          }}
          placeholder="Tell it something while it builds — picked up by the next task, never interrupts one"
          className={cx(
            'h-7 min-w-0 flex-1 border border-lz-border-strong bg-lz-surface px-2 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
            RADIUS.control,
            FOCUS
          )}
          aria-label="Add a note to this build"
        />
        {/* The panel's ONE primary action while it builds. Disabled is the solid neutral, never an opacity. */}
        <Button
          variant="primary"
          size="sm"
          onClick={() => void send()}
          disabled={!text.trim() || busy}
          title={sendWhy}
          aria-describedby={sendWhyId}
        >
          {busy ? 'Sending…' : 'Send'}
        </Button>
        <span id={sendWhyId} className="sr-only">
          {sendWhy}
        </span>
      </div>
      {failed ? (
        <div className={cx('mt-1 text-lz-meta', TONE_TEXT.err)}>
          Could not write the note. Is the build directory still there?
        </div>
      ) : sent > 0 ? (
        <div className="mt-1 text-lz-meta text-lz-ink-3">
          {sent === 1 ? '1 note queued' : `${sent} notes queued`} — it is background context, not an order:
          the spec still wins. Needs “Let me add notes while it builds” on.
        </div>
      ) : null}
    </div>
  );
};

/**
 * The ask that resolved WITHOUT the user. Only the timeout survives: the old proxy branches
 * (armed / answered / failed) rendered state that only the engine's DELETED proxy call could set —
 * clarify_proxy_* has no emitter left, so those cards were unreachable, and ClarifyProxy no longer
 * carries their fields. Archived runs' proxy history renders via buildPhaseTodo's ask rows instead.
 */
const ProxyNotice: React.FC<{ proxy: ClarifyProxy }> = ({ proxy }) => {
  if (!proxy.timedOut) return null;
  const { questions, waitedSecs } = proxy.timedOut;
  return (
    <div
      className={cx('flex items-start gap-2 px-2 py-2 text-lz-body', TONE_FILL.accent, RADIUS.control)}
      data-testid="clarify-timed-out"
    >
      <Bot className="mt-px size-3.5 shrink-0" />
      <span>
        {questions} open decision{questions === 1 ? '' : 's'} went unanswered at the {waitedSecs}s
        unattended window — every worker was told to choose the most conventional option and note
        the choice in a code comment. The build was never paused for it.
      </span>
    </div>
  );
};

const ClarifyPrompt: React.FC<{
  clarify: {
    pending: boolean;
    questions: Array<{ question: string; options: string[]; rationale?: string; resolves?: string }>;
    planConfidence?: number;
    confidence?: ConfidenceBreakdown | null;
    answerPath: string;
  };
  plan: PlanTask[];
  /** Who is going to answer these if you don't (see ProxyNotice). */
  proxy: ClarifyProxy;
  /** The engine's confidence floor — this prompt only exists BECAUSE the plan scored under it, so the
   *  breakdown must name the real bar rather than judge the number against an invented band. */
  askFloor?: number | null;
  /** Engine liveness at render time. The prompt is normally unmounted when stale (PlanningZone shows
   *  the interrupted notice), but liveness can flip between typing and sending — the send path must
   *  never claim goose is building off a write that succeeded into a dead run's directory. */
  stale?: boolean;
}> = ({ clarify, plan, proxy, askFloor = null, stale = false }) => {
  const [answers, setAnswers] = useState<string[]>(() => clarify.questions.map(() => ''));
  const [guidance, setGuidance] = useState('');
  const [showPlan, setShowPlan] = useState(true);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const uid = useId();

  const setAnswer = (i: number, v: string) => setAnswers((a) => a.map((x, j) => (j === i ? v : x)));

  const send = async () => {
    setBusy(true);
    setError(false);
    const ok = await window.electron
      .writeFile(clarify.answerPath, JSON.stringify({ answers, guidance }, null, 2))
      .catch(() => false);
    setBusy(false);
    if (ok) setSent(true);
    else setError(true);
  };

  if (sent) {
    // FINDING 12: the old banner said 'Sent — goose is building with your answers.' off nothing but a
    // successful FILE WRITE — the filesystem being fine says nothing about the engine, and a user
    // answered a SIGKILLed run into a green "building" claim. The write is the only fact this
    // component holds, so that is what it states; engine pickup shows up as the pending flag flipping
    // on the next poll (which unmounts this prompt) and the run moving in the feed.
    return stale ? (
      <div
        className="flex items-center gap-2 px-3 py-2 text-xs text-white"
        style={{ backgroundColor: SWARM_STATUS.solidStopped }}
      >
        <AlertTriangle className="h-4 w-4 shrink-0" />
        Answers written — but the engine is not running, so they will not be read until the run is
        relaunched.
      </div>
    ) : (
      <div
        className="flex items-center gap-2 px-3 py-2 text-xs text-white"
        style={{ backgroundColor: STATUS_COLOR.done }}
      >
        <Check className="h-4 w-4 shrink-0" />
        Answers written — waiting for goose to pick them up.
      </div>
    );
  }

  const canSend = answers.some((a) => a.trim().length > 0) || guidance.trim().length > 0;
  const sendWhy = busy
    ? 'Sending…'
    : canSend
      ? 'Send these answers and start the build'
      : 'Type an answer to at least one question, or some guidance, to send';
  return (
    <div className="border-b border-lz-border">
      <div className="flex items-center gap-2 px-3 py-2 text-white" style={{ backgroundColor: AMBER }}>
        <MessageCircleQuestion className="h-4 w-4 shrink-0" />
        <span className="text-xs font-lz-semibold">Review the plan &amp; steer the build</span>
        {typeof clarify.planConfidence === 'number' ? (
          <span className="text-[11px] tabular-nums">
            planner confidence {clarify.planConfidence}/100
          </span>
        ) : null}
      </div>
      <div className="px-3 py-3 space-y-3 bg-background-secondary">
        <ProxyNotice proxy={proxy} />
        <p className="text-xs text-text-secondary">
          Goose drafted this plan but wants your call on a few things before it builds. Pick an option, type
          your own, or just tell it what to change — it folds your input into the build.
        </p>

        {clarify.confidence ? (
          <div className="border border-border-primary px-2 py-2" style={{ borderRadius: CHIP_RADIUS }}>
            <ConfidenceBreakdownBody
              conf={clarify.confidence}
              hasPendingQuestions
              askFloor={askFloor}
            />
          </div>
        ) : null}

        {plan.length > 0 ? (
          <div className="border border-border-primary" style={{ borderRadius: CHIP_RADIUS }}>
            <button
              type="button"
              onClick={() => setShowPlan((s) => !s)}
              className="w-full flex items-center gap-1.5 px-2 py-1.5 text-[11px] text-text-secondary hover:text-text-primary"
            >
              {showPlan ? (
                <ChevronDown className="h-3 w-3" />
              ) : (
                <ChevronRight className="h-3 w-3" />
              )}
              Drafted plan · {plan.length} task{plan.length === 1 ? '' : 's'}
            </button>
            {showPlan ? (
              <ul className="px-2 pb-2 space-y-0.5">
                {plan.map((t) => (
                  <li key={t.id} className="text-[12px] leading-relaxed text-text-primary flex gap-1.5">
                    <span className="text-lz-ink-3 shrink-0">·</span>
                    <InlineMarkdown content={t.description || t.id} />
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}

        {clarify.questions.map((q, i) => (
          <div key={i} className="space-y-1.5">
            {/* The question IS the answer box's name: a placeholder vanishes on the first keystroke, and
                three boxes named "your answer…" are three boxes named nothing. */}
            <div id={`${uid}-q${i}`} className="text-xs text-text-primary font-lz-medium">
              {i + 1}. <InlineMarkdown content={q.question} />
            </div>
            {q.resolves ? (
              <div className="text-[11px] text-text-secondary flex items-start gap-1.5">
                <Info className="h-3 w-3 mt-0.5 shrink-0" style={{ color: BLUE }} />
                <span>
                  resolves: <span className="text-text-primary">{q.resolves}</span>
                </span>
              </div>
            ) : null}
            {q.options.length > 0 ? (
              <div className="flex flex-wrap gap-1.5">
                {q.options.map((opt) => {
                  const selected = answers[i] === opt;
                  return (
                    <button
                      key={opt}
                      type="button"
                      onClick={() => setAnswer(i, selected ? '' : opt)}
                      className={`text-[11px] px-2 py-1 border transition-colors ${
                        selected
                          ? 'text-white font-lz-medium'
                          : 'border-border-primary text-text-primary hover:border-text-secondary'
                      }`}
                      style={
                        selected
                          ? { backgroundColor: BLUE, borderColor: BLUE, borderRadius: CHIP_RADIUS }
                          : { borderRadius: CHIP_RADIUS }
                      }
                    >
                      {selected ? '✓ ' : ''}
                      {opt}
                    </button>
                  );
                })}
              </div>
            ) : null}
            <input
              type="text"
              aria-labelledby={`${uid}-q${i}`}
              value={q.options.includes(answers[i]) ? '' : answers[i]}
              onChange={(e) => setAnswer(i, e.target.value)}
              placeholder={q.options.length > 0 ? 'or type your own…' : 'your answer…'}
              className="w-full text-xs px-2 py-1.5 bg-background-primary text-text-primary border border-border-primary focus:outline-none focus:border-text-secondary"
              style={{ borderRadius: CHIP_RADIUS }}
            />
          </div>
        ))}

        <div className="space-y-1">
          <label htmlFor={`${uid}-guidance`} className="block text-xs text-text-primary font-lz-medium">
            Anything else? (optional)
          </label>
          <textarea
            id={`${uid}-guidance`}
            value={guidance}
            onChange={(e) => setGuidance(e.target.value)}
            rows={2}
            placeholder="Tell goose to change the plan however you like — e.g. “use SQLite, add an export command, skip the web UI”."
            className={cx(
              'w-full resize-y border border-lz-border-strong bg-lz-surface px-2 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
              RADIUS.control,
              FOCUS
            )}
          />
        </div>

        {error ? (
          <div className={cx('text-lz-body', TONE_TEXT.err)}>
            Couldn&apos;t write the answers file — check that the build directory is still there, then retry.
          </div>
        ) : null}

        <div className="flex flex-wrap items-center gap-2">
          {/* The panel's ONE primary while goose is asking (the note box never mounts beside this). */}
          <Button
            variant="primary"
            onClick={send}
            disabled={busy || !canSend}
            title={sendWhy}
            aria-describedby={`${uid}-send-why`}
            icon={busy ? <Loader2 className="animate-spin" /> : <Send />}
          >
            Send answers &amp; build
          </Button>
          <span id={`${uid}-send-why`} className="text-lz-meta text-lz-ink-3">
            {canSend ? '' : `${sendWhy}. `}
            The build is paused until you respond. Your answers guide every worker; the plan shape
            stays as drafted.
          </span>
        </div>
      </div>
    </div>
  );
};

/**
 * PLANNING zone — how the plan was agreed BEFORE building: the confidence gauge and its two signals, the
 * planning checklist (research / plan / contracts — every check an engine event), the candidate plan
 * drafts, and the clarify prompt when goose pauses to ask. HISTORICAL once building starts: the zone
 * collapses to its one-line summary (confidence pill · tasks · climb) so it stops dominating the view
 * mid-build — expandable any time; forced open while goose is waiting on the user's answers.
 */
const PlanningZone: React.FC<{
  conf: ConfidenceBreakdown | null;
  planConfidence: number | null;
  trail: number[];
  askFloor: number | null;
  clarify: SwarmRunState['clarify'];
  /** Who answers the open decisions — you, or goose from the spec (see ProxyNotice). */
  proxy: ClarifyProxy;
  plan: PlanTask[];
  /** The planning-history phases only (see PLANNING_PHASE_KEYS). */
  phases: PhaseTodo[];
  planLanes: TurnLane[];
  /** RESEARCH's per-slice lanes — one node per slice, each writing that module's spec (v1, archived). */
  sliceLanes: TurnLane[];
  /** The v2 research fan (research-<slice>-q<n>) — the live engine's Research work, one question each. */
  researchLanes: TurnLane[];
  /** CONTRACTS' per-module lanes — one node per module, each freezing that module's interface. */
  contractLanes: TurnLane[];
  /** The single-node planning calls: open / open-resplit / proxy-answer / synthesis / review / rate. */
  planningLanes: TurnLane[];
  deviceOrder: string[];
  stale: boolean;
  mode: SwarmLogMode;
  dev: boolean;
  buildStarted: boolean;
  workingDir?: string;
  activity?: Record<string, unknown>;
}> = ({
  conf,
  planConfidence,
  trail,
  askFloor,
  clarify,
  proxy,
  plan,
  phases,
  planLanes,
  sliceLanes,
  researchLanes,
  contractLanes,
  planningLanes,
  deviceOrder,
  stale,
  mode,
  dev,
  buildStarted,
  workingDir,
  activity,
}) => {
  const [openOverride, setOpenOverride] = useState<boolean | null>(null);
  const [laneOpen, setLaneOpen] = useState<Record<string, boolean>>({});
  // `clarify.pending` is a FILE test (questions exist, answers file absent) — on the proxyless
  // engine a timed-out ask never writes answers, so the file test says "pending" forever while
  // the run builds. The event stream is the truth: once the window expired (or an answer landed),
  // the prompt is history, not a request. And a DEAD engine (finding 12) makes the request itself a
  // lie — nothing is blocked on the answers — so staleness demotes the prompt to the interrupted
  // notice below rather than mounting an interactive form over a corpse.
  const clarifyPending = !!clarify?.pending && !proxy.timedOut && !stale;
  const clarifyInterrupted = !!clarify?.pending && !proxy.timedOut && stale;
  // A phase's own fan (the slice fan under RESEARCH, the contract fan under CONTRACTS) renders under that
  // phase, so the lanes say WHEN they ran; a phase with lanes but no checklist row yet still shows.
  const fanOf = (key: PhaseTodo['key']): PhaseLaneGroup | null => {
    const group = planningLanesFor(key, { sliceLanes, contractLanes, researchLanes });
    return group && group.lanes.length > 0 ? group : null;
  };
  const shownPhases = phases.filter((p) => p.items.length > 0 || fanOf(p.key) != null);
  // The generations that belong to no single phase, grouped by what they ARE.
  const laneGroups: Array<{ key: string; label: string; lanes: TurnLane[] }> = [
    { key: 'planning', label: 'Planning calls', lanes: planningLanes },
    { key: 'drafts', label: 'Candidate drafts', lanes: planLanes },
  ].filter((g) => g.lanes.length > 0);
  const laneGroupBlock = (key: string, label: string, lanes: TurnLane[]) => (
    <div key={key} className="mt-1">
      <div className="flex h-7 items-center gap-1.5 px-3">
        <Braces className="size-3 text-lz-ink-3" />
        <span className={cx(EYEBROW_CLASS, 'text-lz-ink-3')}>
          {label} · {lanes.length} lane{lanes.length === 1 ? '' : 's'}
          {lanes.some((l) => l.status === 'running') ? ' · thinking…' : ''}
        </span>
      </div>
      <div className="divide-y divide-lz-border">
        {lanes.map((lane) => {
          const defaultOpen = lane.status === 'running';
          const isOpen = laneOpen[lane.taskId] ?? defaultOpen;
          return (
            <LaneRow
              key={lane.taskId}
              lane={lane}
              deviceOrder={deviceOrder}
              stale={stale}
              open={isOpen}
              mode={mode}
              dev={dev}
              onToggle={() =>
                setLaneOpen((o) => ({ ...o, [lane.taskId]: !(o[lane.taskId] ?? defaultOpen) }))
              }
            />
          );
        })}
      </div>
    </div>
  );
  const hasBody =
    clarifyPending ||
    clarifyInterrupted ||
    !!conf ||
    !!proxy.timedOut ||
    shownPhases.length > 0 ||
    laneGroups.length > 0;
  if (!hasBody && planConfidence == null) return null;
  // Historical once build starts: collapse by default, keep the one-line summary in the header.
  // An INTERRUPTED ask defaults open (still collapsible, unlike a live ask): the notice explaining
  // that nothing is waiting on the answers is the whole point of the state.
  const open = clarifyPending ? true : (openOverride ?? (!buildStarted || clarifyInterrupted));
  const climb = trail.length >= 2 ? trail[trail.length - 1] - trail[0] : 0;
  const explain = buildStarted
    ? 'how the plan was agreed before building'
    : clarifyPending
      ? 'goose is asking you before it builds'
      : 'agreeing on the plan before building';
  return (
    <div className="border-t border-lz-border">
      <ZoneHeader
        label="Planning"
        explain={explain}
        collapsed={!open}
        onToggle={clarifyPending ? undefined : () => setOpenOverride((o) => !(o ?? !buildStarted))}
        right={
          <>
            {typeof planConfidence === 'number' ? (
              <ConfPill value={planConfidence} askFloor={askFloor} />
            ) : null}
            {climb > 0 ? (
              <Tip label={`Confidence climbed ${trail[0]} → ${trail[trail.length - 1]} as goose retargeted it.`}>
                <span
                  className="text-[11px] tabular-nums flex items-center gap-0.5 shrink-0"
                  style={{ color: STATUS_COLOR.done }}
                >
                  <TrendingUp className="h-2.5 w-2.5" /> +{climb}
                </span>
              </Tip>
            ) : null}
            {!open && plan.length > 0 ? (
              <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
                {plan.length} task{plan.length === 1 ? '' : 's'} planned
              </span>
            ) : null}
          </>
        }
      />
      {open ? (
        clarifyPending && clarify ? (
          <ClarifyPrompt clarify={clarify} plan={plan} proxy={proxy} askFloor={askFloor} stale={stale} />
        ) : (
          <div className="pb-1">
            {/* The ask the engine died holding (finding 12): the questions are real, the request is
                not — nothing is blocked on the answers any more. Said plainly instead of mounting the
                interactive form. */}
            {clarifyInterrupted ? (
              <div className="px-3 pt-2">
                <div
                  className={cx('flex items-start gap-2 px-2 py-2 text-lz-body', TONE_FILL.stopped, RADIUS.control)}
                  data-testid="clarify-interrupted"
                >
                  <AlertTriangle className="mt-px size-3.5 shrink-0" />
                  <span>
                    The run stopped while asking — the engine is not running, so nothing is waiting on
                    these answers. Relaunch the run from its directory to be asked again.
                  </span>
                </div>
              </div>
            ) : null}
            {/* The questions were settled without you. This is the durable record of that — the prompt
                itself unmounts the moment the answers file lands. */}
            {proxy.timedOut ? (
              <div className="px-3 pt-2">
                <ProxyNotice proxy={proxy} />
              </div>
            ) : null}
            {conf ? (
              <div className="px-3 py-2">
                <ConfidenceBreakdownBody
                  conf={conf}
                  trail={trail}
                  hasPendingQuestions={false}
                  askFloor={askFloor}
                />
              </div>
            ) : null}
            {shownPhases.map((p) => {
              const fan = fanOf(p.key);
              return (
                <div key={p.key} data-testid={`planning-phase-${p.key}`} data-phase-state={p.state}>
                  <div className="flex h-7 items-center gap-1.5 px-3">
                    <span className={cx('text-lz-meta text-lz-ink-2', WEIGHT.medium)}>{p.label}</span>
                    {p.counts.total > 0 ? (
                      <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
                        {p.counts.done}/{p.counts.total}
                      </span>
                    ) : null}
                  </div>
                  <div className="px-3 pl-4 space-y-0">
                    {p.items.map((item) => (
                      <PhaseTodoRow
                        key={item.id}
                        item={item}
                        deviceOrder={deviceOrder}
                        stale={stale}
                        activity={activity}
                        plan={plan}
                        workingDir={workingDir}
                      />
                    ))}
                  </div>
                  {fan ? laneGroupBlock(`${p.key}-fan`, fan.label, fan.lanes) : null}
                </div>
              );
            })}
            {laneGroups.map((group) => laneGroupBlock(group.key, group.label, group.lanes))}
          </div>
        )
      ) : null}
    </div>
  );
};

/**
 * KNOWN ACTIVE BUGS — what the run shipped IMPERFECT, on a run that passed.
 *
 * The engine rates each remaining defect CRITICAL or MINOR; zero criticals ships GREEN, and the minors are
 * carried out in `complete_result.known_active_bugs`. Without a surface they vanish: the run is green, the
 * overview is glowing, and the one honest thing left to say about the deliverable is nowhere on screen.
 *
 * These are deliberately NOT rendered as failures. Amber, its own heading, and a sentence that says the run
 * passed — a red list here would be a false red, which this panel exists to prevent as much as a false green.
 */
const KnownActiveBugs: React.FC<{ bugs: string[] }> = ({ bugs }) => {
  const [open, setOpen] = useState(true);
  if (bugs.length === 0) return null;
  return (
    <div className="border-t border-lz-border">
      <ZoneHeader
        label="Known active bugs"
        explain="the run passed — these are what it passed WITH"
        collapsed={!open}
        onToggle={() => setOpen((o) => !o)}
        right={<Chip tone="warn">{bugs.length}</Chip>}
      />
      {open ? (
        <ol className="space-y-1.5 px-3 pb-3">
          {bugs.map((bug, i) => (
            <li key={i} className="flex items-start gap-2 text-lz-body text-lz-ink">
              <Bug className={cx('mt-0.5 size-3.5 shrink-0', TONE_TEXT.warn)} aria-hidden />
              <span className="min-w-0 break-words">
                <InlineMarkdown content={bug} />
              </span>
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
};

// Mid-run Q&A (finding 10): a question dropped into .swarm/questions/ is answered by the engine into
// a hidden .swarm/answers/ dotfile — the swarm_answer event is the only surface a panel reader ever
// sees, and until this card existed the app dropped it.
const SwarmQA: React.FC<{ qa: SwarmRunState['qa'] }> = ({ qa }) => {
  const [open, setOpen] = useState(true);
  if (qa.length === 0) return null;
  return (
    <div className="border-t border-lz-border" data-testid="swarm-qa">
      <ZoneHeader
        label="Questions answered"
        explain="questions dropped to the running swarm, and its answers"
        collapsed={!open}
        onToggle={() => setOpen((o) => !o)}
        right={<Chip tone="accent">{qa.length}</Chip>}
      />
      {open ? (
        <ol className="space-y-2 px-3 pb-3">
          {qa.map((item, i) => (
            <li key={`${item.questionFile}-${i}`} className="text-lz-body text-lz-ink">
              <div className={cx('flex items-start gap-2', WEIGHT.semibold)}>
                <MessageCircleQuestion
                  className={cx('mt-0.5 size-3.5 shrink-0', TONE_TEXT.accent)}
                  aria-hidden
                />
                <span className="min-w-0 break-words">{item.question}</span>
              </div>
              <div className="ml-5 mt-1 flex items-start gap-2">
                <Bot className="mt-0.5 size-3.5 shrink-0 text-lz-ink-3" aria-hidden />
                <span className="min-w-0 whitespace-pre-wrap break-words">
                  <InlineMarkdown content={item.answer} />
                  {item.model ? (
                    <span className="ml-1 font-mono text-lz-mono text-lz-ink-3">— {item.model}</span>
                  ) : null}
                </span>
              </div>
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
};

// End-of-run OVERVIEW at DONE: what was built (grounded summary), how to run it (engine-stamped command),
// VERIFICATION (from phaseTodo — engine only, never the summary model), and what's next. An unverified ship
// LEADS with an amber caveat + hedged headers so three confident-looking lines can never out-shout the one
// caveat; a red build shows no summary at all. Mounts only on a clean `done` finish (guarded at the call site).
const RunOverview: React.FC<{
  overview: RunOverviewData;
  phaseTodo: PhaseTodo[];
  deviceOrder: string[];
  workingDir?: string;
}> = ({ overview, phaseTodo, deviceOrder, workingDir }) => {
  const verifyItems = phaseTodo.find((p) => p.key === 'integrate')?.items ?? [];
  const verified = verifyItems.find((i) => i.id === 'v-e2e')?.state === 'done';
  const hdr = cx('mb-1 mt-2 text-lz-meta text-lz-ink-2', WEIGHT.medium);
  return (
    <div className="space-y-1 border-t border-lz-border px-3 py-3">
      <div className={cx('flex items-center gap-1.5 text-lz-body text-lz-ink', WEIGHT.semibold)}>
        <ListChecks className="size-3.5" /> Build overview
        {workingDir ? (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto"
            onClick={() => void window.electron.revealInFinder(workingDir)}
            title="Reveal the build folder in Finder — every file this run wrote lives here"
            icon={<FolderOpen />}
          >
            Reveal build folder
          </Button>
        ) : null}
      </div>
      {/* RUNNABILITY IS AN ENGINE FACT. It is `verified` (phaseTodo's v-e2e — goose actually RAN the app)
          and runCommandVerified. It is NOT `generated`, which only says whether the model wrote the summary
          prose. Those were conflated: a run that finished 7/7 tasks with 0 failed, complete_result{passed,
          verified}, review{findings:[]} and run_command_verified:TRUE was shown a RED "This build did not
          reach a runnable, verified state" — because the summarizer stayed quiet. A false red is the same
          sin as a false green, and this panel exists to prevent exactly that. Ask the verify check first;
          a missing summary is a missing summary. */}
      {!verified ? (
        <div className={cx('mt-1 flex items-center gap-1.5 px-2 py-1.5 text-lz-meta', TONE_FILL.warn, RADIUS.control)}>
          <AlertTriangle className="size-3.5 shrink-0" />
          Not yet verified — the program was built but never run. Everything below describes the code, not
          proof it works.
        </div>
      ) : !overview.generated ? (
        <div
          className={cx(
            'mt-1 flex items-center gap-1.5 border border-lz-border px-2 py-1.5 text-lz-meta text-lz-ink-2',
            RADIUS.control
          )}
        >
          <ListChecks className="size-3.5 shrink-0" />
          goose ran this app and it works{overview.runCommand ? <> — <code className="font-mono text-lz-mono text-lz-ink">{overview.runCommand}</code></> : null}. It just
          didn&apos;t write up what it built; the verification below is the engine&apos;s own record.
        </div>
      ) : null}
      {overview.generated ? (
        <>
          {overview.features.length ? (
            <div>
              <div className={hdr}>
                {verified ? 'What was built' : 'What the code appears to do — not run or verified'}
              </div>
              <ul className="space-y-0.5">
                {overview.features.map((f, i) => (
                  <li key={i} className="flex gap-1.5 text-lz-body text-lz-ink">
                    <span className="shrink-0 text-lz-ink-3">·</span>
                    <span>{f}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
          <div>
            <div className={hdr}>How to run it</div>
            {overview.runCommand ? (
              <div className="flex items-center gap-2 flex-wrap">
                <code
                  className={cx(
                    'border border-lz-border bg-lz-surface-2 px-1.5 py-0.5 font-mono text-lz-mono text-lz-ink',
                    RADIUS.control
                  )}
                >
                  {overview.runCommand}
                </code>
                <Chip tone={overview.runCommandVerified ? 'ok' : 'stopped'}>
                  {overview.runCommandVerified ? 'verified to start' : 'candidate entry — not verified'}
                </Chip>
              </div>
            ) : (
              <div className="text-lz-body text-lz-ink">No standalone entry point — this runs inside goose.</div>
            )}
            {overview.engage ? (
              <div className="mt-0.5 text-lz-meta text-lz-ink-3">{overview.engage}</div>
            ) : null}
          </div>
        </>
      ) : null}
      <div>
        <div className={hdr}>Verification</div>
        {verifyItems.length ? (
          <div className="space-y-0">
            {verifyItems.map((item) => (
              <PhaseTodoRow key={item.id} item={item} deviceOrder={deviceOrder} stale={false} />
            ))}
          </div>
        ) : (
          <div className="text-lz-meta text-lz-ink-3">Verification gates were off this run.</div>
        )}
      </div>
      {overview.generated && overview.next.length ? (
        <div>
          <div className={hdr}>What&apos;s next</div>
          <ul className="space-y-0.5">
            {overview.next.map((n, i) => (
              <li key={i} className="flex gap-1.5 text-lz-body text-lz-ink">
                <span className="shrink-0 text-lz-ink-3">→</span>
                <span>{n}</span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
};

// The clear ENDING a run was missing: a solid terminal banner so a finished/stopped run never sits in limbo
// (tasks green, no "running", no "done"). Done = green, finished-with-failures = red, stopped-without-a-
// completion-signal (killed/crashed) = solid slate. Carries the tally + total time + the output directory.
const TerminalBanner: React.FC<{
  outcome: 'done' | 'failed' | 'stopped';
  summary: RunSummary | null;
  totals: { done: number; failed: number; tasks: number };
  durationLabel: string | null;
  outputDir?: string;
  deviceOrder: string[];
}> = ({ outcome, summary, totals, durationLabel, outputDir, deviceOrder }) => {
  const done = summary?.done ?? totals.done;
  const failed = summary?.failed ?? totals.failed;
  const tasks = totals.tasks;
  const cfg: { tone: Tone; Icon: typeof Check; title: string } = {
    done: { tone: 'ok' as Tone, Icon: Check, title: 'Build complete' },
    failed: {
      tone: 'err' as Tone,
      Icon: AlertTriangle,
      title: `Finished — ${failed} task${failed === 1 ? '' : 's'} failed`,
    },
    stopped: { tone: 'stopped' as Tone, Icon: CircleSlash, title: 'Run stopped' },
  }[outcome];
  const { tone, Icon, title } = cfg;
  const parts = [
    `${done}/${tasks} task${tasks === 1 ? '' : 's'} done`,
    outcome !== 'failed' && failed ? `${failed} failed` : null,
    durationLabel ? `in ${durationLabel}` : null,
  ].filter(Boolean);
  return (
    <div className="border-b border-lz-border">
      <div className={cx('flex items-center gap-2 px-3 py-2', TONE_FILL[tone])} data-testid="terminal-banner">
        <Icon className="size-4 shrink-0" strokeWidth={2.5} />
        <span className={cx('text-lz-body', WEIGHT.semibold)}>{title}</span>
        <span className={cx('text-lz-meta', TNUM)}>{parts.join(' · ')}</span>
      </div>
      {outcome === 'stopped' ? (
        <div className="px-3 py-1.5 text-lz-meta text-lz-ink-3">
          {/* Absorbs the liveness banner's exited explanation — outcome 'stopped' keys on the same
              EXITED heartbeat stamp, so both surfaces speaking would say one fact twice. */}
          The engine exited without a completion signal — it stopped writing its heartbeat and stamped
          an exit mid-build. What finished is shown below.
        </div>
      ) : null}
      {summary && summary.perDevice.length > 0 ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-lz-border px-3 py-1.5 text-lz-meta">
          {summary.perDevice.map((d) => {
            // Key the hue by the CANONICAL node name — d.device is the raw pool id, which is not in
            // deviceOrder, so every node collapsed onto the same out-of-range hue.
            const idx = deviceIndex(d.node, deviceOrder);
            return (
              <Tip
                key={d.device}
                label={
                  <span className="font-mono">
                    {d.device}
                    <br />
                    {d.dispatched} dispatch{d.dispatched === 1 ? '' : 'es'} · {d.toolCalls} tool calls · {fmtDuration(d.busyMs / 60000)} busy
                  </span>
                }
              >
                <span className={cx('inline-flex cursor-default items-center gap-1.5', TNUM)}>
                  <NodeDot index={idx} letter={String.fromCharCode(65 + (idx % 26))} />
                  <span className={cx('text-lz-ink', WEIGHT.semibold)}>{d.node}</span>
                  <span className="text-lz-ink-3">
                    {d.dispatched} task{d.dispatched === 1 ? '' : 's'} · {d.toolCalls} call
                    {d.toolCalls === 1 ? '' : 's'} · {fmtDuration(d.busyMs / 60000)}
                  </span>
                </span>
              </Tip>
            );
          })}
        </div>
      ) : null}
      {outputDir ? (
        <Tip label={<span className="font-mono break-all">{outputDir}</span>}>
          <div className="flex min-w-0 cursor-default items-center gap-1.5 border-t border-lz-border px-3 py-1.5 text-lz-meta text-lz-ink-3">
            <FolderOpen className="size-3 shrink-0" />
            <span className="truncate font-mono text-lz-mono text-lz-ink-3">{outputDir}</span>
          </div>
        </Tip>
      ) : null}
    </div>
  );
};

const DETAIL_MODE_LABEL: Record<SwarmLogMode, string> = {
  compact: 'Compact',
  verbose: 'Verbose',
  developer: 'Developer',
};

export function nextDetailMode(mode: SwarmLogMode, key: string): SwarmLogMode | null {
  const index = SWARM_LOG_MODES.indexOf(mode);
  if (key === 'Home') return SWARM_LOG_MODES[0];
  if (key === 'End') return SWARM_LOG_MODES[SWARM_LOG_MODES.length - 1];
  if (key === 'ArrowRight' || key === 'ArrowDown')
    return SWARM_LOG_MODES[(index + 1) % SWARM_LOG_MODES.length];
  if (key === 'ArrowLeft' || key === 'ArrowUp')
    return SWARM_LOG_MODES[(index - 1 + SWARM_LOG_MODES.length) % SWARM_LOG_MODES.length];
  return null;
}

const DETAIL_MODE_OPTIONS = SWARM_LOG_MODES.map((option) => ({
  value: option,
  label: DETAIL_MODE_LABEL[option],
}));

/** How much of the run the panel shows. The lz Segmented — a real radiogroup with all three choices
 *  visible and roving focus (arrows move AND select, Home/End jump; `nextDetailMode` above is the same
 *  key map, kept as the documented contract). The old control was one button that cycled, so the two
 *  modes you were not in were invisible and unreachable by keyboard. */
export const DetailModeChooser: React.FC<{
  mode: SwarmLogMode;
  onChange: (mode: SwarmLogMode) => void;
}> = ({ mode, onChange }) => (
  <Segmented<SwarmLogMode>
    aria-label="Run detail"
    size="sm"
    options={DETAIL_MODE_OPTIONS}
    value={mode}
    onChange={onChange}
  />
);

export const SwarmRunPanel: React.FC<{
  workingDir: string | undefined;
  /** The run state, when the host already polls it (BaseChat does, for the workspace split). Passing it
   *  keeps ONE poller per run: mounting a second `useSwarmRun` on the same directory doubled the IPC and
   *  let the two copies disagree about the phase for a poll at a time. */
  run?: SwarmRunState;
  className?: string;
}> = ({ workingDir, run: providedRun, className = '' }) => {
  const observedRun = useSwarmRun(providedRun ? undefined : workingDir);
  const run = providedRun ?? observedRun;
  // The run's OWN directory. The engine redirects the build out of the spawn dir when that dir is
  // $HOME, so everything run-relative — the pause sentinel, the notes inbox, activity file paths —
  // must target this. Passing the session dir instead writes where the engine never looks.
  const runDir = run.runDir ?? workingDir;
  // TRUTH and DISPLAY are separate feeds (U-H2). `useFleetCorroboration` polls LM Studio (and the local
  // MLX sidecar when one is configured) ALWAYS, because deriveFleet's dead-lane demotion is a truth and
  // a display toggle must never switch off a truth. The 'showLmStudioFleet' setting (default off) gates
  // only the LM Studio DOT the fleet rows display; off, `nodeStatus` is `{}` and the rows read exactly
  // as when lms is unavailable.
  const lmStudioVisible = useLmStudioFleetVisible();
  const corroboration = useFleetCorroboration(1500);
  const nodeStatus: Record<string, string> = lmStudioVisible ? corroboration.nodeStatus : {};
  const [mode, setMode] = useSwarmLogMode();
  const verbose = mode !== 'compact';
  const dev = mode === 'developer';

  // Show whenever a run is present — including the PLANNING phase, before any worker executes (no lanes yet).
  if (!run.present) return null;

  // Stable node identity: run.lanes RE-SORTS every poll (running first, then recency), so deriving letters
  // from first-seen lane order made a node's letter/hue flicker between polls. deriveFleet keys the row set
  // off the RESOLVED POOL (pool_resolved / run_started.pool), sorted deterministically, so EVERY fleet node
  // renders — idle ones as an explicit idle row, never absence — and the node's dot/hue is fixed for the whole run.
  // ALL lane kinds count toward WORKING in every mode (scouts/contracts/detailers/repair twins included):
  // the mode toggle controls display density below, not whether a busy node reads busy.
  const laneSources = [
    ...run.lanes,
    ...run.planLanes,
    ...run.scoutLanes,
    ...run.contractLanes,
    ...run.detailLanes,
    // The rewritten pipeline's own lanes: the slice fan (one node per slice, RESEARCH), the v2
    // research fan, and the single-node planning calls (open / synthesis / review / rate). Without
    // them the Fleet zone reads "idle — no task" for the entire planning half of the run.
    ...run.sliceLanes,
    ...run.researchLanes,
    ...run.planningLanes,
    ...run.fixLanes,
  ];
  const fleet = deriveFleet({
    pool: run.pool,
    laneSources,
    digests: run.activityDigests,
    digestMtimes: run.activityMtimes,
    now: Date.now(),
    // SUPERVISION: open judge spans (foldSupervision) joined to the nodes LM Studio itself reports busy —
    // the workload class that used to render a hard-working node as "idle — no task".
    supervision: run.supervision,
    busyNodes: corroboration.busyNodes,
    // EVERY node a feed replied about, idle ones included — what arms the dead-lane demotion when the
    // whole fleet is idle (busyNodes [] alone cannot tell "all idle" from "lms unreachable"). From the
    // corroboration feed, never from the display-gated `nodeStatus`: on a default install that was `{}`
    // and a lane the engine opened and never closed rendered a dead node as "working" for as long as
    // the panel stayed open.
    reportedNodes: corroboration.reportedNodes,
    // Channel-memory scope for the laneless digest rows — same discriminant as the hook's fold scope.
    scope: workingDir,
  });
  // The cumulative per-node call history — the same laneSources and gated digests deriveFleet reads,
  // kept beside it rather than inside it so the NOW derivation and the LOG derivation stay separately
  // testable. Everything here persists on disk after a lane finishes; only the UI ever forgot it.
  const nodeHistory = deriveNodeHistory({
    laneSources,
    digests: run.activityDigests,
    digestMtimes: run.activityMtimes,
    // The interrupted-row liveness test reads the clock; per-render is exactly the cadence the
    // deriveFleet call above already accepts.
    now: Date.now(),
    scope: workingDir,
  });
  const deviceOrder: string[] = fleet.devices;
  // The WORK board — the single source of truth for plan / ongoing / done (see deriveTaskBoard).
  const board = deriveTaskBoard({
    plan: run.plan,
    phaseTodo: run.phaseTodo,
    lanes: run.lanes,
    fixLanes: run.fixLanes,
  });
  // The planning phases live in the PLANNING zone; build / integrate / repair ARE the work board.
  const planningPhases = run.phaseTodo.filter((p) => isPlanningPhase(p.key));
  // Engine truth, not a parsed label: the run is past planning once the plan is loaded (runPhase) or a task
  // has been dispatched.
  const buildStarted =
    run.totals.tasks > 0 ||
    run.runPhase === 'build' ||
    run.runPhase === 'integrate' ||
    run.runPhase === 'repair' ||
    run.runPhase === 'done';
  const appName = runAppName(run.meta?.prompt, runDir);

  // ENGINE liveness — the heartbeat file only. There is no activity-mtime fallback any more: a quiet digest
  // is what a SLOW LOCAL MODEL looks like, and every cap was removed from the engine precisely so a slow
  // model is never cut. This flags a dead ENGINE (the heartbeat ticks every 5s regardless of model speed)
  // and it is NON-TERMINAL: it dims in-flight lanes and raises a banner, and never decides a run is over.
  const now = Date.now();
  const liveness = engineLiveness(run, now);
  const stale = isEngineSilent(run, now);
  const { running, done, failed, tasks } = run.totals;

  const clarifyPending = !!run.clarify?.pending && !run.proxy.timedOut;
  // The APP-LEVEL oracle: the engine's own end-to-end verify (complete_result -> phaseTodo v-e2e = 'done').
  // A green verify means the deliverable WORKS — so the run is 'done' and the overview shows — EVEN IF an
  // individual build task failed (e.g. the integrate-verify sink stalled but the orchestrator's verify still
  // passed). Without this, one failed task forced outcome='failed', which suppressed the overview and headlined
  // "1 task failed" on a working, verified app. The failed task stays visible in the Build phase-todo + counts.
  const appVerified =
    (run.phaseTodo.find((p) => p.key === 'integrate')?.items ?? []).find((i) => i.id === 'v-e2e')
      ?.state === 'done';
  // A run is OVER when the engine said so, and the engine says so TWO ways: run_finished, or the
  // heartbeat's `EXITED:` stamp (Drop ran — nothing more will ever be written). The old derivation
  // defined ended = run.finished and then asked `ended && !run.finished` for 'stopped', which is
  // contradictory — the 'Run stopped' terminal banner was dead code, and a killed run sat on the amber
  // warning with 'N interrupted' forever, the exact limbo the banner was built to end. 'stopped' keys
  // ONLY on the exit stamp, never on 'silent': a frozen heartbeat can be a hard-killed engine the
  // operator resumes, and no clock may end a run. (Benign one-poll race: the EXITED stamp can be read
  // one poll before the run_finished line, flipping a momentary 'stopped' to 'done' on the next fold —
  // accepted, not a flicker bug.)
  const outcome: 'done' | 'failed' | 'stopped' | null = run.finished
    ? appVerified || (run.summary?.failed ?? failed) === 0
      ? 'done'
      : 'failed'
    : liveness.state === 'exited'
      ? 'stopped'
      : null;
  const ended = outcome != null;
  const durationMin =
    run.summary?.totalMin != null
      ? run.summary.totalMin
      : run.startedAt != null && run.mtime != null
        ? (run.mtime - run.startedAt) / 60000
        : null;
  const durationLabel = durationMin != null ? fmtDuration(durationMin) : null;
  const activePhaseTone: FormationActiveTone = ended
    ? outcome === 'done'
      ? 'ok'
      : outcome === 'failed'
        ? 'err'
        : 'stopped'
    : 'accent';
  // The ribbon's fleet is the SAME truth the Fleet zone renders — an open lane per deriveFleet, or LM
  // Studio's own generating/prompt-processing signal for a node whose work has no lane.
  const formationNodes = deviceOrder.map((device) => {
    const liveStatus = nodeStatus[device];
    return {
      device,
      working:
        run.inProgress &&
        !stale &&
        !ended &&
        !run.held &&
        !clarifyPending &&
        (fleet.workingByDevice.has(device) ||
          liveStatus === 'generating' ||
          liveStatus === 'processingPrompt'),
    };
  });

  return (
    <div
      data-testid="swarm-run-panel"
      className={cx(SURFACE.card, 'overflow-hidden text-lz-body', className)}
    >
      {/* ── RUN HEADER zone — identity + state in ONE band: what is being built, the phase, counts,
          elapsed/ETA, pause + display mode. Replaces the floating fragments (brand pill, metrics strip,
          breadcrumb) Mihai read as "lacking any visual definition". Mission control: the eyebrow is the
          zone register, the run name is the h2 step, the phase is ONE Chip tone, the numbers are tabular. */}
      <div className="border-b border-lz-border">
      <div className="flex items-center justify-between gap-3 px-3 py-2">
        <span className="flex min-w-0 items-center gap-2">
          <span className={cx(EYEBROW_CLASS, 'shrink-0 text-lz-ink-3')}>Swarm run</span>
          <Tip
            label={
              run.meta?.prompt ? (
                <span className="whitespace-pre-wrap">{run.meta.prompt.slice(0, 400)}</span>
              ) : (
                'What this run is building'
              )
            }
          >
            <span className={cx(TYPE.h2, 'truncate')}>{appName}</span>
          </Tip>
          {run.meta?.resumed ? (
            // FINDING 7: a resumed run skips planning and re-runs finished tasks — without this badge
            // that read as a broken run doing duplicate work. Solid chip, engine-truth (run_resumed).
            <Tip
              label={`Resumed — reused the previous plan (${run.meta.resumed.tasks} task${run.meta.resumed.tasks === 1 ? '' : 's'}); planning skipped; ${run.meta.resumed.previouslyCompleted} finished task${run.meta.resumed.previouslyCompleted === 1 ? '' : 's'} re-run.`}
            >
              <span className="inline-flex shrink-0" data-testid="run-resumed-chip">
                <Chip tone="accent" icon={<RotateCcw />}>
                  Resumed
                </Chip>
              </span>
            </Tip>
          ) : null}
          {clarifyPending && !stale ? (
            // Paused waiting on the human — NOT active work, so no spinner and a distinct amber "paused" chip
            // (the old code showed a spinning "Planning" here, implying it was still churning).
            // Gated on !stale (finding 12): a SIGKILLed engine mid-ask left this chip claiming "the
            // build is paused, waiting for your answers" directly above the no-heartbeat banner —
            // nothing was waiting, and nothing would ever read the answers. When the engine is dead
            // the liveness banner is the honest surface, not an invitation to answer.
            <Tip label="The build is paused, waiting for your answers in the prompt below.">
              <span className="inline-flex shrink-0">
                <Chip tone="warn" icon={<MessageCircleQuestion />}>
                  Waiting for you
                </Chip>
              </span>
            </Tip>
          ) : run.held ? (
            // HELD — engine truth (run_paused with no later run_unpaused). NO SPINNER: a spinning badge is a
            // claim that work is happening, and while held every node is deliberately idle. MEASURED: Mihai
            // watched this badge read a spinning "Building" through a 20-minute hold and reasonably concluded
            // the run had hung. The phase label is suppressed too — "which task is next" is not the question
            // someone asks when nothing is moving.
            // When STALE (finding 14), 'press ▶ to resume' is a promise nothing can keep: resume only
            // removes a sentinel no process is watching. Say the engine died while held instead.
            <Tip
              label={
                stale
                  ? 'The engine died while held — resuming requires relaunching the run from its directory.'
                  : 'Held at a task boundary. In-flight work finished and nothing was lost — press ▶ to resume.'
              }
            >
              <span className="inline-flex shrink-0">
                <Chip tone={stale ? 'stopped' : 'warn'} icon={<Pause />}>
                  {stale ? 'Paused — engine gone' : 'Paused'}
                </Chip>
              </span>
            </Tip>
          ) : run.inProgress && !stale && !ended && run.phase ? (
            <Tip label={`Current phase: ${run.phase}`}>
              <span className="inline-flex shrink-0">
                <Chip tone="accent" icon={<Loader2 className="animate-spin" />}>
                  {run.phase}
                </Chip>
              </span>
            </Tip>
          ) : ended && outcome ? (
            // The run's END as the same chip: ok when the build verified, err when a task failed, the
            // stopped slate for an engine that exited without a completion signal. The terminal banner
            // below carries the tally; this is the one-word state beside the name.
            <span className="inline-flex shrink-0" data-testid="run-outcome-chip">
              <Chip
                tone={outcome === 'done' ? 'ok' : outcome === 'failed' ? 'err' : 'stopped'}
                icon={outcome === 'done' ? <Check /> : outcome === 'failed' ? <AlertTriangle /> : <CircleSlash />}
              >
                {outcome === 'done' ? 'Done' : outcome === 'failed' ? 'Failed' : 'Stopped'}
              </Chip>
            </span>
          ) : null}
        </span>
        <span className="flex shrink-0 items-center gap-2">
          <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
            {tasks > 0 && (
              <>
                {running > 0 && (
                  <span className={cx(WEIGHT.semibold, stale ? TONE_TEXT.stopped : TONE_TEXT.warn)}>
                    {running} {stale ? 'interrupted' : 'running'}
                  </span>
                )}
                {running > 0 && ' · '}
                <span className={TONE_TEXT.ok}>{done} done</span>
                {failed > 0 && (
                  <>
                    {' · '}
                    <span className={cx(WEIGHT.semibold, TONE_TEXT.err)}>{failed} failed</span>
                  </>
                )}
                {' · '}
                {tasks} tasks ·{' '}
              </>
            )}
            {run.mtime ? (
              <Tip label={`Last activity: ${new Date(run.mtime).toLocaleString()}`}>
                <span>{ago(run.mtime)}</span>
              </Tip>
            ) : null}
          </span>
          {run.inProgress && !stale && !ended && !clarifyPending && !run.sourceMissing ? (
            // An UNOBSERVABLE run has no elapsed to tick: its files no longer resolve, so nothing below
            // can advance and a counting clock would claim a run that cannot be seen.
            <HeaderMetrics startedAt={run.startedAt} phaseTodo={run.phaseTodo} />
          ) : null}
          {run.inProgress && !ended && !clarifyPending && workingDir ? (
            // PAUSE / RESUME — hold the build at the next task boundary (in-flight work finishes, nothing is
            // lost) and resume re-running nothing. An ACTION, so the secondary Button (never the warn colour,
            // never red, so it never reads as a state or as the terminal ■ stop). "Held" is engine-truth
            // (run_paused event); "Pausing…" is the pending request, and the label carries it.
            <Button
              variant="secondary"
              size="sm"
              aria-pressed={run.pauseRequested}
              onClick={() => runDir && window.electron.swarmSetPaused(runDir, !run.pauseRequested)}
              icon={
                !run.pauseRequested ? (
                  <Pause />
                ) : run.held ? (
                  <Play />
                ) : stale ? (
                  <Pause />
                ) : (
                  <Loader2 className="animate-spin" />
                )
              }
              title={
                !run.pauseRequested
                  ? 'Hold at the next task boundary — in-flight work finishes, nothing is lost'
                  : run.held
                    ? stale
                      ? 'The engine died while held — this only clears the pause sentinel; resuming requires relaunching the run.'
                      : 'Resume the build (re-runs nothing)'
                    : stale
                      ? liveness.state === 'exited'
                        ? 'The engine exited — the pause request was written but nothing is reading it.'
                        : 'Engine not responding — most likely hard-killed; the pause sentinel is written but nothing is reading it.'
                      : 'Pausing — finishing the current task, then holding. Click to resume.'
              }
            >
              {/* FINDING 14: an animate-spin 'Pausing…' asserts live progress. On a dead engine the
                  request stands but nothing is finishing anything — a static icon and a label that
                  says which death this is (the button itself stays: the sentinel toggle is still
                  meaningful for a later relaunch). */}
              {!run.pauseRequested
                ? 'Pause'
                : run.held
                  ? 'Resume'
                  : stale
                    ? 'Pause requested'
                    : 'Pausing…'}
            </Button>
          ) : null}
          <DetailModeChooser mode={mode} onChange={setMode} />
        </span>
      </div>
      {/* Row 2 of the band: the run's ROUTE and its real fleet in one formation — which engine phase is
          live, and which nodes are working under it. The active step is the engine's own phase key; a
          held run keeps its position but the chip renders held (grey outline, no fill) — asserting no
          work WITHOUT erasing the completed phases behind it. */}
      {(run.inProgress || ended) && (
        <FormationRibbon
          phase={run.runPhase}
          nodes={formationNodes}
          evidence={run.runPhasesObserved}
          held={run.held}
          activeTone={activePhaseTone}
        />
      )}
      </div>

      {/* The engine's own liveness, as a WARNING and never a verdict. `EXITED:` in .swarm/heartbeat means the
          run future returned early and tore itself down; a frozen stamp means the process was hard-killed.
          Neither ends the run here — the panel keeps showing everything it had. */}
      {!ended && (liveness.state === 'exited' || liveness.state === 'silent') ? (
        <div
          className={cx('flex items-start gap-2 px-3 py-2 text-lz-body', TONE_FILL.warn)}
          data-testid="liveness-banner"
        >
          <AlertTriangle className="mt-px size-4 shrink-0" />
          <span>
            {liveness.state === 'exited'
              ? 'The engine exited on its own — it stopped writing its heartbeat and stamped an exit. Everything below is what it had reached.'
              : `No heartbeat for ${Math.round(liveness.since / 1000)}s. The engine ticks every 5s, so it was most likely hard-killed; nothing below has been discarded.`}
          </span>
        </div>
      ) : null}

      {/* THE RUN'S FILES ARE GONE (U-M5): readSwarmRun answered null after a run was on screen — archived
          or deleted. The hook nulls the heartbeat so liveness reads UNKNOWN (never "hard-killed"); this
          band is the distinct state that says so, over the last state read. Presence-based, never a timer. */}
      {run.sourceMissing ? (
        <div className="px-3 pt-3" data-testid="run-source-missing">
          <Panel>
            <div className="flex items-center gap-2 text-lz-body text-lz-ink">
              <StatusDot tone="stopped" label="Run files no longer resolve" />
              <span>
                This run&apos;s files no longer resolve (archived or deleted) — showing the last state read.
              </span>
            </div>
          </Panel>
        </div>
      ) : null}

      {/* While it is actually building — not ended, and not already blocked on a clarify prompt (that one
          is a question awaiting YOUR answer; two input boxes at once would be a puzzle). */}
      {!ended && !clarifyPending && runDir ? <NoteBox workingDir={runDir} /> : null}

      {ended && outcome ? (
        <TerminalBanner
          outcome={outcome}
          summary={run.summary}
          totals={{ done, failed, tasks }}
          durationLabel={durationLabel}
          outputDir={workingDir}
          deviceOrder={deviceOrder}
        />
      ) : null}

      {/* The end-of-run overview only on a clean DONE — a stopped/crashed run never emits it and never mounts. */}
      {ended && outcome === 'done' && run.overview ? (
        <RunOverview
          overview={run.overview}
          phaseTodo={run.phaseTodo}
          deviceOrder={deviceOrder}
          workingDir={runDir}
        />
      ) : null}

      {/* The imperfections a green run shipped with. Mounted whenever the engine has rated defects — during
          the repair phase as well as at the end, because that is when they are actionable. */}
      <KnownActiveBugs bugs={run.knownActiveBugs} />

      {/* Mid-run questions the swarm answered (finding 10) — otherwise the answers live only in dotfiles. */}
      <SwarmQA qa={run.qa} />

      {/* ── PLANNING zone — the confidence story + planning checklist + candidate drafts, and the clarify
          prompt when goose is asking. Collapses to its one-line summary once building starts. */}
      <PlanningZone
        conf={run.confidence}
        planConfidence={run.planConfidence}
        trail={run.confidenceTrail}
        askFloor={run.askFloor}
        clarify={run.clarify}
        proxy={run.proxy}
        plan={run.plan}
        phases={planningPhases}
        planLanes={run.planLanes}
        sliceLanes={run.sliceLanes}
        researchLanes={run.researchLanes}
        contractLanes={run.contractLanes}
        planningLanes={run.planningLanes}
        deviceOrder={deviceOrder}
        stale={stale}
        mode={mode}
        dev={dev}
        buildStarted={buildStarted}
        workingDir={runDir}
        activity={run.activityDigests}
      />

      {/* ── FLEET zone — the fixed realtime per-node rows, now under the same header register. */}
      {deviceOrder.length > 0 ? (
        <div className="border-t border-lz-border">
          <ZoneHeader
            label="Fleet"
            explain="what each node is doing right now"
            count={deviceOrder.length}
            right={
              run.inProgress && !stale && !ended ? (
                <span className={cx('text-lz-meta text-lz-ink-3', TNUM)}>
                  {fleet.workingByDevice.size} working
                </span>
              ) : undefined
            }
          />
          <FleetStrip
            deviceOrder={deviceOrder}
            runningByDevice={fleet.workingByDevice}
            alsoRunningByDevice={fleet.alsoRunningByDevice}
            dev={dev}
            live={run.inProgress && !stale && !ended}
            nodeStatus={nodeStatus}
            unattributed={fleet.unattributed}
            historyByDevice={nodeHistory}
            runDir={runDir ?? ''}
          />
        </div>
      ) : null}

      {/* ── WORK zone — the one task board: running / queued / done, rows expanding into their own cards. */}
      <WorkZone
        board={board}
        deviceOrder={deviceOrder}
        stale={stale}
        dev={dev}
        live={run.inProgress && !stale && !ended}
        workingDir={runDir}
        digests={run.activityDigests}
      />

      {/* ── EVENT LOG zone — the chronological engine narrative, subordinate and collapsed by default in
          compact mode (judge verdicts + failures already surface on the WORK rows). */}
      <EventLogZone
        items={verbose ? run.verboseActivity : run.activity}
        live={run.inProgress && !stale && !ended}
        verbose={verbose}
        workingDir={runDir}
      />
    </div>
  );
};

export default SwarmRunPanel;
