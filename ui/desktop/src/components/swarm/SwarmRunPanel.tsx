import React, { useState, useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import {
  Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench,
  Search, ListChecks, Play, Pause, FlaskConical, RotateCcw, Gavel, Eye, FileText, Cpu, AlignLeft,
  MessageCircleQuestion, Send, Gauge, AlertTriangle, FolderOpen, TrendingUp, Info, Braces,
  Circle, Minus, Terminal, FilePlus2, FilePenLine, Hammer,
  MessageSquare, Bot, Bug,
} from 'lucide-react';
import {
  useSwarmRun,
  deriveFleet,
  deriveTaskBoard,
  runAppName,
  classifyCall,
  collapseRepeats,
  substantiveChunk,
  resolveActivityPath,
  type TurnStatus,
  type TurnLane,
  type SwarmCall,
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
  type SwarmRunState,
  type ClarifyProxy,
  type RunOverview as RunOverviewData,
} from './useSwarmRun';
import { ZoneHeader, ZONE_HUES } from './ZoneHeader';
import { SWARM_LOG_MODES, useSwarmLogMode, type SwarmLogMode } from './useVerboseSwarm';
import { useFleetStatus } from './useFleet';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';
import InlineMarkdown from './InlineMarkdown';
import StructuredContent, { CodeBlock } from './StructuredContent';
import FormationRibbon from './FormationRibbon';
import {
  CHIP_RADIUS,
  EYEBROW_CLASS,
  FORMATION_INK,
  FORMATION_RAMP,
  PANEL_RADIUS,
  SWARM_STATUS,
  nextRevealedText,
  usePrefersReducedMotion,
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
const CALL_OK = SWARM_STATUS.done;
const CALL_ERR = SWARM_STATUS.error;
const CALL_PENDING = SWARM_STATUS.stopped;
const AMBER = SWARM_STATUS.running;
const BLUE = SWARM_STATUS.action;
// A solid slate for a run that stopped without finishing — neutral (not an error) but dark enough for white
// banner text. Distinct from the amber "running" and red "failed".
const STOPPED = SWARM_STATUS.stopped;
// Body colour for MODEL-GENERATED text (live generations, reasoning). The primary text token — solid, never
// a tint or an opacity fade. Chrome (labels, counts, hints) deliberately stays on the secondary token.
const GEN_TEXT = 'var(--color-text-primary)';

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

function ago(mtime: number | null): string {
  if (!mtime) return '';
  const s = Math.max(0, Math.round((Date.now() - mtime) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  return m < 60 ? `${m}m ago` : `${Math.round(m / 60)}h ago`;
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
            className="text-[10px] text-text-secondary hover:text-text-primary transition-colors"
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
          className="text-[10px] text-text-secondary hover:text-text-primary transition-colors"
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
const CallRow: React.FC<{ call: SwarmCall; defaultOpen?: boolean }> = ({ call, defaultOpen }) => {
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
    <div className="py-0.5 border-b border-border-primary last:border-0">
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
        className={`w-full flex items-start gap-2 text-left ${hasOutput ? 'cursor-pointer hover:opacity-80' : 'cursor-default'}`}
      >
        <span className="mt-0.5">
          <CallTypeIcon icon={m.icon} color={color} />
        </span>
        <span className="flex-1 min-w-0">
          <span className="flex items-center gap-1.5 flex-wrap">
            <span className="text-xs font-medium text-text-primary">{m.action}</span>
            {pill ? (
              <span
                className="shrink-0 font-mono text-[9px] uppercase tracking-wide px-1 py-px"
                style={{ color, border: `1px solid ${color}`, borderRadius: CHIP_RADIUS }}
              >
                {pill}
              </span>
            ) : null}
            {m.kind !== 'ok' ? (
              <span className="text-[10px]" style={{ color: m.kind === 'malformed' ? color : 'var(--color-text-secondary)' }}>
                {m.outcome}
              </span>
            ) : null}
          </span>
          {call.summary ? (
            <span className="block font-mono text-[10px] text-text-secondary break-words mt-px" title={call.summary}>
              {call.summary}
            </span>
          ) : null}
        </span>
        {hasOutput &&
          (open ? (
            <ChevronDown className="h-3 w-3 shrink-0 text-text-secondary mt-0.5" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0 text-text-secondary mt-0.5" />
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
}> = ({ text, forceOpen, label, live }) => {
  const [expandedState, setExpanded] = useState(false);
  const expanded = expandedState || !!forceOpen;
  const words = text.split(/\s+/).filter(Boolean).length;
  const big = text.length > 1200;
  const bodyRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!live) return;
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text, live]);
  const capped = !expanded && big;
  return (
    <div>
      <div className="flex items-center gap-1.5 mb-1.5">
        <span className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary">
          {label || 'Reasoning'}
        </span>
        {live ? (
          <span className="text-[9px] font-bold uppercase tracking-[0.08em]" style={{ color: CALL_OK }}>
            live
          </span>
        ) : null}
      </div>
      <div
        ref={bodyRef}
        className={`text-[13px] break-words bg-background-primary border border-border-primary px-2.5 py-2 ${capped ? (live ? 'max-h-[22rem] overflow-y-auto' : 'max-h-[22rem] overflow-hidden') : ''}`}
        style={{ borderRadius: CHIP_RADIUS, lineHeight: 1.65, color: GEN_TEXT }}
      >
        {/* Prose gets the markdown path; a STRUCTURED payload gets a code path. The plan skeleton used to
            arrive here as raw JSON and markdown both reflowed it into an unreadable wall AND corrupted it —
            `__init__.py` reads as bold syntax, so the file list rendered as **init**.py. */}
        <StructuredContent content={collapseRepeats(text)} />
      </div>
      {big && (
        <button
          onClick={() => setExpanded((e) => !e)}
          className="mt-0.5 text-[10px] text-text-secondary hover:text-text-primary transition-colors"
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
  const hue = FORMATION_RAMP[idx % FORMATION_RAMP.length];
  const letter = String.fromCharCode(65 + (idx % 26));

  const live = lane.status === 'running' && !stale;
  const interrupted = lane.status === 'running' && stale;
  const Icon = interrupted ? CircleSlash : lane.status === 'done' ? Check : lane.status === 'error' ? X : Loader2;
  const iconColor = interrupted ? CALL_PENDING : STATUS_COLOR[lane.status];

  const calls = lane.calls ?? [];
  // THE DURABLE TRANSCRIPT FIRST — see `laneNarrative`, which is the one copy of this chain. It was
  // written out here and again on the board row, and the board's copy had already lost its `lastText`
  // fallback, which is how a rule with N copies reads on the day someone edits N-1 of them.
  const rawReasoning = laneNarrative(lane);
  const reasoning = rawReasoning.length >= 8 && /[a-zA-Z]{3,}/.test(rawReasoning) ? rawReasoning : '';
  const failLike = lane.status === 'error' || interrupted;
  const laneError = failLike && lane.error ? lane.error.trim() : '';
  const hasBody =
    reasoning.length > 0 || calls.length > 0 || (lane.recent?.length ?? 0) > 0 || laneError.length > 0;
  // The first failing call auto-expands so the error is zero clicks away.
  const firstFailIdx = calls.findIndex((c) => c.ok === false);
  // Compact mode's single high-level line: the freshest activity, else the last line of reasoning.
  const compactLine =
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
        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-background-primary transition-colors"
      >
        {hasBody ? (
          open ? (
            <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
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
          <span className="font-bold shrink-0" style={{ color: hue }} aria-label={`node ${letter}`}>
            ⬢{letter}
          </span>
        </Tip>
        <Tip label={<span className="font-mono">{lane.device}</span>}>
          <span className="w-16 shrink-0 truncate text-text-secondary text-xs">{lane.device}</span>
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
                  <span className="font-mono text-text-secondary">{lane.taskId}</span>
                </>
              ) : null}
            </span>
          }
        >
          {/* THE ID IS NOT A SECOND LINE. "Slice · approval-workflow-outbox" over
              "slice-approval-workflow-outbox" is the same string twice, on every row of every group —
              Mihai: "a lot of this UI is self duplicating information". It stays in the hover tip, where
              it is available without costing a line. */}
          <span className="flex-1 min-w-0 truncate text-xs text-text-primary">
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
            <span className="hidden sm:inline shrink-0 max-w-[9rem] truncate text-[10px] font-mono text-text-secondary">
              {lane.model}
            </span>
          </Tip>
        )}
        <span className="text-xs text-text-secondary tabular-nums shrink-0 flex items-center gap-1.5">
          {/* THE JUDGE'S OWN ESTIMATE, shown only while the call is still running — the one number on this
              screen produced by something that READ the work rather than extrapolated from item counts.
              Solid amber, because it is a live claim that will be revised, not a settled fact. */}
          {lane.status === 'running' && typeof lane.judgeEtaMins === 'number' ? (
            <Tip
              label={`The supervisor read this call and estimates ~${lane.judgeEtaMins} more minute${
                lane.judgeEtaMins === 1 ? '' : 's'
              } of work`}
            >
              <span
                className="text-[10px] font-bold px-1.5 py-0.5 text-background-primary"
                style={{ backgroundColor: '#d97706', borderRadius: 3 }}
              >
                ~{lane.judgeEtaMins}m
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
              <span style={{ color: STATUS_COLOR.error }}>{lane.errors}✕</span>
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
              className={`${live ? 'animate-spin' : ''}`}
              style={{ color: iconColor }}
            />
          </span>
        </Tip>
      </button>

      {open && hasBody && (
        <div className="px-3 pb-3 pl-9 space-y-2">
          {laneError ? (
            <div>
              <div className="text-[10px] uppercase tracking-wide mb-1" style={{ color: STATUS_COLOR.error }}>
                {interrupted ? 'Last error before it stalled' : 'Why it failed'}
              </div>
              <MonoOutput text={laneError} failed />
            </div>
          ) : null}
          {mode === 'compact' ? (
            // Compact: a single high-level line of what this node is doing now — no reasoning dump, no calls.
            compactLine ? (
              <div className="text-xs text-text-secondary truncate">{compactLine}</div>
            ) : null
          ) : (
            <>
              {reasoning && (
                <ReasoningBlock
                  text={reasoning}
                  live={live}
                  forceOpen={dev || live}
                  // Developer: name the model so it's unmistakable WHOSE generation this is.
                  label={dev ? `${live ? 'Generating' : 'Reasoning'} · ${lane.model ?? lane.device}` : undefined}
                />
              )}
              {calls.length > 0 ? (
                <div>
                  <div className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary mb-1.5">
                    Tool calls · {lane.toolCalls ?? calls.length}
                  </div>
                  <div
                    className="bg-background-primary border border-border-primary px-2 py-1"
                    style={{ borderRadius: CHIP_RADIUS }}
                  >
                    {calls.map((c, i) => (
                      // Developer mode opens every call's output; otherwise only the first failure.
                      <CallRow key={i} call={c} defaultOpen={dev || i === firstFailIdx} />
                    ))}
                  </div>
                </div>
              ) : lane.recent && lane.recent.length > 0 ? (
                <div className="text-xs text-text-secondary font-mono break-words">
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
// One hue per KIND of engine event, from the LeanZero ramp + status triad. Every value is a theme token so
// the log is legible on both canvases — the old fixed dark-mode hexes washed out on the light one.
const ACTIVITY_COLOR: Record<ActivityItem['kind'], string> = {
  // The user's own words landing in the build — solid amber so it stands apart from engine chatter.
  note: SWARM_STATUS.running,
  phase: SWARM_STATUS.action,
  plan: FORMATION_RAMP[2],
  dispatch: FORMATION_RAMP[1],
  done: SWARM_STATUS.done,
  fail: SWARM_STATUS.error,
  retry: SWARM_STATUS.running,
  retarget: FORMATION_RAMP[2],
  review: FORMATION_RAMP[4],
  // An observation recedes; an ACTION is a solid saturated accent, because the whole question the log
  // has to answer at a glance is which rows changed the run and which only watched it.
  judge: FORMATION_RAMP[4],
  'judge-act': SWARM_STATUS.action,
  prereview: FORMATION_RAMP[1],
  smoke: SWARM_STATUS.done,
  brief: 'var(--color-text-secondary)',
  config: 'var(--color-text-secondary)',
};
const TONE_COLOR: Record<NonNullable<ActivityItem['tone']>, string> = {
  info: 'var(--color-text-secondary)',
  good: SWARM_STATUS.done,
  warn: SWARM_STATUS.running,
  bad: SWARM_STATUS.error,
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
  const itemCls =
    'w-full text-left px-3 py-1.5 text-xs text-text-primary hover:bg-background-secondary flex items-center gap-2';
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
        className="fixed z-50 min-w-[168px] bg-background-primary border border-border-primary shadow-lg py-1"
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

// One line in the activity timeline. Tone (when set) tints the icon so judge warnings / failures stand out.
// Right-click reveals a menu — "Reveal in Finder" when the line references a file path, plus Copy.
// `dim` renders the EVENT LOG register: denser and on the secondary color — the log is the subordinate
// narrative record, never competing with the zones above it (dim = solid secondary text, NOT opacity washes).
const ActivityLine: React.FC<{ it: ActivityItem; wrap?: boolean; workingDir?: string; dim?: boolean }> = ({ it, wrap, workingDir, dim }) => {
  const Icon = ACTIVITY_ICON[it.kind];
  const color = it.tone ? TONE_COLOR[it.tone] : ACTIVITY_COLOR[it.kind];
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
    <div className="flex flex-col">
      <div
        className={`flex items-start gap-2 ${dim ? 'text-[11px] leading-snug' : 'text-xs'} ${expandable ? 'cursor-pointer' : ''}`}
        onClick={expandable ? () => setOpen((o) => !o) : undefined}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY });
        }}
      >
        <Icon size={dim ? 12 : 13} strokeWidth={2.5} className="mt-0.5 shrink-0" style={{ color }} />
        <span
          className={`shrink-0 ${dim ? 'text-text-secondary' : 'text-text-primary'}`}
          style={it.kind === 'judge-act' ? { color, fontWeight: 600 } : undefined}
        >
          {it.text}
        </span>
        {it.sub && !open && (
          <span
            className={`text-text-secondary ${wrap ? 'break-words' : 'truncate'} ${
              it.kind === 'brief' ? 'line-clamp-3' : wrap ? 'line-clamp-2' : ''
            }`}
          >
            — {it.sub}
          </span>
        )}
        {expandable &&
          (open ? (
            <ChevronDown size={12} className="ml-auto mt-0.5 shrink-0 text-text-secondary" />
          ) : (
            <ChevronRight size={12} className="ml-auto mt-0.5 shrink-0 text-text-secondary" />
          ))}
      </div>
      {it.sub && open && (
        <div
          className="ml-[21px] mt-1 mb-0.5 px-2 py-1.5 text-xs text-text-secondary whitespace-pre-wrap break-words bg-background-secondary border border-border-primary"
          style={{ borderRadius: CHIP_RADIUS }}
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
    </div>
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

function useSmoothText(target: string, charsPerSec = 110): string {
  const [shown, setShown] = useState('');
  const reduceMotion = usePrefersReducedMotion();
  const targetRef = useRef(target);
  targetRef.current = target;
  const shownRef = useRef('');

  useEffect(() => {
    if (!reduceMotion) return;
    shownRef.current = target;
    setShown(target);
  }, [reduceMotion, target]);

  useEffect(() => {
    if (reduceMotion) return;
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
  }, [charsPerSec, reduceMotion]);
  return reduceMotion ? target : shown;
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
const NodeExpandBox: React.FC<{ text: string; fill?: boolean }> = ({ text, fill }) => {
  const ref = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState(true);
  useEffect(() => {
    const el = ref.current;
    if (!el || !follow) return;
    el.scrollTop = el.scrollHeight;
  }, [text, follow]);
  const onScroll = () => {
    const el = ref.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (atBottom !== follow) setFollow(atBottom);
  };
  return (
    <div className={fill ? 'relative flex flex-col min-h-0 h-full' : 'contents'}>
      <div
        ref={ref}
        onScroll={onScroll}
        className={
          fill
            ? 'flex-1 min-h-0 overflow-y-auto px-3 py-2 font-mono text-[11px] leading-relaxed text-text-secondary whitespace-pre-wrap break-words'
            : 'ml-6 mt-1 mb-1 p-2 font-mono text-[11px] text-text-secondary whitespace-pre-wrap break-words border border-border-primary bg-background-secondary'
        }
        style={fill ? undefined : { borderRadius: CHIP_RADIUS, maxHeight: 300, overflowY: 'auto' }}
      >
        {text}
      </div>
      {fill && !follow ? (
        <button
          onClick={() => {
            const el = ref.current;
            if (el) el.scrollTop = el.scrollHeight;
            setFollow(true);
          }}
          className="absolute bottom-2 right-3 px-2 py-1 text-[10px] font-mono uppercase tracking-wider font-bold text-white shadow-lg"
          style={{ borderRadius: CHIP_RADIUS, background: SWARM_STATUS.running }}
        >
          ↓ follow
        </button>
      ) : null}
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
/// Tool-call summaries (`recent`) are a DIFFERENT channel and are still prepended: they are what the model
/// DID, and the transcript is what it SAID.
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
};

export function inspectorOutputText(lane: StreamLane): string {
  const durable = lane.fullTranscript?.trim() ?? '';
  const said = durable || (lane.lastText?.trim() ?? '');
  return [...(lane.recent ?? []), said].filter(Boolean).join('\n\n');
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
 */
export function laneLiveLine(lane: StreamLane): string {
  return (
    substantiveChunk(tailOf(lane.fullTranscript?.trim() ?? '', INLINE_TAIL_CHARS)) ||
    substantiveChunk(lane.reasoning) ||
    fleetThinkingLine(lane) ||
    substantiveChunk(lane.lastText) ||
    (lane.recent && lane.recent.length > 0
      ? substantiveChunk(lane.recent[lane.recent.length - 1])
      : '')
  );
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
  const said = lane?.fullTranscript?.trim() || lane?.fullReasoning?.trim() || '';
  return [said, fleetThinkingLine(lane)].filter(Boolean).join('\n\n');
}

/** The narration a ROW renders in its expanded body: the durable answer channel, then the clipped digests. */
export function laneNarrative(lane: StreamLane): string {
  return (
    lane.fullTranscript?.trim() ||
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
  const durable = lane.fullThinking?.trim() || lane.fullTranscript?.trim() || '';
  const text = durable || inspectorThinkingText(lane) || laneNarrative(lane);
  return tailOf(text, CARD_TAIL_CHARS);
}

const NodeInspector: React.FC<{
  device: string;
  letter: string;
  hue: string;
  ink: string;
  lane?: TurnLane;
  nodeState?: string;
  onClose: () => void;
}> = ({ device, letter, hue, ink, lane, nodeState, onClose }) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

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
  const thinkText = collapseRepeats(inspectorThinkingText(lane ?? {}));
  const calls = lane?.calls ?? [];
  const outText = inspectorOutputText(lane ?? {});

  const Pane: React.FC<{ title: string; count: string; body: string; empty: string }> = ({
    title,
    count,
    body,
    empty,
  }) => (
    <div
      className="flex flex-col min-h-0 flex-1 border border-border-primary overflow-hidden"
      style={{ borderRadius: CHIP_RADIUS }}
    >
      <div className="flex items-center justify-between px-3 py-1.5 border-b border-border-primary shrink-0 bg-background-secondary">
        <span className="font-mono uppercase tracking-[0.18em] text-[10px] font-bold text-text-primary">
          {title}
        </span>
        <span className="text-[10px] tabular-nums text-text-secondary">{count}</span>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">
        {body ? (
          <NodeExpandBox text={body} fill />
        ) : (
          <div className="px-3 py-2 text-xs text-text-secondary">{empty}</div>
        )}
      </div>
    </div>
  );

  return createPortal(
    <>
      <div className="fixed inset-0 z-40 bg-black/60" onClick={onClose} />
      <div
        role="dialog"
        aria-modal="true"
        aria-label={`Node ${letter}, ${device}`}
        className="fixed z-50 inset-4 md:inset-8 flex flex-col bg-background-primary border border-border-primary shadow-2xl"
        style={{ borderRadius: CHIP_RADIUS }}
      >
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border-primary shrink-0">
          <span
            className="inline-flex items-center justify-center font-mono font-semibold shrink-0"
            style={{ width: 20, height: 20, borderRadius: CHIP_RADIUS, background: hue, color: ink, fontSize: 11 }}
          >
            {letter}
          </span>
          <span className="font-mono text-sm text-text-primary">{device}</span>
          {nodeState && (
            <span
              className="px-2 py-0.5 text-[10px] font-mono uppercase tracking-wider font-bold text-white"
              style={{
                borderRadius: CHIP_RADIUS,
                background:
                  nodeState === 'generating'
                    ? SWARM_STATUS.done
                    : nodeState === 'processingPrompt'
                      ? SWARM_STATUS.running
                      : SWARM_STATUS.stopped,
              }}
            >
              {nodeState === 'processingPrompt' ? 'processing prompt' : nodeState}
            </span>
          )}
          {/* THE SUPERVISOR IS READING THIS LANE. While an omni-judge probe is in flight the engine
              buffers the worker's stream instead of processing it, so the counters below are genuinely
              frozen and the lane is NOT dead. Saying so is the difference between a panel that looks
              broken and one that is telling the truth. */}
          {lane?.judging && (
            <span
              className="px-2 py-0.5 text-[10px] font-mono uppercase tracking-wider font-bold text-white"
              style={{ borderRadius: CHIP_RADIUS, background: SWARM_STATUS.running }}
            >
              {'supervisor reading'}
            </span>
          )}
          {lane?.description && (
            <span className="text-xs text-text-secondary truncate">{lane.description}</span>
          )}
          <button
            className="ml-auto shrink-0 text-text-secondary hover:text-text-primary"
            onClick={onClose}
            aria-label="Close"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 min-h-0 grid grid-cols-1 lg:grid-cols-2 gap-3 p-3">
          <Pane
            title="Thinking"
            // COUNT WHAT IS ON SCREEN, and say so when it is a clipped tail. The header used to report
            // the engine's `thinkingChars` while the body rendered something else entirely, so a pane
            // showing 2,000 characters could be captioned 22,150 — which reads as "the UI is hiding
            // things" and, before the lane-path fix, actually was.
            //
            // `thinkingChars` IS NOT THE DENOMINATOR. It is the engine's per-stream counter and resets on
            // a re-stream, so it says nothing about the size of `<task>.think.log`; the only number that
            // does is `thinkingBytes`, which main.ts has attached to every digest and nothing has read.
            count={`${thinkText.length.toLocaleString()} chars${streamTailNote(lane?.fullThinking, lane?.thinkingBytes)}`}
            body={thinkText}
            empty="Nothing on the reasoning channel yet."
          />
          <Pane
            title="Output"
            // SAY WHEN THE TRANSCRIPT IS A TAIL — main.ts already computed the answer, in the one place
            // that knows both the file size and the budget it read with. See streamTailNote.
            count={`${calls.length} tool call${calls.length === 1 ? '' : 's'}${streamTailNote(lane?.fullTranscript, lane?.transcriptBytes, lane?.transcriptClipped)}`}
            body={outText}
            empty="Nothing emitted yet — reasoning, but no tool call and no text."
          />
        </div>
      </div>
    </>,
    document.body
  );
};

const FleetStrip: React.FC<{
  /** Every node the run's RESOLVED POOL carries (idle ones included) + any lane device — see deriveFleet. */
  deviceOrder: string[];
  /** node -> its live lane (task lifecycle, open activity digest, or supervision span), from deriveFleet. */
  runningByDevice: Map<string, TurnLane>;
  live: boolean;
  dev: boolean;
  /** LM Studio's own live status per node short-name (generating/processingPrompt/idle), for the truth dot. */
  nodeStatus: Record<string, string>;
  /** Open supervision spans deriveFleet could not pin to a busy node — still shown, never dropped. */
  unattributed: SupervisionSpan[];
}> = ({ deviceOrder, runningByDevice, live, dev, nodeStatus, unattributed }) => {
  // The full stream opens in a MODAL. Inline it was clipped by whatever height the row happened to have,
  // which made the panel least readable exactly when a node was busiest.
  const [inspect, setInspect] = useState<string | null>(null);
  if (deviceOrder.length === 0) return null;
  const shortName = (device: string): string => device.match(/^([^-]+)/)?.[1] ?? device;
  return (
    <div className="px-3 py-2 bg-background-primary space-y-1.5">
      {deviceOrder.map((device, i) => {
        const hue = FORMATION_RAMP[i % FORMATION_RAMP.length];
        // Each ramp hue carries its own glyph colour: no single ink clears AA across six saturated fills.
        const ink = FORMATION_INK[i % FORMATION_INK.length];
        const letter = String.fromCharCode(65 + (i % 26));
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
        const canExpand = !!lane && fullGen.length > 0;
        return (
          // THE INSTRUMENT'S ANCHOR. The per-tick frontend check reads the RENDERED lane text and the
          // RENDERED clickability off these attributes; without them it fell back to re-deriving both
          // from the IPC payload and reported a healthy render path while the renderer was dropping
          // every field. `data-task` is what joins a cell to its digest on the SAME object, so the
          // instrument compares a lane against its own data and never against a neighbour's.
          <div
            key={device}
            data-testid="fleet-node"
            data-device={device}
            data-task={lane?.taskId ?? ''}
            data-expandable={canExpand ? 'true' : 'false'}
            data-gen-len={fullGen.length}
            className="border border-border-primary px-2 py-1.5"
            style={{ borderRadius: CHIP_RADIUS }}
          >
            <div
              className="flex items-start gap-2 text-xs"
              style={{ cursor: canExpand ? 'pointer' : 'default' }}
              role={canExpand ? 'button' : undefined}
              tabIndex={canExpand ? 0 : undefined}
              aria-label={canExpand ? `Open the full stream from ${shortName(device)}` : undefined}
              onClick={canExpand ? () => setInspect(device) : undefined}
              onKeyDown={
                canExpand
                  ? (e: React.KeyboardEvent) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        setInspect(device);
                      }
                    }
                  : undefined
              }
            >
              <span
                className="inline-flex items-center justify-center font-mono font-semibold shrink-0 mt-[1px]"
                style={{ width: 16, height: 16, borderRadius: CHIP_RADIUS, background: hue, color: ink, fontSize: 10 }}
              >
                {letter}
              </span>
              <span className="font-mono text-text-primary shrink-0" style={{ minWidth: 96 }}>
                {shortName(device)}
              </span>
              {/* LM Studio's OWN live state for this node (lms ps --json), independent of goose's digest — the
                  ground-truth "is it generating right now" Mihai asked for. Solid dot: green generating, amber
                  prompt-processing, dim grey idle; nothing when LM Studio is unreachable. */}
              {(() => {
                const st = nodeStatus[shortName(device)];
                if (!st) return null;
                const color =
                  st === 'generating' ? SWARM_STATUS.done : st === 'processingPrompt' ? SWARM_STATUS.running : SWARM_STATUS.stopped;
                const label =
                  st === 'generating'
                    ? 'generating'
                    : st === 'processingPrompt'
                      ? 'processing prompt'
                      : 'idle';
                return (
                  <Tip label={`LM Studio: ${label}`}>
                    <span
                      className="shrink-0 mt-[5px]"
                      style={{ width: 7, height: 7, borderRadius: '50%', background: color }}
                    />
                  </Tip>
                );
              })()}
              {lane ? (
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    {/* Only a LIVE run spins. A dead/historical session's lanes are frozen at 'running' (a run
                        that died mid-task never emits task_completed), so on a non-live run show a static
                        interrupted marker instead of a fake spinner — this is why opening an old session used to
                        look like it was still streaming. */}
                    {(() => {
                      // SUPERVISION work (a judge generation with no task lane) is visually its own class:
                      // a solid violet gavel, never the amber build spinner — a supervising node says what
                      // it is really doing instead of reading "idle — no task".
                      const supervising = lane.phase === 'supervision';
                      if (!live)
                        return <CircleSlash size={12} className="shrink-0" style={{ color: STOPPED }} />;
                      if (supervising)
                        return <Gavel size={12} className="shrink-0" style={{ color: FORMATION_RAMP[2] }} />;
                      return (
                        <Loader2
                          size={12}
                          className="animate-spin shrink-0"
                          style={{ color: STATUS_COLOR.running }}
                        />
                      );
                    })()}
                    <span
                      className={`truncate ${live && lane.phase !== 'supervision' ? 'text-text-primary' : ''}`}
                      style={
                        !live
                          ? { color: STOPPED }
                          : lane.phase === 'supervision'
                            ? { color: FORMATION_RAMP[2], fontWeight: 600 }
                            : undefined
                      }
                    >
                      {lane.description || lane.taskId}
                      {lane.phase === 'supervision' && typeof lane.elapsedMs === 'number'
                        ? ` · ${Math.round(lane.elapsedMs / 1000)}s`
                        : ''}
                    </span>
                    {canExpand ? (
                      <ChevronRight
                        size={12}
                        className="shrink-0 text-text-secondary transition-transform"
                        style={{ transform: 'none' }}
                      />
                    ) : null}
                  </div>
                  {dev && lane.calls && lane.calls.length > 0
                    ? (() => {
                        // What this node is DOING right now — its latest tool/MCP call (running… when in-flight).
                        const last = lane.calls![lane.calls!.length - 1];
                        const cm = classifyCall(last);
                        return (
                          <div className="flex items-center gap-1.5 mt-0.5">
                            <CallTypeIcon icon={cm.icon} color={CALL_KIND_COLOR[cm.kind]} />
                            <span className="text-text-secondary truncate">
                              {cm.action}
                              {last.summary ? (
                                <span className="font-mono text-text-secondary"> · {last.summary}</span>
                              ) : null}
                            </span>
                            {lane.toolCalls ? (
                              <span className="ml-auto font-mono text-text-secondary shrink-0">
                                ⚙{lane.toolCalls}
                              </span>
                            ) : null}
                          </div>
                        );
                      })()
                    : null}
                  {liveGen ? (
                    live ? (
                      // LIVE: typewriter-smoothed so the stream flows instead of jumping every poll. dev = 5
                      // lines to fill the space, compact/verbose = 2. Click the node to expand the full stream.
                      <NodeLiveText text={liveGen} lines={dev ? 5 : 2} />
                    ) : (
                      // HISTORICAL/DEAD run: the last frozen snapshot, static + dimmed — NEVER animated, so an
                      // old session no longer looks like it is still streaming.
                      <div
                        data-testid="fleet-node-gen"
                        className="text-text-secondary whitespace-pre-wrap break-words mt-0.5"
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
              ) : (() => {
                  // No lane, no span — but LM Studio's own status may still say the node is generating.
                  // The engine's pre-review / test-gen / sink-review calls report ONLY on completion
                  // (verified in swarm.rs), so a busy node here is running exactly that class of call.
                  // Saying "idle — no task" while LM Studio shows requests in flight was the lie Mihai
                  // caught live; name the work class honestly instead.
                  const st = nodeStatus[shortName(device)];
                  const busy = live && (st === 'generating' || st === 'processingPrompt');
                  return busy ? (
                    <Tip label="LM Studio reports this node generating. The engine's supervision calls (pre-review, test-gen, sink review) log only when they finish — the result lands in the event log.">
                      <span style={{ color: FORMATION_RAMP[2], fontWeight: 600 }} className="flex items-center gap-1.5">
                        <Eye size={12} className="shrink-0" />
                        supervising — review/test-gen call in flight
                      </span>
                    </Tip>
                  ) : (
                    <span style={{ color: STOPPED }}>{live ? 'idle — no task' : 'idle'}</span>
                  );
                })()}
            </div>
          </div>
        );
      })}
      {live &&
        unattributed.map((s) => (
          // A judge span with no busy node to pin it to — real work, shown unattributed rather than dropped.
          <div key={`sup-${s.taskId}`} className="flex items-center gap-2 text-xs">
            <span className="shrink-0" style={{ width: 16 }} />
            <Gavel size={12} className="shrink-0" style={{ color: FORMATION_RAMP[2] }} />
            <span style={{ color: FORMATION_RAMP[2], fontWeight: 600 }}>{s.label}</span>
            <span className="text-text-secondary">— on an idle node (the verdict names it when it lands)</span>
          </div>
        ))}
      {inspect
        ? (() => {
            const i = Math.max(deviceOrder.indexOf(inspect), 0);
            return (
              <NodeInspector
                device={inspect}
                letter={String.fromCharCode(65 + (i % 26))}
                hue={FORMATION_RAMP[i % FORMATION_RAMP.length]}
                ink={FORMATION_INK[i % FORMATION_INK.length]}
                lane={runningByDevice.get(inspect)}
                nodeState={nodeStatus[shortName(inspect)]}
                onClose={() => setInspect(null)}
              />
            );
          })()
        : null}
    </div>
  );
};

// The chronological engine narrative — the body of the EVENT LOG zone. Latest at the bottom; a spinner
// tail while the run is live. In verbose mode it shows the FULL stream and wraps. Deliberately dense and
// on the secondary color: the zones above are the primary read; this is the record.
const ActivityFeed: React.FC<{ items: ActivityItem[]; live: boolean; verbose: boolean; workingDir?: string }> = ({ items, live, verbose, workingDir }) => {
  const shown = verbose ? items : items.slice(-8);
  if (items.length === 0) return null;
  return (
    <div className="px-3 py-2 space-y-0.5 bg-background-primary">
      {shown.map((it) => (
        <ActivityLine key={it.seq} it={it} wrap={verbose} workingDir={workingDir} dim />
      ))}
      {live && (
        <div className="flex items-center gap-2 text-[11px] text-text-secondary">
          <Loader2 size={12} className="animate-spin shrink-0" />
          <span>working…</span>
        </div>
      )}
    </div>
  );
};

/**
 * EVENT LOG zone — the feed, explicitly named for what it is and visually subordinate. Collapsed by
 * default outside developer/verbose mode: judge verdicts and failures already surface on WORK-board rows,
 * so the log is the narrative/debugging record, not the primary read. Collapsed, it still shows the count
 * and the latest line so nothing feels hidden.
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
  return (
    <div className="border-t border-border-primary">
      <ZoneHeader
        hue={ZONE_HUES.log}
        label="Event log"
        explain="everything the engine reported, in order"
        collapsed={!open}
        onToggle={() => setOverride((o) => !(o ?? verbose))}
        right={
          <>
            {!open && last ? (
              <span className="text-[10px] text-text-secondary truncate max-w-[18rem]">{last.text}</span>
            ) : null}
            <span className="text-[10px] tabular-nums text-text-secondary shrink-0">
              {items.length} events
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

// Threshold color for a confidence value: solid green >=70 (confident), amber 40-69 (unsure), red <40.
// Use this for the SUB-SIGNALS (agreement / spec-clarity), where the point is which one is lower — not
// whether the run may proceed. For the headline number use confColorVsFloor.
const confColor = (v: number): string =>
  v >= 70 ? STATUS_COLOR.done : v >= 40 ? AMBER : STATUS_COLOR.error;

/** Colour for the HEADLINE confidence, against the engine's own bar.
 *
 *  The band above is a UI invention. When a floor is set, the engine has already made the go/no-go call:
 *  below the floor it ASKS instead of building. A 73 under a floor of 80 painted green said "good" in the
 *  one channel a user reads before any words — while the run had stopped and asked. confVerdict was fixed
 *  for exactly this and the colour was left behind, so the pill went on being green next to text saying
 *  "Below your bar of 80". No floor = that run never asks = there is no bar = the band is all we can say. */
export const confColorVsFloor = (v: number, floor: number | null): string => {
  if (floor == null) return confColor(v);
  if (v >= floor) return STATUS_COLOR.done;
  // Under the bar. Amber = goose asked and is waiting; red = it is not close.
  return v >= floor - 20 ? AMBER : STATUS_COLOR.error;
};

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
  const col = confColorVsFloor(value, askFloor);
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline gap-2">
        <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-text-primary">
          {label}
        </span>
        {binding ? (
          <span
            className="text-[9px] font-bold uppercase tracking-[0.08em] px-1.5 py-px text-background-primary"
            style={{ backgroundColor: col, borderRadius: CHIP_RADIUS }}
          >
            binding
          </span>
        ) : null}
        <span
          className="ml-auto text-[17px] font-extrabold leading-none tabular-nums"
          style={{ color: col }}
        >
          {value}
        </span>
      </div>
      <div
        className="h-1.5 bg-background-primary border border-border-primary overflow-hidden"
        style={{ borderRadius: CHIP_RADIUS }}
      >
        <div
          className="h-full"
          style={{
            width: `${Math.max(0, Math.min(100, value))}%`,
            backgroundColor: col,
            transition: 'width 500ms ease-out',
          }}
        />
      </div>
      {reason ? (
        <div className="text-[11px] leading-snug text-text-secondary">{reason}</div>
      ) : null}
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
  const col = confColorVsFloor(v, askFloor);
  return (
    <svg width={size} height={size} viewBox="0 0 80 80" className="shrink-0" role="img" aria-label={`plan confidence ${v} of 100${askFloor != null ? `, your bar is ${askFloor}` : ''}`}>
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        stroke="var(--color-border-primary)"
        strokeWidth="8"
        strokeDasharray={`${track} ${circ}`}
        transform="rotate(135 40 40)"
      />
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        stroke={col}
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
          fill="var(--color-text-primary)"
          transform={`rotate(${135 + (Math.max(0, Math.min(100, askFloor)) / 100) * 270} 40 40)`}
        />
      ) : null}
      <text
        x="40"
        y="39"
        textAnchor="middle"
        dominantBaseline="middle"
        style={{
          fill: col,
          fontSize: 24,
          fontWeight: 800,
          letterSpacing: -0.6,
          fontVariantNumeric: 'tabular-nums',
        }}
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
        style={{ fill: col, fontSize: 9, fontWeight: 700, letterSpacing: 0.5 }}
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
          <div className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary">
            Plan confidence
          </div>
          <div
            className="text-[15px] font-bold leading-snug mt-1"
            style={{ color: confColorVsFloor(conf.final, askFloor) }}
          >
            {confVerdict(conf.final, askFloor)}
          </div>
          {askFloor != null ? (
            <div className="text-[11px] text-text-secondary mt-1">
              Your bar is <span className="font-bold text-text-primary">{askFloor}</span> — below it, goose
              asks you instead of guessing.
            </div>
          ) : null}
        </div>
      </div>
      {/* Full border, never a left rail. The two signals are one group because the LOWER of them IS the
          headline score — showing them apart hides that relationship. */}
      <div
        className="border border-border-primary px-3 py-3 space-y-4"
        style={{ borderRadius: CHIP_RADIUS }}
      >
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
        <div className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary mb-1.5">
          What&apos;s holding it back
        </div>
        {showDecisions ? (
          <ul className="space-y-0.5">
            {conf.openDecisions.map((d, i) => (
              <li key={i} className="text-[12px] leading-relaxed text-text-primary flex gap-1.5">
                <span className="text-text-secondary shrink-0">·</span>
                <span>{d}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="text-[12px] leading-relaxed text-text-primary">{holdingBack}</div>
        )}
      </div>
      <div>
        <div className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary mb-1.5">
          What would raise it
        </div>
        <div className="text-[12px] leading-relaxed text-text-primary">{raiseIt}</div>
      </div>
      {trail && trail.length >= 2 ? (
        <div className="flex items-center gap-2">
          <div className="flex items-end gap-0.5 h-6">
            {trail.map((v, i) => (
              <div
                key={i}
                style={{
                  width: 4,
                  height: `${Math.max(6, v * 0.24)}px`,
                  backgroundColor: confColor(v),
                  borderRadius: 1,
                }}
              />
            ))}
          </div>
          <span className="text-[10px] tabular-nums text-text-secondary">
            {trail.map((v, i) => (
              <React.Fragment key={i}>
                {i > 0 ? ' → ' : ''}
                <span style={i === trail.length - 1 ? { color: confColor(v) } : undefined}>{v}</span>
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
      className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium tabular-nums"
      style={{ backgroundColor: confColorVsFloor(value, askFloor), borderRadius: CHIP_RADIUS }}
    >
      <Gauge className="h-2.5 w-2.5" />
      conf {value}
    </span>
  </Tip>
);

function fmtElapsed(min: number): string {
  const totalSec = Math.max(0, Math.round(min * 60));
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
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
    etaLabel = `~${lo}–${hi} min left`;
  } else if (itemsTotal > 0 && itemsDone >= itemsTotal) {
    etaLabel = 'wrapping up';
  } else {
    // Never blank — before the first checklist item completes there is genuinely no basis to estimate from yet.
    etaLabel = 'estimating…';
  }
  return (
    <span className="flex items-center gap-3 shrink-0 tabular-nums">
      <Tip label="Total wall-clock time since the run started.">
        <span className="text-xs font-semibold" style={{ color: SWARM_STATUS.action }}>
          {fmtElapsed(elapsedMin)}
        </span>
      </Tip>
      <Tip label="A deliberately rough range — the local fleet is variable, so a precise figure would lie.">
        <span className="text-xs font-semibold" style={{ color: AMBER }}>
          {etaLabel}
        </span>
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
  // Slate-blue — a real, solid colour that is deliberately NOT green: the work finished, the app was never
  // run. It has to read as "shipped, unproven", which neither grey (pending) nor green (verified) says.
  unverified: 'var(--color-node-2, #0891b2)',
  failed: STATUS_COLOR.error,
  judge_failed: AMBER,
  blocked: 'var(--color-text-secondary)',
  skipped: SWARM_STATUS.stopped,
  advisory: SWARM_STATUS.action,
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

const TodoPill: React.FC<{ text: string; color: string }> = ({ text, color }) => (
  <span
    className="text-[9px] uppercase tracking-wide px-1 py-px shrink-0"
    style={{ color, border: `1px solid ${color}`, borderRadius: CHIP_RADIUS }}
  >
    {text}
  </span>
);

// The judge's REASONING for a task — the diagnosis (verdict) + the exact corrective note it gave the worker.
// This is what Mihai wanted surfaced: not just "judge decision" but WHY.
const JudgeReason: React.FC<{ judge: NonNullable<PhaseTodoItem['judge']> }> = ({ judge }) => (
  <div className="text-[10px]">
    <div className="flex items-center gap-1.5 flex-wrap">
      <Gavel className="h-3 w-3 shrink-0" style={{ color: AMBER }} />
      <span className="font-semibold text-text-primary">Judge</span>
      {judge.verdict ? (
        <span style={{ color: AMBER }}>{judge.verdict.replace(/_/g, ' ')}</span>
      ) : null}
      <span className="text-text-secondary">→ {judge.action.replace(/_/g, ' ')}</span>
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
    <div className="text-[10px] text-text-secondary space-y-1">
      <span className="uppercase tracking-wide text-text-tertiary">Live generation</span>
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
          <span style={{ color: SWARM_STATUS.running }} className="font-medium">
            {errors} app-error{malformed > 0 ? ` · ${malformed} malformed` : ''}
          </span>
        ) : null}
      </div>
      {reasoning.trim() ? <ReasoningBlock text={reasoning} label="Model reasoning (live)" /> : null}
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
        className={`w-full flex items-center gap-1.5 text-[11px] py-0.5 min-w-0 text-left ${hasDetail ? 'cursor-pointer hover:opacity-80' : ''}`}
      >
        {interrupted ? (
          <CircleSlash className="h-3.5 w-3.5 shrink-0" style={{ color: c }} />
        ) : (
          <TodoGlyph state={item.state} />
        )}
        {idx >= 0 ? (
          <span
            className="text-[9px] font-mono shrink-0"
            style={{ color: FORMATION_RAMP[idx % FORMATION_RAMP.length] }}
          >
            ⬢{String.fromCharCode(65 + (idx % 26))}
          </span>
        ) : null}
        <span
          className="shrink-0 font-medium"
          style={{ color: item.state === 'pending' ? 'var(--color-text-secondary)' : 'var(--color-text-primary)' }}
        >
          {item.label}
        </span>
        {item.summary ? (
          <span className="text-text-secondary truncate">· {item.summary}</span>
        ) : null}
        {interrupted ? <TodoPill text="interrupted" color={c} /> : null}
        {!interrupted && item.state === 'unverified' ? <TodoPill text="unverified" color={c} /> : null}
        {!interrupted && item.state === 'judge_failed' ? <TodoPill text="judge" color={c} /> : null}
        {!interrupted && item.state === 'blocked' ? <TodoPill text="blocked" color={c} /> : null}
        {item.detail ? <span className="text-[10px] text-text-secondary truncate">· {item.detail}</span> : null}
        {hasDetail ? (
          <span className="ml-auto shrink-0 text-text-secondary">
            {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          </span>
        ) : null}
      </div>
      {open && hasDetail ? (
        <div className="ml-5 mt-1 mb-1.5 space-y-1.5 border-l-0">
          {planTask && (planTask.difficulty || planTask.deps.length) ? (
            <div className="text-[10px] text-text-secondary flex flex-wrap gap-x-3">
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
            <div className="text-[10px] text-text-secondary break-words">
              <span className="uppercase tracking-wide text-text-tertiary">Files</span>{' '}
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
          {item.description ? <ReasoningBlock text={item.description} label="Full task spec" /> : null}
          {digest ? <TaskGenDetail digest={digest} /> : null}
          {item.judge ? <JudgeReason judge={item.judge} /> : null}
        </div>
      ) : null}
    </div>
  );
};

// A WORK-board group header: solid state dot + the mono uppercase group name + count. The three groups
// (RUNNING / QUEUED / DONE) are the "what is ongoing, what is planned, what is done" Mihai asked for.
const BoardGroupHeader: React.FC<{ label: string; color: string; count: number; extra?: React.ReactNode }> = ({
  label,
  color,
  count,
  extra,
}) => (
  <div className="flex items-center gap-1.5 px-3 pt-2 pb-1">
    <span aria-hidden className="shrink-0" style={{ width: 7, height: 7, borderRadius: '50%', background: color }} />
    <span className="font-mono text-[10px] font-bold uppercase tracking-[0.14em]" style={{ color }}>
      {label}
    </span>
    <span className="text-[10px] tabular-nums text-text-secondary">· {count}</span>
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
  const c = interrupted ? CALL_PENDING : TODO_COLOR[row.state];
  const idx = row.device ? deviceIndex(row.device, deviceOrder) : -1;
  const lane = row.lane;
  const calls = lane?.calls ?? [];
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
  const firstBadCall = calls.findIndex((cl) => {
    const k = classifyCall(cl).kind;
    return k === 'malformed' || k === 'ran-nothing';
  });
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
        className={`w-full flex items-center gap-1.5 text-[11px] py-0.5 min-w-0 text-left ${hasDetail ? 'cursor-pointer hover:opacity-80' : ''}`}
      >
        {interrupted ? (
          <CircleSlash className="h-3.5 w-3.5 shrink-0" style={{ color: c }} />
        ) : (
          <TodoGlyph state={row.state} />
        )}
        {idx >= 0 ? (
          <span
            className="text-[9px] font-mono shrink-0"
            style={{ color: FORMATION_RAMP[idx % FORMATION_RAMP.length] }}
          >
            ⬢{String.fromCharCode(65 + (idx % 26))}
          </span>
        ) : null}
        <span
          className="shrink-0 font-medium"
          style={{
            color:
              row.state === 'failed'
                ? STATUS_COLOR.error
                : row.state === 'pending'
                  ? 'var(--color-text-secondary)'
                  : 'var(--color-text-primary)',
          }}
        >
          {row.title}
        </span>
        {row.kind === 'repair' ? <TodoPill text="repair" color="#b14cff" /> : null}
        {row.summary ? <span className="text-text-secondary truncate">· {row.summary}</span> : null}
        {interrupted ? <TodoPill text="interrupted" color={c} /> : null}
        {!interrupted && row.state === 'unverified' ? <TodoPill text="unverified" color={c} /> : null}
        {!interrupted && row.state === 'judge_failed' ? <TodoPill text="judge" color={c} /> : null}
        {!interrupted && row.state === 'blocked' ? <TodoPill text="blocked" color={c} /> : null}
        {judgeFlag ? (
          <Tip label={`The judge intervened: ${judgeFlag}${row.judge?.hint ? ` — ${row.judge.hint.slice(0, 140)}` : ''}`}>
            <span className="flex items-center gap-0.5 shrink-0" style={{ color: AMBER }}>
              <Gavel className="h-3 w-3" /> {judgeFlag}
            </span>
          </Tip>
        ) : null}
        {row.detail ? <span className="text-[10px] text-text-secondary truncate">· {row.detail}</span> : null}
        <span className="ml-auto shrink-0 flex items-center gap-1.5 text-[10px] tabular-nums text-text-secondary">
          {row.state === 'running' && lane?.toolCalls ? (
            <span className="flex items-center gap-0.5">
              <Wrench className="h-3 w-3" />
              {lane.toolCalls}
            </span>
          ) : null}
          {row.state === 'running' && lane?.errors ? (
            <span style={{ color: STATUS_COLOR.error }}>{lane.errors}✕</span>
          ) : null}
          {row.state !== 'running' && secs != null ? <span>{secs}s</span> : null}
          {typeof row.attempts === 'number' && row.attempts > 1 ? <span>×{row.attempts}</span> : null}
          {row.state === 'pending' && row.deps.length ? (
            <span className="font-mono truncate max-w-[12rem]">after {row.deps.join(', ')}</span>
          ) : null}
          {row.state === 'pending' && row.difficulty ? <span>{row.difficulty}</span> : null}
          {hasDetail ? (
            expanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />
          ) : null}
        </span>
      </div>
      {row.state === 'running' && !interrupted && !expanded ? (
        <div className="flex items-center gap-1.5 pl-5 pb-0.5 text-[10px] min-w-0">
          {lastCall ? (
            (() => {
              const cm = classifyCall(lastCall);
              return (
                <>
                  <CallTypeIcon icon={cm.icon} color={CALL_KIND_COLOR[cm.kind]} />
                  <span className="text-text-secondary truncate">
                    {cm.action}
                    {lastCall.summary ? <span className="font-mono text-text-secondary"> · {lastCall.summary}</span> : null}
                  </span>
                </>
              );
            })()
          ) : liveGen ? (
            <span className="text-text-secondary truncate">{liveGen}</span>
          ) : (
            <span className="text-text-secondary">generating…</span>
          )}
        </div>
      ) : null}
      {expanded && hasDetail ? (
        <div
          className="ml-5 mt-1 mb-2 px-2.5 py-2 space-y-2 border border-border-primary bg-background-secondary"
          style={{ borderRadius: CHIP_RADIUS }}
        >
          {row.difficulty || row.deps.length || lane?.model ? (
            <div className="text-[10px] text-text-secondary flex flex-wrap gap-x-3">
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
            <div className="text-[10px] text-text-secondary break-words">
              <span className="uppercase tracking-wide text-text-tertiary">Files</span>{' '}
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
              <div className="text-[10px] uppercase tracking-wide mb-1" style={{ color: STATUS_COLOR.error }}>
                {interrupted ? 'Last error before it stalled' : 'Why it failed'}
              </div>
              <MonoOutput text={laneError} failed />
            </div>
          ) : null}
          {row.description ? <ReasoningBlock text={row.description} label="Task spec" /> : null}
          {reasoning ? (
            <ReasoningBlock
              text={reasoning}
              live={row.state === 'running' && !interrupted}
              forceOpen={dev}
              label={row.state === 'running' ? 'Generating' : 'Reasoning'}
            />
          ) : null}
          {calls.length > 0 ? (
            <div>
              <div className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary mb-1.5">
                Tool calls · {lane?.toolCalls ?? calls.length}
              </div>
              <div className="bg-background-primary border border-border-primary px-2 py-1" style={{ borderRadius: CHIP_RADIUS }}>
                {calls.map((cl, i) => (
                  <CallRow key={i} call={cl} defaultOpen={dev || i === firstBadCall} />
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
    <div className="px-3 pb-1 pl-4 space-y-0">
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
    <div className="border-t border-border-primary">
      <ZoneHeader
        hue={ZONE_HUES.work}
        label="Work"
        explain="the plan as a live board — running, queued, done"
        right={
          total > 0 ? (
            <span className="text-[10px] tabular-nums text-text-secondary">
              {board.running.length} running · {board.queued.length} queued · {board.done.length} done
              {board.addedByReplan > 0 ? ` · +${board.addedByReplan} re-planned` : ''}
            </span>
          ) : undefined
        }
      />
      {board.stuck ? (
        <div className="mx-3 mb-2 px-2 py-1.5 text-[11px] text-white flex items-center gap-1.5" style={{ backgroundColor: STATUS_COLOR.error, borderRadius: CHIP_RADIUS }}>
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
          {board.stuck}
        </div>
      ) : null}
      {total === 0 ? (
        <div className="px-3 pb-2 text-[11px] text-text-secondary">
          {live
            ? 'The plan lands here once it is agreed — tasks appear the moment the first one is dispatched.'
            : 'No tasks were dispatched in this run.'}
        </div>
      ) : (
        <>
          {board.running.length > 0 ? (
            <>
              <BoardGroupHeader label="Running" color={STATUS_COLOR.running} count={board.running.length} />
              {rows(board.running)}
            </>
          ) : null}
          {board.queued.length > 0 ? (
            <>
              <BoardGroupHeader
                label="Queued"
                color={CALL_PENDING}
                count={board.queued.length}
                extra={<span className="text-[10px] text-text-secondary">— waiting on dependencies</span>}
              />
              {rows(board.queued)}
            </>
          ) : null}
          {board.done.length > 0 ? (
            <>
              <BoardGroupHeader
                label="Done"
                color={STATUS_COLOR.done}
                count={board.done.length}
                extra={
                  failedCount > 0 ? (
                    <span className="text-[10px] font-semibold" style={{ color: STATUS_COLOR.error }}>
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
    <div className="border-t border-border-primary px-3 py-2">
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
          className="flex-1 min-w-0 bg-background-primary border border-border-primary px-2 py-1 text-xs text-text-primary placeholder:text-text-secondary focus:outline-none focus:border-text-secondary"
          style={{ borderRadius: CHIP_RADIUS }}
          aria-label="Add a note to this build"
        />
        <button
          type="button"
          onClick={() => void send()}
          disabled={!text.trim() || busy}
          className="shrink-0 px-2.5 py-1 text-xs font-semibold text-white disabled:opacity-40"
          style={{ backgroundColor: SWARM_STATUS.action, borderRadius: CHIP_RADIUS }}
        >
          {busy ? 'Sending…' : 'Send'}
        </button>
      </div>
      {failed ? (
        <div className="text-[11px] mt-1" style={{ color: STATUS_COLOR.error }}>
          Could not write the note. Is the build directory still there?
        </div>
      ) : sent > 0 ? (
        <div className="text-[11px] text-text-secondary mt-1">
          {sent === 1 ? '1 note queued' : `${sent} notes queued`} — it is background context, not an order:
          the spec still wins. Needs “Let me add notes while it builds” on.
        </div>
      ) : null}
    </div>
  );
};

/**
 * WHO ANSWERED — the fact the clarify surface exists to carry.
 *
 * A question is ALWAYS answered now: by you, or — instantly on an unattended run, otherwise after a wait —
 * by goose from the spec. Both halves are shown, because a run that answered its own questions must never
 * read like a run someone steered. `armed` says who WILL answer, before the answer exists; `answered` says
 * who DID.
 */
const ProxyNotice: React.FC<{ proxy: ClarifyProxy }> = ({ proxy }) => {
  if (proxy.failed) {
    return (
      <div
        className="flex items-start gap-2 px-2 py-2 text-xs text-white"
        style={{ backgroundColor: SWARM_STATUS.solidRunning, borderRadius: CHIP_RADIUS }}
      >
        <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-px" />
        <span>
          Goose&apos;s own answer failed, so it took the most conventional option and carried on. An
          unanswered question that idles the whole fleet is worse than a conventional default.
        </span>
      </div>
    );
  }
  if (proxy.answered) {
    return (
      <div
        className="px-2 py-2 text-xs text-white"
        style={{ backgroundColor: SWARM_STATUS.solidRunning, borderRadius: CHIP_RADIUS }}
      >
        <div className="flex items-center gap-2 font-semibold">
          <Bot className="h-3.5 w-3.5 shrink-0" />
          Answered by goose — you did not reply
        </div>
        <ol className="mt-1.5 space-y-1.5">
          {proxy.answered.questions.map((q: string, i: number) => (
            <li key={i}>
              <div className="font-medium">
                {i + 1}. {q}
              </div>
              <div>→ {proxy.answered?.answers[i] ?? ''}</div>
            </li>
          ))}
        </ol>
      </div>
    );
  }
  if (!proxy.armed) return null;
  const { mode, waitSecs, questions } = proxy.armed;
  return (
    <div
      className="flex items-start gap-2 px-2 py-2 text-xs text-white"
      style={{ backgroundColor: SWARM_STATUS.action, borderRadius: CHIP_RADIUS }}
    >
      <Bot className="h-3.5 w-3.5 shrink-0 mt-px" />
      <span>
        {mode === 'immediate'
          ? `Unattended run — goose is answering ${questions === 1 ? 'this' : 'these'} from the spec. Reply here and yours wins.`
          : `No reply in ${Math.round(waitSecs / 60)} min and goose will answer ${questions === 1 ? 'this' : 'these'} from the spec itself.`}
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
}> = ({ clarify, plan, proxy, askFloor = null }) => {
  const [answers, setAnswers] = useState<string[]>(() => clarify.questions.map(() => ''));
  const [guidance, setGuidance] = useState('');
  const [showPlan, setShowPlan] = useState(true);
  const [sent, setSent] = useState(false);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);

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
    return (
      <div
        className="flex items-center gap-2 px-3 py-2 text-xs text-white"
        style={{ backgroundColor: STATUS_COLOR.done }}
      >
        <Check className="h-4 w-4 shrink-0" />
        Sent — goose is building with your answers.
      </div>
    );
  }

  const canSend = answers.some((a) => a.trim().length > 0) || guidance.trim().length > 0;
  return (
    <div className="border-b border-border-primary">
      <div className="flex items-center gap-2 px-3 py-2 text-white" style={{ backgroundColor: AMBER }}>
        <MessageCircleQuestion className="h-4 w-4 shrink-0" />
        <span className="text-xs font-semibold">Review the plan &amp; steer the build</span>
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
                    <span className="text-text-secondary shrink-0">·</span>
                    <InlineMarkdown content={t.description || t.id} />
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}

        {clarify.questions.map((q, i) => (
          <div key={i} className="space-y-1.5">
            <div className="text-xs text-text-primary font-medium">
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
                          ? 'text-white font-medium'
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
              value={q.options.includes(answers[i]) ? '' : answers[i]}
              onChange={(e) => setAnswer(i, e.target.value)}
              placeholder={q.options.length > 0 ? 'or type your own…' : 'your answer…'}
              className="w-full text-xs px-2 py-1.5 bg-background-primary text-text-primary border border-border-primary focus:outline-none focus:border-text-secondary"
              style={{ borderRadius: CHIP_RADIUS }}
            />
          </div>
        ))}

        <div className="space-y-1">
          <div className="text-xs text-text-primary font-medium">Anything else? (optional)</div>
          <textarea
            value={guidance}
            onChange={(e) => setGuidance(e.target.value)}
            rows={2}
            placeholder="Tell goose to change the plan however you like — e.g. “use SQLite, add an export command, skip the web UI”."
            className="w-full text-xs px-2 py-1.5 bg-background-primary text-text-primary border border-border-primary focus:outline-none focus:border-text-secondary resize-y"
            style={{ borderRadius: CHIP_RADIUS }}
          />
        </div>

        {error ? (
          <div className="text-xs" style={{ color: STATUS_COLOR.error }}>
            Couldn&apos;t write the answers file — check that the build directory is still there, then retry.
          </div>
        ) : null}

        <div className="flex items-center gap-2 flex-wrap">
          <button
            type="button"
            onClick={send}
            disabled={busy || !canSend}
            className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white disabled:opacity-50 transition-opacity"
            style={{ backgroundColor: BLUE, borderRadius: CHIP_RADIUS }}
          >
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Send className="h-3.5 w-3.5" />}
            Send answers &amp; build
          </button>
          <span className="text-[11px] text-text-secondary">
            {proxy.armed
              ? 'Your answers guide every worker; the plan shape stays as drafted. Goose answers for you if you leave this.'
              : 'The build is paused until you respond. Your answers guide every worker; the plan shape stays as drafted.'}
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
  /** The planning-history phases only: open, research, synthesis, review. */
  phases: PhaseTodo[];
  planLanes: TurnLane[];
  /** RESEARCH's per-slice lanes — one node per slice, each writing that module's spec. */
  sliceLanes: TurnLane[];
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
  const clarifyPending = !!clarify?.pending;
  const shownPhases = phases.filter((p) => p.items.length > 0);
  // Every planning generation, grouped by what it IS. The slice fan is the interesting one — it is the
  // whole fleet writing specs in parallel, and it had no surface at all before.
  const laneGroups: Array<{ key: string; label: string; lanes: TurnLane[] }> = [
    { key: 'slices', label: 'Slice specs', lanes: sliceLanes },
    { key: 'planning', label: 'Planning calls', lanes: planningLanes },
    { key: 'drafts', label: 'Candidate drafts', lanes: planLanes },
  ].filter((g) => g.lanes.length > 0);
  const hasBody =
    clarifyPending ||
    !!conf ||
    !!proxy.answered ||
    !!proxy.failed ||
    shownPhases.length > 0 ||
    laneGroups.length > 0;
  if (!hasBody && planConfidence == null) return null;
  // Historical once build starts: collapse by default, keep the one-line summary in the header.
  const open = clarifyPending ? true : (openOverride ?? !buildStarted);
  const climb = trail.length >= 2 ? trail[trail.length - 1] - trail[0] : 0;
  const explain = buildStarted
    ? 'how the plan was agreed before building'
    : clarifyPending
      ? 'goose is asking you before it builds'
      : 'agreeing on the plan before building';
  return (
    <div className="border-t border-border-primary">
      <ZoneHeader
        hue={ZONE_HUES.planning}
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
                  className="text-[10px] tabular-nums flex items-center gap-0.5 shrink-0"
                  style={{ color: STATUS_COLOR.done }}
                >
                  <TrendingUp className="h-2.5 w-2.5" /> +{climb}
                </span>
              </Tip>
            ) : null}
            {!open && plan.length > 0 ? (
              <span className="text-[10px] tabular-nums text-text-secondary">
                {plan.length} task{plan.length === 1 ? '' : 's'} planned
              </span>
            ) : null}
          </>
        }
      />
      {open ? (
        clarifyPending && clarify ? (
          <ClarifyPrompt clarify={clarify} plan={plan} proxy={proxy} askFloor={askFloor} />
        ) : (
          <div className="pb-1">
            {/* The questions were settled without you. This is the durable record of that — the prompt
                itself unmounts the moment the answers file lands. */}
            {proxy.answered || proxy.failed ? (
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
            {shownPhases.map((p) => (
              <div key={p.key}>
                <div className="px-3 pt-1 pb-0.5 flex items-center gap-1.5">
                  <span className="text-[10px] font-bold uppercase tracking-[0.12em] text-text-secondary">
                    {p.label}
                  </span>
                  {p.counts.total > 0 ? (
                    <span className="text-[10px] tabular-nums text-text-secondary">
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
              </div>
            ))}
            {laneGroups.map((group) => (
              <div key={group.key} className="mt-1">
                <div className="px-3 pt-1 pb-0.5 flex items-center gap-1.5">
                  <Braces className="h-3 w-3" style={{ color: ZONE_HUES.planning }} />
                  <span className={`${EYEBROW_CLASS} text-text-secondary`}>
                    {group.label} · {group.lanes.length} lane{group.lanes.length === 1 ? '' : 's'}
                    {group.lanes.some((l) => l.status === 'running') ? ' · thinking…' : ''}
                  </span>
                </div>
                <div className="divide-y divide-border-primary">
                  {group.lanes.map((lane) => {
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
            ))}
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
    <div className="border-t border-border-primary">
      <ZoneHeader
        hue={SWARM_STATUS.running}
        label="Known active bugs"
        explain="the run passed — these are what it passed WITH"
        collapsed={!open}
        onToggle={() => setOpen((o) => !o)}
        right={
          <span
            className="text-xs px-2 py-0.5 text-white font-semibold tabular-nums"
            style={{ backgroundColor: SWARM_STATUS.solidRunning, borderRadius: CHIP_RADIUS }}
          >
            {bugs.length}
          </span>
        }
      />
      {open ? (
        <ol className="px-3 pb-3 space-y-1.5">
          {bugs.map((bug, i) => (
            <li key={i} className="flex items-start gap-2 text-xs" style={{ color: GEN_TEXT }}>
              <Bug
                className="h-3.5 w-3.5 shrink-0 mt-0.5"
                style={{ color: SWARM_STATUS.running }}
                aria-hidden
              />
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
  const hdr = 'text-[10px] uppercase tracking-wide text-text-secondary mb-1 mt-2';
  const slate = 'var(--color-node-2, #0891b2)';
  return (
    <div className="border-t border-border-primary px-3 py-3 bg-background-secondary space-y-1">
      <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary">
        <ListChecks className="h-3.5 w-3.5" /> Build overview
        {workingDir ? (
          <button
            onClick={() => void window.electron.revealInFinder(workingDir)}
            title="Reveal the build folder in Finder — every file this run wrote lives here"
            className="ml-auto flex items-center gap-1 text-[10px] font-normal text-text-secondary hover:text-text-primary"
          >
            <FolderOpen className="h-3 w-3" /> Reveal build folder
          </button>
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
        <div
          className="mt-1 px-2 py-1.5 text-[11px] text-white flex items-center gap-1.5"
          style={{ backgroundColor: AMBER, borderRadius: CHIP_RADIUS }}
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
          Not yet verified — the program was built but never run. Everything below describes the code, not
          proof it works.
        </div>
      ) : !overview.generated ? (
        <div
          className="mt-1 px-2 py-1.5 text-[11px] flex items-center gap-1.5 border border-border-primary text-text-secondary"
          style={{ borderRadius: CHIP_RADIUS }}
        >
          <ListChecks className="h-3.5 w-3.5 shrink-0" />
          goose ran this app and it works{overview.runCommand ? <> — <code className="text-text-primary">{overview.runCommand}</code></> : null}. It just
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
                  <li key={i} className="text-[12px] leading-relaxed text-text-primary flex gap-1.5">
                    <span className="text-text-secondary shrink-0">·</span>
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
                  className="text-[11px] font-mono bg-background-primary border border-border-primary px-1.5 py-0.5"
                  style={{ borderRadius: CHIP_RADIUS }}
                >
                  {overview.runCommand}
                </code>
                <span
                  className="text-[9px] uppercase tracking-wide px-1 py-px"
                  style={{
                    color: overview.runCommandVerified ? STATUS_COLOR.done : slate,
                    border: `1px solid ${overview.runCommandVerified ? STATUS_COLOR.done : slate}`,
                    borderRadius: CHIP_RADIUS,
                  }}
                >
                  {overview.runCommandVerified ? 'verified to start' : 'candidate entry — not verified'}
                </span>
              </div>
            ) : (
              <div className="text-[11px] text-text-primary">
                No standalone entry point — this runs inside goose.
              </div>
            )}
            {overview.engage ? (
              <div className="text-[11px] text-text-secondary mt-0.5">{overview.engage}</div>
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
          <div className="text-[11px] text-text-secondary">Verification gates were off this run.</div>
        )}
      </div>
      {overview.generated && overview.next.length ? (
        <div>
          <div className={hdr}>What&apos;s next</div>
          <ul className="space-y-0.5">
            {overview.next.map((n, i) => (
              <li key={i} className="text-[12px] leading-relaxed text-text-primary flex gap-1.5">
                <span className="text-text-secondary shrink-0">→</span>
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
  const cfg = {
    done: { color: STATUS_COLOR.done, Icon: Check, title: 'Build complete' },
    failed: { color: STATUS_COLOR.error, Icon: AlertTriangle, title: `Finished — ${failed} task${failed === 1 ? '' : 's'} failed` },
    stopped: { color: STOPPED, Icon: CircleSlash, title: 'Run stopped' },
  }[outcome];
  const { color, Icon, title } = cfg;
  const parts = [
    `${done}/${tasks} task${tasks === 1 ? '' : 's'} done`,
    outcome !== 'failed' && failed ? `${failed} failed` : null,
    durationLabel ? `in ${durationLabel}` : null,
  ].filter(Boolean);
  return (
    <div className="border-b border-border-primary">
      <div className="flex items-center gap-2 px-3 py-2 text-white" style={{ backgroundColor: color }}>
        <Icon className="h-4 w-4 shrink-0" strokeWidth={2.5} />
        <span className="text-xs font-semibold">{title}</span>
        <span className="text-[11px] tabular-nums">{parts.join(' · ')}</span>
      </div>
      {outcome === 'stopped' ? (
        <div className="px-3 py-1.5 text-[11px] text-text-secondary bg-background-secondary">
          It ended without a completion signal — stopped or crashed mid-build. What finished is shown below.
        </div>
      ) : null}
      {summary && summary.perDevice.length > 0 ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-1.5 text-[11px] bg-background-secondary border-t border-border-primary">
          {summary.perDevice.map((d) => {
            // Key the hue by the CANONICAL node name — d.device is the raw pool id, which is not in
            // deviceOrder, so every node collapsed onto the same out-of-range hue.
            const hue = FORMATION_RAMP[deviceIndex(d.node, deviceOrder) % FORMATION_RAMP.length];
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
                <span className="inline-flex items-center gap-1 tabular-nums cursor-default">
                  <span className="font-semibold" style={{ color: hue }}>
                    {d.node}
                  </span>
                  <span className="text-text-secondary">
                    {d.dispatched}t · {d.toolCalls}🔧 · {fmtDuration(d.busyMs / 60000)}
                  </span>
                </span>
              </Tip>
            );
          })}
        </div>
      ) : null}
      {outputDir ? (
        <Tip label={<span className="font-mono break-all">{outputDir}</span>}>
          <div className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-text-secondary bg-background-secondary border-t border-border-primary min-w-0 cursor-default">
            <FolderOpen className="h-3 w-3 shrink-0" />
            <span className="font-mono truncate">{outputDir}</span>
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

/** How much of the run the panel shows. A real radiogroup with all three choices visible — the old control
 *  was one button that cycled, so the two modes you were not in were invisible and unreachable by keyboard. */
export const DetailModeChooser: React.FC<{
  mode: SwarmLogMode;
  onChange: (mode: SwarmLogMode) => void;
}> = ({ mode, onChange }) => {
  const optionRefs = useRef<Partial<Record<SwarmLogMode, HTMLButtonElement | null>>>({});

  const onKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    const next = nextDetailMode(mode, event.key);
    if (!next) return;
    event.preventDefault();
    onChange(next);
    optionRefs.current[next]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label="Run detail"
      className="flex items-center overflow-hidden border border-border-primary bg-background-primary"
      style={{ borderRadius: CHIP_RADIUS }}
    >
      <span className="flex h-7 w-7 items-center justify-center border-r border-border-primary text-text-secondary">
        <AlignLeft className="h-3.5 w-3.5" aria-hidden />
      </span>
      {SWARM_LOG_MODES.map((option, index) => {
        const selected = option === mode;
        return (
          <button
            ref={(element) => {
              optionRefs.current[option] = element;
            }}
            key={option}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(option)}
            onKeyDown={onKeyDown}
            className={`h-7 px-2 text-xs font-semibold outline-none focus-visible:ring-2 focus-visible:ring-inset ${
              selected ? 'focus-visible:ring-white' : 'focus-visible:ring-[var(--color-action-solid,#1d4ed8)]'
            } ${index < SWARM_LOG_MODES.length - 1 ? 'border-r border-border-primary' : ''} ${
              selected ? 'text-white' : 'text-text-secondary hover:bg-background-secondary hover:text-text-primary'
            }`}
            style={{ backgroundColor: selected ? SWARM_STATUS.action : 'transparent' }}
          >
            {DETAIL_MODE_LABEL[option]}
          </button>
        );
      })}
    </div>
  );
};

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
  // LM Studio's OWN live per-node status (lms ps --json) — the ground-truth generating/idle dot per node.
  const nodeStatus = useFleetStatus();
  const [mode, setMode] = useSwarmLogMode();
  const verbose = mode !== 'compact';
  const dev = mode === 'developer';

  // Show whenever a run is present — including the PLANNING phase, before any worker executes (no lanes yet).
  if (!run.present) return null;

  // Stable node identity: run.lanes RE-SORTS every poll (running first, then recency), so deriving letters
  // from first-seen lane order made a node's letter/hue flicker between polls. deriveFleet keys the row set
  // off the RESOLVED POOL (pool_resolved / run_started.pool), sorted deterministically, so EVERY fleet node
  // renders — idle ones as an explicit idle row, never absence — and ⬢A/hue is fixed for the whole run.
  // ALL lane kinds count toward WORKING in every mode (scouts/contracts/detailers/repair twins included):
  // the mode toggle controls display density below, not whether a busy node reads busy.
  const fleet = deriveFleet({
    pool: run.pool,
    laneSources: [
      ...run.lanes,
      ...run.planLanes,
      ...run.scoutLanes,
      ...run.contractLanes,
      ...run.detailLanes,
      // The rewritten pipeline's own lanes: the slice fan (one node per slice, RESEARCH) and the
      // single-node planning calls (open / synthesis / review / proxy-answer / rate). Without them the
      // Fleet zone reads "idle — no task" for the entire planning half of the run.
      ...run.sliceLanes,
      ...run.planningLanes,
      ...run.fixLanes,
    ],
    digests: run.activityDigests,
    digestMtimes: run.activityMtimes,
    now: Date.now(),
    // SUPERVISION: open judge spans (foldSupervision) joined to the nodes LM Studio itself reports busy —
    // the workload class that used to render a hard-working node as "idle — no task".
    supervision: run.supervision,
    busyNodes: Object.entries(nodeStatus)
      .filter(([, st]) => st === 'generating' || st === 'processingPrompt')
      .map(([n]) => n),
  });
  const deviceOrder: string[] = fleet.devices;
  // The WORK board — the single source of truth for plan / ongoing / done (see deriveTaskBoard).
  const board = deriveTaskBoard({
    plan: run.plan,
    phaseTodo: run.phaseTodo,
    lanes: run.lanes,
    fixLanes: run.fixLanes,
  });
  // The four planning phases live in the PLANNING zone; build / integrate / repair ARE the work board.
  const planningPhases = run.phaseTodo.filter(
    (p) =>
      p.key === 'open' || p.key === 'research' || p.key === 'synthesis' || p.key === 'review'
  );
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

  // A run is OVER when the engine said so. Nothing else — no timer, no quiet window — may end it.
  const clarifyPending = !!run.clarify?.pending;
  const ended = run.finished;
  // The APP-LEVEL oracle: the engine's own end-to-end verify (complete_result -> phaseTodo v-e2e = 'done').
  // A green verify means the deliverable WORKS — so the run is 'done' and the overview shows — EVEN IF an
  // individual build task failed (e.g. the integrate-verify sink stalled but the orchestrator's verify still
  // passed). Without this, one failed task forced outcome='failed', which suppressed the overview and headlined
  // "1 task failed" on a working, verified app. The failed task stays visible in the Build phase-todo + counts.
  const appVerified =
    (run.phaseTodo.find((p) => p.key === 'integrate')?.items ?? []).find((i) => i.id === 'v-e2e')
      ?.state === 'done';
  const outcome: 'done' | 'failed' | 'stopped' | null = !ended
    ? null
    : run.finished
      ? appVerified || (run.summary?.failed ?? failed) === 0
        ? 'done'
        : 'failed'
      : 'stopped';
  const durationMin =
    run.summary?.totalMin != null
      ? run.summary.totalMin
      : run.startedAt != null && run.mtime != null
        ? (run.mtime - run.startedAt) / 60000
        : null;
  const durationLabel = durationMin != null ? fmtDuration(durationMin) : null;
  const activePhaseColor = ended
    ? outcome === 'done'
      ? SWARM_STATUS.solidDone
      : outcome === 'failed'
        ? SWARM_STATUS.solidError
        : SWARM_STATUS.solidStopped
    : SWARM_STATUS.action;
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
      className={`border border-border-primary bg-background-secondary text-text-primary text-sm ${className}`}
      style={{ borderRadius: PANEL_RADIUS }}
    >
      {/* ── RUN HEADER zone — identity + state in ONE band: what is being built, the phase, counts,
          elapsed/ETA, pause + display mode. Replaces the floating fragments (brand pill, metrics strip,
          breadcrumb) Mihai read as "lacking any visual definition". */}
      <div className="border-b border-border-primary">
      <div className="flex items-center justify-between px-3 py-2 gap-2">
        <span className="flex items-center gap-2 min-w-0">
          <span
            aria-hidden
            className="shrink-0"
            style={{ width: 8, height: 8, background: ZONE_HUES.run, borderRadius: 1 }}
          />
          <span className={`${EYEBROW_CLASS} shrink-0`} style={{ color: ZONE_HUES.run }}>
            Swarm run
          </span>
          <Tip
            label={
              run.meta?.prompt ? (
                <span className="whitespace-pre-wrap">{run.meta.prompt.slice(0, 400)}</span>
              ) : (
                'What this run is building'
              )
            }
          >
            <span className="text-xs font-semibold truncate text-text-primary">{appName}</span>
          </Tip>
          {clarifyPending ? (
            // Paused waiting on the human — NOT active work, so no spinner and a distinct amber "paused" chip
            // (the old code showed a spinning "Planning" here, implying it was still churning).
            <Tip label="The build is paused, waiting for your answers in the prompt below.">
              <span
                className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
                style={{ backgroundColor: AMBER, borderRadius: CHIP_RADIUS }}
              >
                <MessageCircleQuestion size={10} /> Waiting for you
              </span>
            </Tip>
          ) : run.held ? (
            // HELD — engine truth (run_paused with no later run_unpaused). NO SPINNER: a spinning badge is a
            // claim that work is happening, and while held every node is deliberately idle. MEASURED: Mihai
            // watched this badge read a spinning "Building" through a 20-minute hold and reasonably concluded
            // the run had hung. The phase label is suppressed too — "which task is next" is not the question
            // someone asks when nothing is moving.
            <Tip label="Held at a task boundary. In-flight work finished and nothing was lost — press ▶ to resume.">
              <span
                className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
                style={{ backgroundColor: AMBER, borderRadius: CHIP_RADIUS }}
              >
                <Pause size={10} /> Paused
              </span>
            </Tip>
          ) : run.inProgress && !stale && !ended && run.phase ? (
            <Tip label={`Current phase: ${run.phase}`}>
              <span
                className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
                style={{ backgroundColor: STATUS_COLOR.running, borderRadius: CHIP_RADIUS }}
              >
                <Loader2 size={10} className="animate-spin" /> {run.phase}
              </span>
            </Tip>
          ) : null}
        </span>
        <span className="flex items-center gap-2 shrink-0">
          <span className="text-xs text-text-secondary tabular-nums">
            {tasks > 0 && (
              <>
                {running > 0 && (
                  <span style={{ color: stale ? CALL_PENDING : STATUS_COLOR.running }} className="font-semibold">
                    {running} {stale ? 'interrupted' : 'running'}
                  </span>
                )}
                {running > 0 && ' · '}
                <span style={{ color: STATUS_COLOR.done }}>{done} done</span>
                {failed > 0 && (
                  <>
                    {' · '}
                    <span style={{ color: STATUS_COLOR.error }} className="font-semibold">
                      {failed} failed
                    </span>
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
          {run.inProgress && !ended && !clarifyPending && workingDir ? (
            // PAUSE / RESUME — hold the build at the next task boundary (in-flight work finishes, nothing is
            // lost) and resume re-running nothing. Amber (not red) + a ‖/▶ glyph so it never reads as the
            // terminal ■ stop. "Held" is engine-truth (run_paused event); "Pausing…" is the pending request.
            <button
              onClick={() => runDir && window.electron.swarmSetPaused(runDir, !run.pauseRequested)}
              className="flex items-center gap-1 text-[10px] px-1.5 py-0.5 border transition-colors"
              style={{
                borderRadius: CHIP_RADIUS,
                borderColor: AMBER,
                color: run.pauseRequested ? '#fff' : AMBER,
                backgroundColor: run.pauseRequested ? AMBER : 'transparent',
              }}
              title={
                !run.pauseRequested
                  ? 'Hold at the next task boundary — in-flight work finishes, nothing is lost'
                  : run.held
                    ? 'Resume the build (re-runs nothing)'
                    : 'Pausing — finishing the current task, then holding. Click to resume.'
              }
            >
              {!run.pauseRequested ? (
                <>
                  <Pause size={11} /> Pause
                </>
              ) : run.held ? (
                <>
                  <Play size={11} /> Resume
                </>
              ) : (
                <>
                  <Loader2 size={11} className="animate-spin" /> Pausing…
                </>
              )}
            </button>
          ) : null}
          <DetailModeChooser mode={mode} onChange={setMode} />
        </span>
      </div>
      {/* Row 2 of the band: the run's ROUTE and its real fleet in one formation — which engine phase is
          live, and which nodes are working under it. The active step is the engine's own phase key; a held
          run has none, and the ribbon lights nothing rather than asserting work that is not happening. */}
      {(run.inProgress || ended) && (
        <FormationRibbon
          phase={run.runPhase}
          nodes={formationNodes}
          evidence={run.runPhasesObserved}
          activeColor={activePhaseColor}
          metrics={
            run.inProgress && !stale && !ended && !clarifyPending ? (
              <HeaderMetrics startedAt={run.startedAt} phaseTodo={run.phaseTodo} />
            ) : null
          }
        />
      )}
      </div>

      {/* The engine's own liveness, as a WARNING and never a verdict. `EXITED:` in .swarm/heartbeat means the
          run future returned early and tore itself down; a frozen stamp means the process was hard-killed.
          Neither ends the run here — the panel keeps showing everything it had. */}
      {!ended && (liveness.state === 'exited' || liveness.state === 'silent') ? (
        <div
          className="flex items-start gap-2 px-3 py-2 text-xs text-white"
          style={{ backgroundColor: SWARM_STATUS.solidRunning }}
        >
          <AlertTriangle className="h-4 w-4 shrink-0 mt-px" />
          <span>
            {liveness.state === 'exited'
              ? 'The engine exited on its own — it stopped writing its heartbeat and stamped an exit. Everything below is what it had reached.'
              : `No heartbeat for ${Math.round(liveness.since / 1000)}s. The engine ticks every 5s, so it was most likely hard-killed; nothing below has been discarded.`}
          </span>
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
        <div className="border-t border-border-primary">
          <ZoneHeader
            hue={ZONE_HUES.fleet}
            label="Fleet"
            explain="what each node is doing right now"
            right={
              <span className="text-[10px] tabular-nums text-text-secondary">
                {deviceOrder.length} node{deviceOrder.length === 1 ? '' : 's'}
                {run.inProgress && !stale && !ended ? ` · ${fleet.workingByDevice.size} working` : ''}
              </span>
            }
          />
          <FleetStrip
            deviceOrder={deviceOrder}
            runningByDevice={fleet.workingByDevice}
            dev={dev}
            live={run.inProgress && !stale && !ended}
            nodeStatus={nodeStatus}
            unattributed={fleet.unattributed}
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
