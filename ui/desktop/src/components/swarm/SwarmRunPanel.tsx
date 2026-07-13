import React, { useState } from 'react';
import {
  Check, X, Loader2, CircleSlash, ChevronRight, ChevronDown, Wrench,
  Search, ListChecks, Play, FlaskConical, RotateCcw, Gavel, Eye, FileText, Cpu, AlignLeft,
  MessageCircleQuestion, Send, Gauge,
} from 'lucide-react';
import {
  useSwarmRun,
  type TurnStatus,
  type TurnLane,
  type SwarmCall,
  type ActivityItem,
  type PlanTask,
} from './useSwarmRun';
import { useSwarmLogMode, type SwarmLogMode } from './useVerboseSwarm';

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
const CALL_OK = '#2ecc71';
const CALL_ERR = '#ff3b30';
const CALL_PENDING = '#8a8a8a';
const AMBER = '#f5a623';
const BLUE = '#2e8bff';

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
  const hasBody = reasoning.length > 0 || calls.length > 0 || (lane.recent?.length ?? 0) > 0;
  // The first failing call auto-expands so the error is zero clicks away.
  const firstFailIdx = calls.findIndex((c) => c.ok === false);

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
        <span className="font-bold shrink-0" style={{ color: hue }} aria-label={`node ${letter}`}>
          ⬢{letter}
        </span>
        <span className="w-16 shrink-0 truncate text-text-secondary text-xs">{lane.device}</span>
        {/* Readable name: the architect's description ("Tokenize the template source") is the primary label;
            the terse id ("lexer") drops to a mono sub-tag so it's still identifiable but no longer cryptic. */}
        <span className="flex-1 min-w-0 flex flex-col leading-tight">
          <span className="truncate text-xs text-text-primary">{lane.description || lane.taskId}</span>
          {lane.description ? (
            <span className="truncate text-[10px] font-mono text-text-secondary">{lane.taskId}</span>
          ) : null}
        </span>
        {lane.model && (
          <span className="hidden sm:inline shrink-0 max-w-[9rem] truncate text-[10px] font-mono text-text-secondary">
            {lane.model}
          </span>
        )}
        <span className="text-xs text-text-secondary tabular-nums shrink-0 flex items-center gap-1">
          {typeof lane.toolCalls === 'number' && lane.toolCalls > 0 ? (
            <span className="flex items-center gap-0.5">
              <Wrench className="h-3 w-3" />
              {lane.toolCalls}
            </span>
          ) : null}
          {lane.errors ? <span style={{ color: STATUS_COLOR.error }}>{lane.errors}✕</span> : null}
          {typeof lane.elapsedMs === 'number' ? <span>{Math.round(lane.elapsedMs / 1000)}s</span> : null}
        </span>
        <Icon
          size={15}
          strokeWidth={3}
          className={`shrink-0 ${live ? 'animate-spin' : ''}`}
          style={{ color: iconColor }}
        />
      </button>

      {open && hasBody && (
        <div className="px-3 pb-3 pl-9 space-y-2">
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

// How sure the planner was about HOW to break this app down, from cross-draft agreement (not the model's
// self-report). Solid green ≥70 (confident), amber 40-69 (unsure), red <40 (guessing) — so a low number is a
// visible flag that goose was uncertain, which is exactly when the ask gate fires.
const ConfidenceBadge: React.FC<{ value: number }> = ({ value }) => {
  const color = value >= 70 ? STATUS_COLOR.done : value >= 40 ? AMBER : STATUS_COLOR.error;
  const label = value >= 70 ? 'confident' : value >= 40 ? 'unsure' : 'guessing';
  return (
    <span
      className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium tabular-nums"
      style={{ backgroundColor: color, borderRadius: 2 }}
      title={`Planner confidence in how it decomposed this app (cross-draft agreement): ${value}/100 — ${label}.`}
    >
      <Gauge className="h-2.5 w-2.5" />
      {value}
    </span>
  );
};

// The fixed pipeline every build moves through, so the free-text `phase` label reads as PROGRESS, not a
// mystery. The active step is filled; passed steps get a check; upcoming steps stay muted.
const PHASE_STEPS = ['Research', 'Plan', 'Contracts', 'Build', 'Verify', 'Done'] as const;
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
const PhaseSteps: React.FC<{ phase: string }> = ({ phase }) => {
  const active = phaseStepIndex(phase);
  return (
    <div className="flex items-center gap-1 px-3 py-1.5 border-b border-border-primary overflow-x-auto">
      {PHASE_STEPS.map((step, i) => (
        <React.Fragment key={step}>
          {i > 0 && <span className="text-text-secondary text-[10px] shrink-0">›</span>}
          <span
            className={`text-[10px] px-1.5 py-0.5 whitespace-nowrap shrink-0 ${
              i === active
                ? 'text-white font-semibold'
                : i < active
                  ? 'text-text-secondary'
                  : 'text-text-secondary opacity-60'
            }`}
            style={i === active ? { backgroundColor: STATUS_COLOR.running, borderRadius: 2 } : undefined}
          >
            {i < active ? '✓ ' : ''}
            {step}
          </span>
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
    questions: Array<{ question: string; options: string[] }>;
    planConfidence?: number;
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

export const SwarmRunPanel: React.FC<{ workingDir: string | undefined; className?: string }> = ({
  workingDir,
  className = '',
}) => {
  const run = useSwarmRun(workingDir);
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});
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

  // Deterministic node ordering (first-seen device order) for stable letters/hues.
  const deviceOrder: string[] = [];
  for (const l of run.lanes) if (!deviceOrder.includes(l.device)) deviceOrder.push(l.device);

  const stale = run.mtime != null && Date.now() - run.mtime > STALE_MS;
  const { running, done, failed, tasks } = run.totals;

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
            <ConfidenceBadge value={run.planConfidence} />
          ) : null}
          {run.inProgress && !stale && run.phase && (
            <span
              className="text-[10px] px-1.5 py-0.5 flex items-center gap-1 shrink-0 text-white font-medium"
              style={{ backgroundColor: STATUS_COLOR.running, borderRadius: 2 }}
            >
              <Loader2 size={10} className="animate-spin" /> {run.phase}
            </span>
          )}
          {stale && run.inProgress && (
            <span className="text-[10px] px-1.5 py-0.5 shrink-0" style={{ color: CALL_PENDING }}>
              interrupted
            </span>
          )}
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
            {ago(run.mtime)}
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

      {run.clarify?.pending ? <ClarifyPrompt clarify={run.clarify} plan={run.plan} /> : null}

      {run.inProgress && <PhaseSteps phase={run.phase} />}

      <ActivityFeed
        items={verbose ? run.verboseActivity : run.activity}
        live={run.inProgress && !stale}
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
