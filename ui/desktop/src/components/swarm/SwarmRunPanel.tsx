import React, { useState } from 'react';
import {
  Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench,
  Search, ListChecks, Play, FlaskConical, RotateCcw, Gavel, Eye, FileText, Cpu, AlignLeft,
  MessageCircleQuestion, Send, Gauge, AlertTriangle, FolderOpen, TrendingUp, Info, Braces,
  Circle, Minus, ListTodo, Terminal, FilePlus2, FilePenLine, Hammer,
} from 'lucide-react';
import {
  useSwarmRun,
  classifyCall,
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
  type RunOverview as RunOverviewData,
} from './useSwarmRun';
import { useSwarmLogMode, type SwarmLogMode } from './useVerboseSwarm';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';
import InlineMarkdown from './InlineMarkdown';
import StructuredContent, { CodeBlock } from './StructuredContent';

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

// Node identity ramp — matches FanInCard so a node reads the same across the fleet + run views.
const FORMATION_RAMP = ['#17c4c4', '#2e8bff', '#6a5cff', '#b14cff', '#ff3ea5', '#ff5c7a'];
const STATUS_COLOR: Record<TurnStatus, string> = {
  running: '#f5a623',
  done: '#2ecc71',
  error: '#ff3b30',
};
// A worker rewrites its digest each turn, but a single long tool call (cargo build, a big pytest run)
// produces no write while it runs. Keep this above realistic single-tool durations so a live worker isn't
// mislabelled "interrupted"; a genuinely dead run still goes stale within this window.
const STALE_MS = 300_000;
// With a liveness heartbeat present, a dead engine is detectable in seconds (the ticker touches every ~5s),
// so a much shorter window is safe and doesn't false-positive on long tool calls.
const HEARTBEAT_STALE_MS = 45_000;
const CALL_OK = '#2ecc71';
const CALL_ERR = '#ff3b30';
const CALL_PENDING = '#8a8a8a';
const AMBER = '#f5a623';
const BLUE = '#2e8bff';
// A solid slate for a run that stopped without finishing — neutral (not an error) but dark enough for white
// banner text (grey #8a8a8a fails contrast on white). Distinct from the amber "running" and red "failed".
const STOPPED = '#4b5563';

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
  malformed: CALL_ERR,
  pending: CALL_PENDING,
};
// A short pill word per kind so the row reads at a glance.
const CALL_KIND_PILL: Record<CallMeaning['kind'], string> = {
  ok: '',
  'app-error': 'app output',
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
  return (
    <div className="py-0.5 border-b border-border-primary/30 last:border-0">
      <button
        type="button"
        onClick={() => hasOutput && setOpen((o) => !o)}
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
                style={{ color, border: `1px solid ${color}`, borderRadius: 2 }}
              >
                {pill}
              </span>
            ) : null}
            {m.kind !== 'ok' ? (
              <span className="text-[10px]" style={{ color: m.kind === 'malformed' ? color : 'var(--text-secondary)' }}>
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
    </div>
  );
};

