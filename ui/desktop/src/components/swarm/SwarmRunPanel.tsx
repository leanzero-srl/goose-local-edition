import React, { useState } from 'react';
import {
  Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench,
  Search, ListChecks, Play, FlaskConical, RotateCcw, Gavel, Eye, FileText, Cpu, AlignLeft,
  MessageCircleQuestion, Send, Gauge, AlertTriangle, FolderOpen, TrendingUp, Info,
} from 'lucide-react';
import {
  useSwarmRun,
  type TurnStatus,
  type TurnLane,
  type SwarmCall,
  type ActivityItem,
  type PlanTask,
  type RunSummary,
  type ConfidenceBreakdown,
} from './useSwarmRun';
import { useSwarmLogMode, type SwarmLogMode } from './useVerboseSwarm';
import { Tooltip, TooltipTrigger, TooltipContent } from '../ui/Tooltip';

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

function callColor(ok: boolean | null): string {
  if (ok === true) return CALL_OK;
  if (ok === false) return CALL_ERR;
  return CALL_PENDING;
}

// A machine-emitted block (shell stdout/stderr, a printed value) rendered TRUE MONOSPACE with alignment
// preserved, capped with a Show-all escape hatch (never truncate-and-lose) and a copy button.
const MonoOutput: React.FC<{ text: string; failed?: boolean }> = ({ text, failed }) => {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const lineCount = text.split('\n').length;
  const big = lineCount > 24;
  return (
    <div>
      <pre
        className={`font-mono text-[11px] whitespace-pre-wrap break-words px-2 py-1 bg-background-secondary border border-border-primary ${!expanded && big ? 'max-h-64 overflow-hidden' : ''}`}
        style={{ borderRadius: 2, color: failed ? '#ff8f88' : 'var(--text-secondary)' }}
      >
        {text}
      </pre>
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
// expand. A failing call auto-expands so the error is zero-clicks away.
const CallRow: React.FC<{ call: SwarmCall; defaultOpen?: boolean }> = ({ call, defaultOpen }) => {
  const hasOutput = !!call.result && call.result.trim().length > 0;
  const failed = call.ok === false;
  const [open, setOpen] = useState(defaultOpen ?? false);
  return (
    <div className="py-0.5 border-b border-border-primary/30 last:border-0">
      <button
        type="button"
        onClick={() => hasOutput && setOpen((o) => !o)}
        className={`w-full flex items-start gap-2 text-left ${hasOutput ? 'cursor-pointer hover:opacity-80' : 'cursor-default'}`}
      >
        <span
          className="mt-1 h-1.5 w-1.5 shrink-0"
          style={{ backgroundColor: callColor(call.ok), borderRadius: 1 }}
          aria-hidden
        />
        <span
          className="shrink-0 font-mono text-[10px] uppercase tracking-wide px-1 py-px text-text-secondary border border-border-primary"
          style={{ borderRadius: 2 }}
        >
          {call.name.replace(/^developer__/, '')}
        </span>
        <span
          className="flex-1 font-mono text-xs break-words"
          style={{ color: failed ? CALL_ERR : 'var(--text-primary)' }}
          title={call.summary}
        >
          {call.summary || '—'}
        </span>
        {hasOutput &&
          (open ? (
            <ChevronDown className="h-3 w-3 shrink-0 text-text-secondary mt-0.5" />
          ) : (
            <ChevronRight className="h-3 w-3 shrink-0 text-text-secondary mt-0.5" />
          ))}
      </button>
      {hasOutput && open && (
        <div className="ml-[1.15rem] mt-1">
          <MonoOutput text={call.result!.trim()} failed={failed} />
        </div>
      )}
    </div>
  );
};

// The worker's full narration, rendered as readable PROSE (not mono), capped with a Show-all escape hatch.
// In developer mode it starts fully expanded (no cap).
const ReasoningBlock: React.FC<{ text: string; forceOpen?: boolean }> = ({ text, forceOpen }) => {
  const [expandedState, setExpanded] = useState(false);
  const expanded = expandedState || !!forceOpen;
  const words = text.split(/\s+/).filter(Boolean).length;
  const big = text.length > 1200;
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wide text-text-secondary mb-1">Reasoning</div>
      <div
        className={`text-xs text-text-primary whitespace-pre-wrap break-words leading-relaxed bg-background-primary border border-border-primary px-2 py-1.5 ${!expanded && big ? 'max-h-[22rem] overflow-hidden' : ''}`}
        style={{ borderRadius: 3 }}
      >
        {text}
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
  dev?: boolean;
  onToggle: () => void;
}> = ({ lane, deviceOrder, stale, open, dev, onToggle }) => {
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
          {reasoning && <ReasoningBlock text={reasoning} forceOpen={dev || live} />}

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
            <div className="text-xs text-text-secondary font-mono break-words">{lane.recent.join(' · ')}</div>
          ) : null}
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
const confColor = (v: number): string =>
  v >= 70 ? STATUS_COLOR.done : v >= 40 ? AMBER : STATUS_COLOR.error;

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
const ConfGauge: React.FC<{ value: number; size?: number }> = ({ value, size = 76 }) => {
  const v = Math.max(0, Math.min(100, Math.round(value)));
  const r = 32;
  const circ = 2 * Math.PI * r;
  const sweep = 0.75; // 270°
  const track = sweep * circ;
  const fill = (v / 100) * sweep * circ;
  const col = confColor(v);
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
      <text
        x="40"
        y="55"
        textAnchor="middle"
        dominantBaseline="middle"
        style={{ fill: 'var(--text-secondary)', fontSize: 8, letterSpacing: 0.5 }}
      >
        /100
      </text>
    </svg>
  );
};