// The worker's full narration, rendered as readable PROSE (not mono), capped with a Show-all escape hatch.
// In developer mode it starts fully expanded (no cap).
const ReasoningBlock: React.FC<{ text: string; forceOpen?: boolean; label?: string }> = ({
  text,
  forceOpen,
  label,
}) => {
  const [expandedState, setExpanded] = useState(false);
  const expanded = expandedState || !!forceOpen;
  const words = text.split(/\s+/).filter(Boolean).length;
  const big = text.length > 1200;
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">
        {label || 'Reasoning'}
      </div>
      <div
        className={`text-xs text-text-primary break-words leading-relaxed bg-background-primary border border-border-primary px-2 py-1.5 ${!expanded && big ? 'max-h-[22rem] overflow-hidden' : ''}`}
        style={{ borderRadius: 3 }}
      >
        {/* Prose gets the markdown path; a STRUCTURED payload gets a code path. The plan skeleton used to
            arrive here as raw JSON and markdown both reflowed it into an unreadable wall AND corrupted it —
            `__init__.py` reads as bold syntax, so the file list rendered as **init**.py. */}
        <StructuredContent content={text} />
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
  onToggle: () => void;
}> = ({ lane, deviceOrder, stale, open, mode, dev, onToggle }) => {
  const idx = deviceIndex(lane.device, deviceOrder);
  const hue = FORMATION_RAMP[idx % FORMATION_RAMP.length];
  const letter = String.fromCharCode(65 + (idx % 26));

  const live = lane.status === 'running' && !stale;
  const interrupted = lane.status === 'running' && stale;
  const Icon = interrupted ? CircleSlash : lane.status === 'done' ? Check : lane.status === 'error' ? X : Loader2;
  const iconColor = interrupted ? CALL_PENDING : STATUS_COLOR[lane.status];

  const calls = lane.calls ?? [];
  // Prefer the FULL narration; fall back to the short digest, then last_text. fullReasoning is real prose,
  // so no length/regex gate is needed — show whatever substantive text the worker produced.
  const rawReasoning = lane.fullReasoning?.trim() || lane.reasoning?.trim() || lane.lastText?.trim() || '';
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
        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-background-primary/40 transition-colors"
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
                  <span className="font-mono opacity-80">{lane.taskId}</span>
                </>
              ) : null}
            </span>
          }
        >
          <span className="flex-1 min-w-0 flex flex-col leading-tight">
            <span className="truncate text-xs text-text-primary">{lane.description || lane.taskId}</span>
            {lane.description ? (
              <span className="truncate text-[10px] font-mono text-text-secondary">{lane.taskId}</span>
            ) : null}
          </span>
        </Tip>
        {lane.model && (
          <Tip label={<span className="font-mono">{lane.model}</span>}>
            <span className="hidden sm:inline shrink-0 max-w-[9rem] truncate text-[10px] font-mono text-text-secondary">
              {lane.model}
            </span>
          </Tip>
        )}
        <span className="text-xs text-text-secondary tabular-nums shrink-0 flex items-center gap-1.5">
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
                  forceOpen={dev || live}
                  // Developer: name the model so it's unmistakable WHOSE generation this is.
                  label={dev ? `${live ? 'Generating' : 'Reasoning'} · ${lane.model ?? lane.device}` : undefined}
                />
              )}
              {calls.length > 0 ? (
                <div>
                  <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">
                    Tool calls · {lane.toolCalls ?? calls.length}
                  </div>
                  <div
                    className="bg-background-primary border border-border-primary px-2 py-1"
                    style={{ borderRadius: 3 }}
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
  phase: Search,
  plan: ListChecks,
  dispatch: Play,
  done: Check,
  fail: X,
  retry: RotateCcw,
  retarget: TrendingUp,
  review: FlaskConical,
  judge: Gavel,
  prereview: Eye,
  smoke: FlaskConical,
  brief: FileText,
  config: Cpu,
};
const ACTIVITY_COLOR: Record<ActivityItem['kind'], string> = {
  phase: '#2e8bff',
  plan: '#6a5cff',
  dispatch: '#17c4c4',
  done: '#2ecc71',
  fail: '#ff3b30',
  retry: '#f5a623',
  retarget: '#6a5cff',
  review: '#b14cff',
  judge: '#b14cff',
  prereview: '#17c4c4',
  smoke: '#2ecc71',
  brief: '#8a8a8a',
  config: '#8a8a8a',
};
const TONE_COLOR: Record<NonNullable<ActivityItem['tone']>, string> = {
  info: '#8a8a8a',
  good: '#2ecc71',
  warn: '#f5a623',
  bad: '#ff3b30',
};

// One line in the activity timeline. Tone (when set) tints the icon so judge warnings / failures stand out.
const ActivityLine: React.FC<{ it: ActivityItem; wrap?: boolean }> = ({ it, wrap }) => {
  const Icon = ACTIVITY_ICON[it.kind];
  const color = it.tone ? TONE_COLOR[it.tone] : ACTIVITY_COLOR[it.kind];
  return (
    <div className="flex items-start gap-2 text-xs">
      <Icon size={13} strokeWidth={2.5} className="mt-0.5 shrink-0" style={{ color }} />
      <span className="text-text-primary shrink-0">{it.text}</span>
      {it.sub && (
        <span
          className={`text-text-secondary ${wrap ? 'break-words' : 'truncate'} ${it.kind === 'brief' ? 'line-clamp-3' : ''}`}
        >
          — {it.sub}
        </span>
      )}
    </div>
  );
};

// The live "what goose is doing" timeline — the fix for a build showing nothing during planning. Latest
// at the bottom; a spinner tail while the run is live. In verbose mode it shows the FULL stream and wraps.
const ActivityFeed: React.FC<{ items: ActivityItem[]; live: boolean; verbose: boolean }> = ({ items, live, verbose }) => {
  const shown = verbose ? items : items.slice(-8);
  if (items.length === 0) return null;
  return (
    <div className="px-3 py-2 space-y-1 border-b border-border-primary bg-background-primary">
      {shown.map((it) => (
        <ActivityLine key={it.seq} it={it} wrap={verbose} />
      ))}
      {live && (
        <div className="flex items-center gap-2 text-xs text-text-secondary">
          <Loader2 size={13} className="animate-spin shrink-0" />
          <span>working…</span>
        </div>
      )}
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

// One labelled sub-bar (agreement / spec-clarity): a solid fill in the value's threshold color over a
// bordered track (not a faded tint), so the LOWER (binding) signal visibly reads as the culprit.
const ConfBar: React.FC<{ label: string; value: number }> = ({ label, value }) => (
  <div className="flex items-center gap-2 text-[11px]">
    <span className="w-20 shrink-0 text-text-secondary">{label}</span>
    <div
      className="flex-1 h-1.5 bg-background-primary border border-border-primary overflow-hidden"
      style={{ borderRadius: 2 }}
    >
      <div
        className="h-full"
        style={{ width: `${Math.max(0, Math.min(100, value))}%`, backgroundColor: confColor(value) }}
      />
    </div>
    <span className="w-6 text-right tabular-nums" style={{ color: confColor(value) }}>
      {value}
    </span>
  </div>
);

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
    <svg width={size} height={size} viewBox="0 0 80 80" className="shrink-0" role="img" aria-label={`plan confidence ${v} of 100`}>
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        stroke="var(--border-primary)"
        strokeWidth="7"
        strokeDasharray={`${track} ${circ}`}
        transform="rotate(135 40 40)"
      />
      <circle
        cx="40"
        cy="40"
        r={r}
        fill="none"
        stroke={col}
        strokeWidth="7"
        strokeDasharray={`${fill} ${circ}`}
        transform="rotate(135 40 40)"
        style={{ transition: 'stroke-dasharray 500ms ease-out' }}
      />
      <text
        x="40"
        y="39"
        textAnchor="middle"
        dominantBaseline="middle"
        style={{ fill: col, fontSize: 23, fontWeight: 700, fontVariantNumeric: 'tabular-nums' }}
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
        style={{ fill: col, fontSize: 9, fontWeight: 600, letterSpacing: 0.5, opacity: 0.85 }}
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

const ConfidenceBreakdownBody: React.FC<{
  conf: ConfidenceBreakdown;
  trail?: number[];
  hasPendingQuestions: boolean;
  askFloor?: number | null;
}> = ({ conf, trail, hasPendingQuestions, askFloor = null }) => {
  const bindingAgreement = conf.agreement <= conf.specClarity;
  const showDecisions = !bindingAgreement && conf.openDecisions.length > 0;
  const holdingBack = bindingAgreement
    ? conf.agreementReason || 'The planning drafts disagree on how to structure the build.'
    : conf.productSpecified
      ? 'Some requirements are still ambiguous.'
      : "The product itself isn't fully specified yet.";
  const raiseIt = bindingAgreement
    ? 'The drafts disagree on how to structure the build. The score reflects that drafted plan — it only lifts if Goose re-drafts toward a consensus (the retarget option), and a small/weak fleet may still not fully agree.'
    : hasPendingQuestions
      ? 'Answer the questions below — each resolves an open decision. Goose can also research the undecided points.'
      : 'Researching the undecided points to firm up the spec.';
  return (
    <div className="space-y-2.5">
      <div className="flex items-center gap-3">
        <ConfGauge value={conf.final} askFloor={askFloor} />
        <div>
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">Plan confidence</div>
          <div className="text-[12px] font-medium mt-0.5" style={{ color: confColorVsFloor(conf.final, askFloor) }}>
            {confVerdict(conf.final, askFloor)}
          </div>
        </div>
      </div>
      <div className="space-y-2">
        <div>
          <ConfBar label="Agreement" value={conf.agreement} />
          {conf.agreementReason ? (
            <div className="text-[10px] text-text-secondary mt-0.5 pl-[5.5rem]">
              {conf.agreementReason}
            </div>
          ) : null}
        </div>
        <div>
          <ConfBar label="Spec clarity" value={conf.specClarity} />
          {conf.specClarityReason ? (
            <div className="text-[10px] text-text-secondary mt-0.5 pl-[5.5rem]">
              {conf.specClarityReason}
            </div>
          ) : null}
        </div>
      </div>
      <div>
        <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">
          What&apos;s holding it back
        </div>
        {showDecisions ? (
          <ul className="space-y-0.5">
            {conf.openDecisions.map((d, i) => (
              <li key={i} className="text-[11px] text-text-primary flex gap-1.5">
                <span className="text-text-secondary shrink-0">·</span>
                <span>{d}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="text-[11px] text-text-primary">{holdingBack}</div>
        )}
      </div>
      <div>
        <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">
          What would raise it
        </div>
        <div className="text-[11px] text-text-primary">{raiseIt}</div>
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

// Full-width breakdown section that drops in under the header when the badge is expanded. Informational (no
// loud colored strip), sharp corners, no left rail.
const ConfidencePanel: React.FC<{
  conf: ConfidenceBreakdown;
  trail?: number[];
  hasPendingQuestions: boolean;
  askFloor?: number | null;
}> = (p) => (
  <div className="border-b border-border-primary bg-background-secondary px-3 py-3">
    <ConfidenceBreakdownBody {...p} />
  </div>
);

// Compact header pill — the live meter (advances as the swarm retargets). Click to expand the breakdown; a
// +Δ chip appears when the meter has climbed over the run.
const ConfidenceBadge: React.FC<{
  value: number;
  trail?: number[];
  expanded?: boolean;
  onToggle?: () => void;
  hasBreakdown?: boolean;
  askFloor?: number | null;
}> = ({ value, trail, expanded, onToggle, hasBreakdown, askFloor = null }) => {
  const color = confColorVsFloor(value, askFloor);
  // Same rule as the expanded panel: the verdict comes from the ENGINE's floor, never a band the UI made
  // up. The panel was fixed for this and the badge's tooltip was not, so the tooltip went on calling a 73
  // "confident" while the run had ASKED because 73 was under the bar of 80 — with the floor named in the
  // very next sentence of the same tooltip.
  const label = confVerdict(value, askFloor);
  const climb = trail && trail.length >= 2 ? trail[trail.length - 1] - trail[0] : 0;
  const pill = (
    <span
      className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium tabular-nums"
      style={{ backgroundColor: color, borderRadius: 2 }}
    >
      <Gauge className="h-2.5 w-2.5" />
      conf {value}
      {hasBreakdown ? (
        expanded ? (
          <ChevronDown className="h-2.5 w-2.5" />
        ) : (
          <ChevronRight className="h-2.5 w-2.5" />
        )
      ) : null}
    </span>
  );
  return (
    <span className="flex items-center gap-1 shrink-0">
      <Tip
        label={`Planner confidence in how it broke this app down — ${value}/100. ${label}.${hasBreakdown ? ' Click for the breakdown.' : ''}`}
      >
        {hasBreakdown ? (
          <button type="button" onClick={onToggle} className="flex">
            {pill}
          </button>
        ) : (
          pill
        )}
      </Tip>
      {climb > 0 ? (
        <Tip label={`Confidence climbed ${trail![0]} → ${trail![trail!.length - 1]} as goose retargeted it.`}>
          <span
            className="text-[10px] tabular-nums flex items-center gap-0.5 shrink-0"
            style={{ color: STATUS_COLOR.done }}
          >
            <TrendingUp className="h-2.5 w-2.5" /> +{climb}
          </span>
        </Tip>
      ) : null}
    </span>
  );
};

// The fixed pipeline every build moves through, so the free-text `phase` label reads as PROGRESS, not a
// mystery. The active step is filled; passed steps get a check; upcoming steps stay muted.
const PHASE_STEPS = ['Research', 'Plan', 'Contracts', 'Build', 'Verify', 'Done'] as const;
// What each pipeline step actually is — the labels alone (esp. "Contracts") are opaque without this.
const PHASE_TIPS: Record<(typeof PHASE_STEPS)[number], string> = {
  Research: 'Scouts research the problem — libraries, architecture, edge cases — before any code is written.',
  Plan: 'The planner drafts and picks a task breakdown, and scores its confidence in the decomposition.',
  Contracts: 'Per-module interface stubs are drafted so nodes building in parallel agree on shared APIs.',
  Build: 'Worker nodes build the tasks in parallel across the fleet.',
  Verify: 'Integration, review, and an end-to-end smoke test that actually runs the program.',
  Done: 'The run finished.',
};
function phaseStepIndex(phase: string): number {
  const p = phase.toLowerCase();
  if (/done|finished|complete/.test(p)) return 5;
  if (/verif|integrat/.test(p)) return 4;
  if (/contract/.test(p)) return 2;
  if (/build|execut|dispatch|working/.test(p)) return 3;
  if (/plan/.test(p)) return 1;
  if (/research|scout|start/.test(p)) return 0;
  return 3;
}
const PhaseSteps: React.FC<{ phase: string; activeColor?: string; live?: boolean }> = ({
  phase,
  activeColor = STATUS_COLOR.running,
  live = false,
}) => {
  const active = phaseStepIndex(phase);
  return (
    <div className="flex items-center gap-1 px-3 py-1.5 border-b border-border-primary overflow-x-auto">
      {PHASE_STEPS.map((step, i) => (
        <React.Fragment key={step}>
          {i > 0 && <span className="text-text-secondary text-[10px] shrink-0">›</span>}
          <Tip label={PHASE_TIPS[step]}>
            <span
              className={`text-[10px] px-1.5 py-0.5 whitespace-nowrap shrink-0 inline-flex items-center gap-0.5 ${
                i === active
                  ? `text-white font-semibold${live ? ' animate-pulse' : ''}`
                  : i < active
                    ? 'text-text-primary'
                    : 'text-text-secondary opacity-60'
              }`}
              style={i === active ? { backgroundColor: activeColor, borderRadius: 2 } : undefined}
            >
              {i < active ? <Check className="h-2.5 w-2.5" strokeWidth={3} style={{ color: STATUS_COLOR.done }} /> : null}
              {step}
            </span>
          </Tip>
        </React.Fragment>
      ))}
    </div>
  );
};

// Per-phase TODO. Every item's state is driven by an ENGINE event (see buildPhaseTodo) — the honesty is in
// the colors: 'done' (green) is a VERIFIED completion; 'unverified' is a SLATE check (built but the app was
// never run — must never look green); 'advisory' is info, never a check.
const TODO_COLOR: Record<TodoState, string> = {
  pending: '#8a8a8a',
  running: AMBER,
  done: STATUS_COLOR.done,
  unverified: '#5b8db8', // slate-blue — deliberately NOT green
  failed: STATUS_COLOR.error,
  judge_failed: AMBER,
  blocked: '#8a8a8a',
  skipped: '#6b7280',
  advisory: '#2e8bff',
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
    style={{ color, border: `1px solid ${color}`, borderRadius: 2 }}
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

// One task row: TITLE + short summary collapsed; the full spec, owned files, and the judge's reasoning are
// tucked under an expand. A 'running' item on a STALE run is relabeled 'interrupted' (the process is dead).
const PhaseTodoRow: React.FC<{ item: PhaseTodoItem; deviceOrder: string[]; stale: boolean }> = ({
  item,
  deviceOrder,
  stale,
}) => {
  const interrupted = stale && item.state === 'running';
  const c = interrupted ? CALL_PENDING : TODO_COLOR[item.state];
  const idx = item.device ? deviceIndex(item.device, deviceOrder) : -1;
  const hasDetail = !!(item.description || (item.files && item.files.length) || item.judge);
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
          style={{ color: item.state === 'pending' ? 'var(--text-secondary)' : 'var(--text-primary)' }}
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
          {item.judge ? <JudgeReason judge={item.judge} /> : null}
          {item.files && item.files.length ? (
            <div className="text-[10px] text-text-secondary">
              <span className="uppercase tracking-wide">Files</span>{' '}
              <span className="font-mono text-text-primary">{item.files.join(', ')}</span>
            </div>
          ) : null}
          {item.description ? (
            <ReasoningBlock text={item.description} label="Full task spec" />
          ) : null}
        </div>
      ) : null}
    </div>
  );
};

// The accordion of per-phase checklists. Active phase default-open; finished phases collapse to a counts
// header. Sits under the phase breadcrumb, above the activity feed.
const PhaseTodoList: React.FC<{ phases: PhaseTodo[]; deviceOrder: string[]; stale: boolean }> = ({
  phases,
  deviceOrder,
  stale,
}) => {
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const shown = phases.filter((p) => p.items.length > 0);
  if (shown.length === 0) return null;
  return (
    <div className="border-b border-border-primary">
      <div className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wide text-text-secondary flex items-center gap-1.5">
        <ListTodo className="h-3 w-3" /> Phase checklist
        <span className="text-[9px] normal-case text-text-secondary opacity-70">
          · every check is an engine fact, not a claim
        </span>
      </div>
      {shown.map((p) => {
        const isOpen = open[p.key] ?? p.active;
        // On a stale run a 'running' phase isn't live — it stopped. Show 'interrupted', not 'running'.
        const interrupted = stale && p.state === 'running';
        const label = interrupted ? 'interrupted' : p.state === 'unverified' ? 'unverified' : p.state;
        const chip = interrupted ? CALL_PENDING : TODO_COLOR[p.state];
        return (
          <div key={p.key}>
            <button
              onClick={() => setOpen((o) => ({ ...o, [p.key]: !(o[p.key] ?? p.active) }))}
              className="w-full flex items-center gap-2 px-3 py-1 hover:bg-background-secondary transition-colors text-left"
            >
              {isOpen ? (
                <ChevronDown className="h-3 w-3 text-text-secondary shrink-0" />
              ) : (
                <ChevronRight className="h-3 w-3 text-text-secondary shrink-0" />
              )}
              <span
                className={`text-[11px] font-medium ${p.active && !interrupted ? '' : 'text-text-secondary'}`}
                style={p.active && !interrupted ? { color: chip } : undefined}
              >
                {p.label}
              </span>
              <span
                className="text-[9px] uppercase tracking-wide px-1 py-px shrink-0"
                style={{ color: chip, border: `1px solid ${chip}`, borderRadius: 2 }}
              >
                {label}
              </span>
              {p.counts.total > 0 ? (
                <span className="text-[10px] tabular-nums text-text-secondary ml-auto">
                  {p.counts.done}/{p.counts.total}
                </span>
              ) : null}
            </button>
            {isOpen ? (
              <div className="px-3 pb-2 pl-8 space-y-0">
                {p.items.map((item) => (
                  <PhaseTodoRow key={item.id} item={item} deviceOrder={deviceOrder} stale={stale} />
                ))}
              </div>
            ) : null}
          </div>
        );
      })}
    </div>
  );
};

// When the planner's confidence is below the ask floor, the swarm BLOCKS and asks the user. This prompt is
// the interactive answer surface: the user types answers and we write them to the handshake file, which
// unblocks the run. Amber (solid, not faded) because the build is PAUSED waiting on the human.
const ClarifyPrompt: React.FC<{
  clarify: {
    pending: boolean;
    questions: Array<{ question: string; options: string[]; rationale?: string; resolves?: string }>;
    planConfidence?: number;
    confidence?: ConfidenceBreakdown | null;
    answerPath: string;
  };
  plan: PlanTask[];
  /** The engine's confidence floor — this prompt only exists BECAUSE the plan scored under it, so the
   *  breakdown must name the real bar rather than judge the number against an invented band. */
  askFloor?: number | null;
}> = ({ clarify, plan, askFloor = null }) => {
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
          <span className="text-[10px] opacity-90 tabular-nums">
            planner confidence {clarify.planConfidence}/100
          </span>
        ) : null}
      </div>
      <div className="px-3 py-3 space-y-3 bg-background-secondary">
        <p className="text-xs text-text-secondary">
          Goose drafted this plan but wants your call on a few things before it builds. Pick an option, type
          your own, or just tell it what to change — it folds your input into the build.
        </p>

        {clarify.confidence ? (
          <div className="border border-border-primary px-2 py-2" style={{ borderRadius: 3 }}>
            <ConfidenceBreakdownBody
              conf={clarify.confidence}
              hasPendingQuestions
              askFloor={askFloor}
            />
          </div>
        ) : null}

        {plan.length > 0 ? (
          <div className="border border-border-primary" style={{ borderRadius: 2 }}>
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
                  <li key={t.id} className="text-[11px] text-text-primary flex gap-1.5">
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
                          ? { backgroundColor: BLUE, borderColor: BLUE, borderRadius: 2 }
                          : { borderRadius: 2 }
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
              style={{ borderRadius: 2 }}
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
            style={{ borderRadius: 2 }}
          />
        </div>

        {error ? (
          <div className="text-xs" style={{ color: STATUS_COLOR.error }}>
            Couldn&apos;t write the answers file — check that the build directory is still there, then retry.
          </div>
        ) : null}

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={send}
            disabled={busy || !canSend}
            className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 text-white disabled:opacity-50 transition-opacity"
            style={{ backgroundColor: BLUE, borderRadius: 2 }}
          >
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Send className="h-3.5 w-3.5" />}
            Send answers &amp; build
          </button>
          <span className="text-[10px] text-text-secondary">
            The build is paused until you respond. Your answers guide every worker; the plan shape stays as drafted.
          </span>
        </div>
      </div>
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
}> = ({ overview, phaseTodo, deviceOrder }) => {
  const verifyItems = phaseTodo.find((p) => p.key === 'verify')?.items ?? [];
  const verified = verifyItems.find((i) => i.id === 'v-e2e')?.state === 'done';
  const hdr = 'text-[10px] uppercase tracking-wide text-text-secondary mb-1 mt-2';
  const slate = '#5b8db8';
  return (
    <div className="border-t border-border-primary px-3 py-3 bg-background-secondary space-y-1">
      <div className="flex items-center gap-1.5 text-xs font-semibold text-text-primary">
        <ListChecks className="h-3.5 w-3.5" /> Build overview
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
          style={{ backgroundColor: AMBER, borderRadius: 3 }}
        >
          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
          Not yet verified — the program was built but never run. Everything below describes the code, not
          proof it works.
        </div>
      ) : !overview.generated ? (
        <div
          className="mt-1 px-2 py-1.5 text-[11px] flex items-center gap-1.5 border border-border-primary text-text-secondary"
          style={{ borderRadius: 3 }}
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
                  <li key={i} className="text-[11px] text-text-primary flex gap-1.5">
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
                  style={{ borderRadius: 2 }}
                >
                  {overview.runCommand}
                </code>
                <span
                  className="text-[9px] uppercase tracking-wide px-1 py-px"
                  style={{
                    color: overview.runCommandVerified ? STATUS_COLOR.done : slate,
                    border: `1px solid ${overview.runCommandVerified ? STATUS_COLOR.done : slate}`,
                    borderRadius: 2,
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
              <li key={i} className="text-[11px] text-text-primary flex gap-1.5">
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
        <span className="text-[11px] opacity-90 tabular-nums">{parts.join(' · ')}</span>
      </div>
      {outcome === 'stopped' ? (
        <div className="px-3 py-1.5 text-[11px] text-text-secondary bg-background-secondary">
          It ended without a completion signal — stopped or crashed mid-build. What finished is shown below.
        </div>
      ) : null}
      {summary && summary.perDevice.length > 0 ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-1.5 text-[11px] bg-background-secondary border-t border-border-primary/50">
          {summary.perDevice.map((d) => {
            const hue = FORMATION_RAMP[deviceIndex(d.device, deviceOrder) % FORMATION_RAMP.length];
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
          <div className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] text-text-secondary bg-background-secondary border-t border-border-primary/50 min-w-0 cursor-default">
            <FolderOpen className="h-3 w-3 shrink-0" />
            <span className="font-mono truncate">{outputDir}</span>
          </div>
        </Tip>
      ) : null}
    </div>
  );
};

export const SwarmRunPanel: React.FC<{ workingDir: string | undefined; className?: string }> = ({
  workingDir,
  className = '',
}) => {
  const run = useSwarmRun(workingDir);
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});
  // Default OPEN so the confidence gauge shows on EVERY build, not only when the badge is clicked or an ask
  // fires (Mihai: "I would like to see it for every app"). The badge still collapses it.
  const [confOpen, setConfOpen] = useState(true);
  const [mode, setMode] = useSwarmLogMode();
  const verbose = mode !== 'compact';
  const dev = mode === 'developer';
  const nextMode: Record<SwarmLogMode, SwarmLogMode> = {
    compact: 'verbose',
    verbose: 'developer',
    developer: 'compact',
  };

  // Show whenever a run is present — including the PLANNING phase, before any worker executes (no lanes yet).
  if (!run.present) return null;

  // Stable node identity: run.lanes RE-SORTS every poll (running first, then recency), so deriving letters
  // from first-seen lane order made a node's letter/hue flicker between polls. Sort the distinct devices
  // deterministically so ⬢A/hue is fixed for the whole run.
  const deviceOrder: string[] = Array.from(
    new Set([...run.lanes, ...run.planLanes].map((l) => l.device))
  ).sort();

  // Liveness: prefer the engine heartbeat (fast, precise) when the run has one; otherwise fall back to the
  // last-activity mtime with the old conservative window (runs that predate heartbeats).
  const stale =
    run.heartbeat != null
      ? Date.now() - run.heartbeat > HEARTBEAT_STALE_MS
      : run.mtime != null && Date.now() - run.mtime > STALE_MS;
  const { running, done, failed, tasks } = run.totals;

  // A run is OVER when it cleanly finished (run_finished) OR it went quiet with tasks in flight (killed /
  // crashed) — but NOT when it is merely paused waiting on the user's clarify answer. This drives the
  // terminal banner so an ended run never sits in the old limbo (green tasks, no "running", no "done").
  const clarifyPending = !!run.clarify?.pending;
  // A present run that has gone stale is over, regardless of how far it got — a run KILLED DURING PLANNING has
  // zero dispatched tasks, so the old `stale && tasks > 0` gate left it stuck showing "planning" forever. The
  // heartbeat makes `stale` precise (a live planner keeps ticking), so staleness alone is a safe end signal.
  const ended = !clarifyPending && (run.finished || stale);
  // The APP-LEVEL oracle: the engine's own end-to-end verify (complete_result -> phaseTodo v-e2e = 'done').
  // A green verify means the deliverable WORKS — so the run is 'done' and the overview shows — EVEN IF an
  // individual build task failed (e.g. the integrate-verify sink stalled but the orchestrator's verify still
  // passed). Without this, one failed task forced outcome='failed', which suppressed the overview and headlined
  // "1 task failed" on a working, verified app. The failed task stays visible in the Build phase-todo + counts.
  const appVerified =
    (run.phaseTodo.find((p) => p.key === 'verify')?.items ?? []).find((i) => i.id === 'v-e2e')?.state === 'done';
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

  return (
    <div
      data-testid="swarm-run-panel"
      className={`border border-border-primary bg-background-secondary text-text-primary text-sm ${className}`}
      style={{ borderRadius: 3 }}
    >
      <div className="flex items-center justify-between px-3 py-2 border-b border-border-primary gap-2">
        <span className="flex items-center gap-2 min-w-0">
          <span className="text-xs font-semibold shrink-0">Swarm LeanZero</span>
          {typeof run.planConfidence === 'number' ? (
            <ConfidenceBadge
              value={run.planConfidence}
              trail={run.confidenceTrail}
              expanded={confOpen}
              onToggle={() => setConfOpen((o) => !o)}
              hasBreakdown={!!run.confidence}
              askFloor={run.askFloor}
            />
          ) : null}
          {clarifyPending ? (
            // Paused waiting on the human — NOT active work, so no spinner and a distinct amber "paused" chip
            // (the old code showed a spinning "Planning" here, implying it was still churning).
            <Tip label="The build is paused, waiting for your answers in the prompt below.">
              <span
                className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
                style={{ backgroundColor: AMBER, borderRadius: 2 }}
              >
                <MessageCircleQuestion size={10} /> Waiting for you
              </span>
            </Tip>
          ) : run.inProgress && !stale && !ended && run.phase ? (
            <Tip label={`Current phase: ${run.phase}`}>
              <span
                className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
                style={{ backgroundColor: STATUS_COLOR.running, borderRadius: 2 }}
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
          <button
            onClick={() => setMode(nextMode[mode])}
            className="flex items-center gap-1 text-[10px] px-1.5 py-0.5 border transition-colors capitalize"
            style={
              dev
                ? { borderRadius: 2, borderColor: '#b14cff', color: '#b14cff' }
                : verbose
                  ? { borderRadius: 2, borderColor: '#2e8bff', color: '#2e8bff' }
                  : { borderRadius: 2, borderColor: 'var(--border-primary)', color: 'var(--text-secondary)' }
            }
            title={
              dev
                ? 'Developer — everything expanded + raw. Click for Compact.'
                : verbose
                  ? 'Verbose — the full timeline. Click for Developer.'
                  : 'Compact — headlines only. Click for Verbose.'
            }
          >
            <AlignLeft size={11} /> {mode}
          </button>
        </span>
      </div>

      {confOpen && run.confidence && !run.clarify?.pending ? (
        <ConfidencePanel
          conf={run.confidence}
          trail={run.confidenceTrail}
          hasPendingQuestions={false}
          askFloor={run.askFloor}
        />
      ) : null}

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
        <RunOverview overview={run.overview} phaseTodo={run.phaseTodo} deviceOrder={deviceOrder} />
      ) : null}

      {run.clarify?.pending ? (
        <ClarifyPrompt clarify={run.clarify} plan={run.plan} askFloor={run.askFloor} />
      ) : null}

      {(run.inProgress || ended) && (
        <PhaseSteps
          phase={run.phase}
          live={run.inProgress && !stale && !ended && !clarifyPending}
          activeColor={
            ended
              ? outcome === 'done'
                ? STATUS_COLOR.done
                : outcome === 'failed'
                  ? STATUS_COLOR.error
                  : STOPPED
              : STATUS_COLOR.running
          }
        />
      )}

      <PhaseTodoList phases={run.phaseTodo} deviceOrder={deviceOrder} stale={stale} />

      <ActivityFeed
        items={verbose ? run.verboseActivity : run.activity}
        live={run.inProgress && !stale && !ended}
        verbose={verbose}
      />

      {run.planLanes.length > 0 ? (
        <div className="border-t border-border-primary">
          <div className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wide text-text-secondary flex items-center gap-1.5">
            <Braces className="h-3 w-3" style={{ color: '#b14cff' }} />
            Drafting the plan · {run.planLanes.length} model
            {run.planLanes.length === 1 ? '' : 's'}
            {run.planLanes.some((l) => l.status === 'running') ? ' · thinking…' : ''}
          </div>
          <div className="divide-y divide-border-primary">
            {run.planLanes.map((lane) => {
              // Plan drafts: expand while thinking, collapse once the plan is chosen. Mode controls density.
              const defaultOpen = lane.status === 'running';
              const open = overrides[lane.taskId] ?? defaultOpen;
              return (
                <LaneRow
                  key={lane.taskId}
                  lane={lane}
                  deviceOrder={deviceOrder}
                  stale={stale}
                  open={open}
                  mode={mode}
                  dev={dev}
                  onToggle={() =>
                    setOverrides((o) => ({ ...o, [lane.taskId]: !(o[lane.taskId] ?? defaultOpen) }))
                  }
                />
              );
            })}
          </div>
        </div>
      ) : null}

      <div className="divide-y divide-border-primary">
        {run.lanes.map((lane) => {
          // Regardless of mode: IN-PROGRESS lanes default OPEN (you watch the live work), FINISHED ones
          // default COLLAPSED (headline only — the history stays scannable). The mode controls the CONTENT
          // density of an open lane (compact = titles, developer = full reasoning), not whether it's open.
          // The user can still expand/collapse any lane via the override.
          const defaultOpen = lane.status === 'running';
          const open = overrides[lane.taskId] ?? defaultOpen;
          return (
            <LaneRow
              key={lane.taskId}
              lane={lane}
              deviceOrder={deviceOrder}
              stale={stale}
              open={open}
              mode={mode}
              dev={dev}
              onToggle={() => setOverrides((o) => ({ ...o, [lane.taskId]: !(o[lane.taskId] ?? defaultOpen) }))}
            />
          );
        })}
      </div>
    </div>
  );
};

export default SwarmRunPanel;