// Shared breakdown body — reused by the header-expand panel AND the ClarifyPrompt (one visual language). The
// min number, the two sub-bars, WHAT'S HOLDING IT BACK (the binding/lower signal), WHAT WOULD RAISE IT
// (honest, research-backed), and a climb trail/sparkline when the meter has moved.
const ConfidenceBreakdownBody: React.FC<{
  conf: ConfidenceBreakdown;
  trail?: number[];
  hasPendingQuestions: boolean;
}> = ({ conf, trail, hasPendingQuestions }) => {
  const bindingAgreement = conf.agreement <= conf.specClarity;
  const showDecisions = !bindingAgreement && conf.openDecisions.length > 0;
  const holdingBack = bindingAgreement
    ? conf.agreementReason || 'The planning drafts disagree on how to structure the build.'
    : conf.productSpecified
      ? 'Some requirements are still ambiguous.'
      : "The product itself isn't fully specified yet.";
  const raiseIt = bindingAgreement
    ? 'Goose re-drafts toward a single consensus plan to reconcile the structure — designed to converge, though a small/weak fleet may not fully agree.'
    : hasPendingQuestions
      ? 'Answer the questions below — each resolves an open decision. Goose can also research the undecided points.'
      : 'Researching the undecided points to firm up the spec.';
  return (
    <div className="space-y-2.5">
      <div className="flex items-center gap-3">
        <ConfGauge value={conf.final} />
        <div>
          <div className="text-[10px] uppercase tracking-wide text-text-secondary">Plan confidence</div>
          <div className="text-[12px] font-medium mt-0.5" style={{ color: confColor(conf.final) }}>
            {conf.final >= 70
              ? 'Strong — ready to build'
              : conf.final >= 40
                ? 'Mixed — check below'
                : 'Low — needs your input'}
          </div>
        </div>
      </div>
      <div className="space-y-1.5">
        <ConfBar label="Agreement" value={conf.agreement} />
        <ConfBar label="Spec clarity" value={conf.specClarity} />
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
}> = ({ value, trail, expanded, onToggle, hasBreakdown }) => {
  const color = confColor(value);
  const label = value >= 70 ? 'confident' : value >= 40 ? 'unsure' : 'guessing';
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
        label={`Planner confidence in how it broke this app down — ${value}/100 (${label}).${hasBreakdown ? ' Click for the breakdown.' : ''} Below the ask floor, goose pauses to ask you before building.`}
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
}> = ({ clarify, plan }) => {
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
        Sent — goose is re-planning with your answers.
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
          your own, or just tell it what to change — it re-plans with your input.
        </p>

        {clarify.confidence ? (
          <div className="border border-border-primary px-2 py-2" style={{ borderRadius: 3 }}>
            <ConfidenceBreakdownBody conf={clarify.confidence} hasPendingQuestions />
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
                    <span>{t.description || t.id}</span>
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        ) : null}

        {clarify.questions.map((q, i) => (
          <div key={i} className="space-y-1.5">
            <div className="text-xs text-text-primary font-medium">
              {i + 1}. {q.question}
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
            Send &amp; re-plan
          </button>
          <span className="text-[10px] text-text-secondary">The build is paused until you respond.</span>
        </div>
      </div>
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
  const deviceOrder: string[] = Array.from(new Set(run.lanes.map((l) => l.device))).sort();

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
  const outcome: 'done' | 'failed' | 'stopped' | null = !ended
    ? null
    : run.finished
      ? (run.summary?.failed ?? failed) > 0
        ? 'failed'
        : 'done'
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

      {run.clarify?.pending ? <ClarifyPrompt clarify={run.clarify} plan={run.plan} /> : null}

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

      <ActivityFeed
        items={verbose ? run.verboseActivity : run.activity}
        live={run.inProgress && !stale && !ended}
        verbose={verbose}
      />

      <div className="divide-y divide-border-primary">
        {run.lanes.map((lane) => {
          // In verbose, every lane defaults open (show all reasoning + tool calls); the user can still
          // collapse individual ones via the override.
          const defaultOpen = verbose || lane.status === 'running';
          const open = overrides[lane.taskId] ?? defaultOpen;
          return (
            <LaneRow
              key={lane.taskId}
              lane={lane}
              deviceOrder={deviceOrder}
              stale={stale}
              open={open}
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
