import { useEffect, useRef, useState } from 'react';

/**
 * Reads the LIVE swarm run for a working directory (via the `read-swarm-run` IPC) and folds its event
 * stream + per-turn activity digests into one lane PER TASK — the "turn loops" the user inspects. A
 * `goose swarm run` writes <cwd>/.swarm/run-<id>.jsonl (task_dispatched / task_completed / task_retry …)
 * plus <cwd>/.swarm/activity/<task>.json ({tool_calls, errors, recent[], last_text}); this turns that raw
 * data into what each node is DOING right now. Polls so an in-flight run animates.
 */

export type TurnStatus = 'running' | 'done' | 'error';

export interface SwarmCall {
  name: string;
  summary: string;
  ok: boolean | null;
  /** A snippet of what the call produced — test output, a traceback, a printed value. */
  result?: string;
}

export interface TurnLane {
  taskId: string;
  /** The architect's one-line human description of the subtask (e.g. "Tokenize the template source") — the
   *  readable name the panel shows, so a lane isn't just the terse id "lexer". */
  description?: string;
  device: string;
  model?: string;
  status: TurnStatus;
  lastText?: string;
  recent?: string[];
  reasoning?: string;
  /** The worker's full narration (all substantive text chunks) — the "reasoning in plain" the panel shows. */
  fullReasoning?: string;
  calls?: SwarmCall[];
  toolCalls?: number;
  errors?: number;
  elapsedMs?: number;
  /** How many attempts the task took (from task_completed) — surfaced in the status tooltip. */
  attempts?: number;
  /** The last retry's failure text, so a failed/interrupted lane can say WHY (was silently dropped before). */
  error?: string;
  seq: number;
}

export interface SwarmRunTotals {
  tasks: number;
  running: number;
  done: number;
  failed: number;
}

// A per-phase TODO checklist, derived ENTIRELY from the engine's deterministic event stream (scheduler task
// states + orchestrator phase events) — never from a model self-reporting. The honesty hinges on TodoState:
// a built task is 'unverified' (the worker's loop returned + passed a syntax gate; the app was NOT run), and
// only Verify's complete_result.passed&&verified earns a green 'done'. Advisory items (LLM reviewers) are
// info, never checks. See the phase-todo design workflow (2026-07-15).
export type PhaseKey = 'research' | 'plan' | 'contracts' | 'build' | 'verify' | 'done';
export type TodoState =
  | 'pending'
  | 'running'
  | 'done' // class-A VERIFIED complete (green)
  | 'unverified' // built/shipped but NOT proven to run — neutral, never green
  | 'failed' // engine hard-fail or terminal block
  | 'judge_failed' // a task the JUDGE LLM decided to fail (not the engine)
  | 'blocked' // never dispatched but doomed (cascade dep-fail / scheduler stuck)
  | 'skipped' // phase legitimately no-op'd (research off / gate off)
  | 'advisory'; // LLM-reviewer / heuristic signal — info only, never a checkbox
export interface PhaseTodoItem {
  id: string;
  label: string;
  state: TodoState;
  detail?: string;
  device?: string;
  advisory?: boolean;
}
export interface PhaseTodo {
  key: PhaseKey;
  label: string;
  items: PhaseTodoItem[];
  state: TodoState;
  active: boolean;
  counts: { done: number; total: number };
}

export type ActivityKind =
  | 'phase'
  | 'plan'
  | 'dispatch'
  | 'done'
  | 'fail'
  | 'retry'
  | 'retarget'
  | 'review'
  | 'judge'
  | 'prereview'
  | 'smoke'
  | 'brief'
  | 'config';
export type ActivityTone = 'info' | 'good' | 'warn' | 'bad';
export interface ActivityItem {
  kind: ActivityKind;
  text: string;
  sub?: string;
  tone?: ActivityTone;
  seq: number;
}

export interface PlanTask {
  id: string;
  description?: string;
  files: string[];
  deps: string[];
  difficulty: string;
}
export interface RunMeta {
  prompt: string;
  plannerModel: string;
  endpoint: string;
  nodes: string[];
  gates: boolean;
}
export interface SmokeResult {
  ran: boolean;
  testsPass: boolean | null;
  entryOk: boolean | null;
  pyFiles: number;
}
/** Per-node aggregate for the completion summary — how much each fleet node actually did. */
export interface DeviceStat {
  node: string;
  device: string;
  dispatched: number;
  toolCalls: number;
  busyMs: number;
}
/** The end-of-run tally from run_finished — done/failed task counts, total wall-clock minutes, and the
 *  per-node breakdown. Present only after a CLEAN finish; a run that died without run_finished has none (the
 *  panel falls back to counting lanes + wall time from startedAt). */
export interface RunSummary {
  done: number;
  failed: number;
  totalMin: number | null;
  perDevice: DeviceStat[];
}

/** The full plan-confidence breakdown behind the single number, so the panel can show WHICH signal is low
 *  (agreement = do the drafts agree on structure; spec-clarity = is the product pinned down) and what would
 *  raise it. Parsed from the additive `plan_confidence_breakdown` event key; null on runs that predate it. */
export interface ConfidenceBreakdown {
  final: number;
  agreement: number;
  agreementReason: string;
  specClarity: number;
  specClarityReason: string;
  productSpecified: boolean;
  openDecisions: string[];
}

// End-of-run overview (shown at DONE). features/engage/next are a GROUNDED, scrubbed summary from the model;
// runCommand is engine-stamped (never the model). VERIFICATION is NOT here — the panel re-derives it from
// phaseTodo (engine-only) so no model string can reach the honesty surface. generated=false => a red/failed
// or skipped build: show the caveat, not the summary.
export interface RunOverview {
  generated: boolean;
  runCommand: string | null;
  runCommandLang: string | null;
  runCommandVerified: boolean;
  features: string[];
  engage: string | null;
  next: string[];
}

export interface SwarmRunState {
  present: boolean;
  runId: string | null;
  lanes: TurnLane[];
  /** PLAN-phase generation lanes (architect drafts) — what each model produced while decomposing the app.
   *  Separate from `lanes` (build tasks) and excluded from `totals`. */
  planLanes: TurnLane[];
  /** Per-phase TODO checklist, derived entirely from the engine's deterministic events (see buildPhaseTodo). */
  phaseTodo: PhaseTodo[];
  /** End-of-run overview (what built / how to run / next) — null until the run cleanly finishes at DONE. */
  overview: RunOverview | null;
  totals: SwarmRunTotals;
  /** A human-readable timeline of what the swarm is doing — shown even during PLANNING (before any worker
   *  executes), so the user isn't left staring at a blank "working on it". */
  activity: ActivityItem[];
  /** The FULL timeline — every dispatch, judge verdict + hint, pre-review, completion, smoke result — for
   *  the verbose view. The compact `activity` is a subset of headline phases. */
  verboseActivity: ActivityItem[];
  /** Run header detail (the brief, planner model, nodes, gates) for the verbose view. */
  meta: RunMeta | null;
  /** The planned task graph (files/deps/difficulty per task) for the verbose view. */
  plan: PlanTask[];
  /** End-to-end smoke-test result, once the run reaches verification. */
  smoke: SmokeResult | null;
  /** Friendly current-phase label (Planning research / Building / Verifying / Done…). */
  phase: string;
  /** Cross-draft-agreement plan confidence (0-100) — how sure the planner was about the decomposition.
   *  null before planning finishes / when not computed. Updates live as the swarm retargets to raise it. */
  planConfidence: number | null;
  /** The full breakdown behind planConfidence (agreement vs spec-clarity + drivers), null on older runs. */
  confidence: ConfidenceBreakdown | null;
  /** Distinct successive confidence values across the run (initial → each retarget → final) for the climb
   *  trail / sparkline. */
  confidenceTrail: number[];
  /** True while a run is underway (started, not finished, and its files are still fresh). */
  inProgress: boolean;
  /** True once a clean run_finished event was seen — distinguishes a real completion from a run that just went
   *  quiet (killed/crashed), which the panel renders as "Stopped" rather than "Done". */
  finished: boolean;
  /** End-of-run tally, present only after a clean finish (see RunSummary). */
  summary: RunSummary | null;
  /** Epoch ms of the run's first event, for computing wall time when there is no clean-finish total. */
  startedAt: number | null;
  /** Set when the planner's confidence is below the ask floor and the swarm is BLOCKED waiting for the user
   *  to answer clarifying questions (via the run panel's clarify prompt, written to answerPath). */
  clarify: {
    pending: boolean;
    questions: Array<{ question: string; options: string[]; rationale?: string; resolves?: string }>;
    planConfidence?: number;
    confidence?: ConfidenceBreakdown | null;
    answerPath: string;
  } | null;
  mtime: number | null;
  /** Epoch ms of the engine's last liveness heartbeat (touched every ~5s while running), or null for a run
   *  with no heartbeat file. Lets the panel detect a killed engine in seconds without false "stopped" during
   *  a legitimately long tool call (which leaves task files quiet but keeps the heartbeat ticking). */
  heartbeat: number | null;
  loading: boolean;
}

const EMPTY: SwarmRunState = {
  present: false,
  runId: null,
  lanes: [],
  planLanes: [],
  phaseTodo: [],
  overview: null,
  totals: { tasks: 0, running: 0, done: 0, failed: 0 },
  activity: [],
  verboseActivity: [],
  meta: null,
  plan: [],
  smoke: null,
  phase: '',
  planConfidence: null,
  confidence: null,
  confidenceTrail: [],
  inProgress: false,
  finished: false,
  summary: null,
  startedAt: null,
  clarify: null,
  mtime: null,
  heartbeat: null,
  loading: true,
};

/** Short node name from a device id like 'mac-gabee-qwopus3.6-27b-coder-ml-2' -> 'gabee'. */
function nodeName(device: string): string {
  const bare = device.replace(/-qwopus.*$/i, '');
  const parts = bare.split('-').filter(Boolean);
  return parts[parts.length - 1] || device;
}

const num = (v: unknown): number | null => (typeof v === 'number' ? v : null);
const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);

/** Parse the additive `plan_confidence_breakdown` object (agreement/spec-clarity split + drivers). Returns
 *  null on older runs where the key is absent — the panel then just shows the scalar. */
function parseConfidence(v: unknown): ConfidenceBreakdown | null {
  if (!v || typeof v !== 'object') return null;
  const o = v as Record<string, unknown>;
  const agreement = num(o['agreement']);
  const specClarity = num(o['spec_clarity']);
  const final =
    num(o['final']) ??
    (agreement != null && specClarity != null ? Math.min(agreement, specClarity) : null);
  if (final == null) return null;
  return {
    final,
    agreement: agreement ?? final,
    agreementReason: str(o['agreement_reason']),
    specClarity: specClarity ?? final,
    specClarityReason: str(o['spec_clarity_reason']),
    productSpecified: o['product_specified'] === true,
    openDecisions: arr(o['open_decisions']).map(String),
  };
}

/** The build prompt is wrapped by goose's <turn-context>…</turn-context> preamble (time, cwd, todo
 *  boilerplate). Strip it so the Brief shows just the user's actual spec. */
function cleanBrief(prompt: string): string {
  return prompt.replace(/^[\s\S]*?<\/turn-context>\s*/i, '').trim() || prompt.trim();
}

/** The judge's verdict maps to a tone: ok is good; a named failure mode (spec_drift/over_reading/looping…)
 *  is a warning the user should see. */
function judgeTone(verdict: string): ActivityTone {
  return verdict === 'ok' || verdict === '' ? 'good' : 'warn';
}

/** Turn the run event stream into TWO timelines — a compact headline feed (phases only) and a VERBOSE feed
 *  (every dispatch, judge verdict + hint, pre-review, completion, smoke) — plus the run header meta, the
 *  planned task graph, and the smoke result. All of this is already in the event stream; nothing is dropped
 *  server-side, so verbose mode is purely a richer read of the same data. */
function buildActivity(events: Array<Record<string, unknown>>): {
  activity: ActivityItem[];
  verbose: ActivityItem[];
  meta: RunMeta | null;
  plan: PlanTask[];
  smoke: SmokeResult | null;
  phase: string;
  finished: boolean;
  planConfidence: number | null;
  confidence: ConfidenceBreakdown | null;
  confidenceTrail: number[];
  summary: RunSummary | null;
  startedAt: number | null;
  overview: RunOverview | null;
} {
  const feed: ActivityItem[] = [];
  const vfeed: ActivityItem[] = [];
  let phase = 'Starting…';
  let finished = false;
  let cseq = 0;
  let vseq = 0;
  let meta: RunMeta | null = null;
  let plan: PlanTask[] = [];
  let planConfidence: number | null = null;
  let confidence: ConfidenceBreakdown | null = null;
  const confTrail: number[] = [];
  let smoke: SmokeResult | null = null;
  let summary: RunSummary | null = null;
  let startedAt: number | null = null;
  let overview: RunOverview | null = null;
  const compact = (it: Omit<ActivityItem, 'seq'>) => feed.push({ ...it, seq: cseq++ });
  const verbose = (it: Omit<ActivityItem, 'seq'>) => vfeed.push({ ...it, seq: vseq++ });
  // Push each distinct confidence value onto the trail (initial → retargets → final) and set the live
  // header value; last-write-wins on each poll makes the pill advance without ref plumbing.
  const setConf = (v: number) => {
    if (confTrail[confTrail.length - 1] !== v) confTrail.push(v);
    planConfidence = v;
  };

  for (const e of events) {
    const type = String(e['event'] ?? '');
    // First event with a parseable timestamp anchors the run's start, so the terminal summary can show wall
    // time even for a run that died without a run_finished (no phases.total_min to read).
    if (startedAt == null && typeof e['ts'] === 'string') {
      const ms = Date.parse(e['ts'] as string);
      if (!Number.isNaN(ms)) startedAt = ms;
    }
    switch (type) {
      case 'run_started': {
        const pool = arr(e['pool']).map((d) => nodeName(str((d as Record<string, unknown>)['id'])));
        // `gates` is now an OBJECT of per-gate booleans, so `!!e['gates']` was always true. Treat gates as on
        // when the assured bundle is on or ANY individual gate is enabled.
        const gatesVal = e['gates'];
        const anyGate =
          typeof gatesVal === 'object' && gatesVal !== null
            ? Object.values(gatesVal as Record<string, unknown>).some((v) => v === true)
            : !!gatesVal;
        meta = {
          prompt: cleanBrief(str(e['prompt'])),
          plannerModel: str(e['planner_model']),
          endpoint: str(e['endpoint']),
          nodes: pool,
          gates: anyGate || !!e['assured'],
        };
        compact({ kind: 'phase', text: 'Starting the build' });
        verbose({ kind: 'phase', text: 'Starting the build' });
        if (meta.prompt) verbose({ kind: 'brief', text: 'Brief', sub: meta.prompt, tone: 'info' });
        verbose({
          kind: 'config',
          text: `Fleet: ${pool.length} node${pool.length === 1 ? '' : 's'}${pool.length ? ' — ' + pool.join(', ') : ''}`,
          sub: `planner ${nodeName(meta.plannerModel)}${meta.gates ? ' · gates on' : ''}`,
          tone: 'info',
        });
        phase = 'Starting';
        break;
      }
      case 'scouts_planned': {
        const lenses = arr(e['lenses']).map(String).join(', ');
        compact({ kind: 'phase', text: 'Planning research', sub: lenses || undefined });
        verbose({ kind: 'phase', text: 'Planning research', sub: lenses || undefined });
        phase = 'Planning research';
        break;
      }
      case 'research_planned':
        verbose({ kind: 'phase', text: 'Researching the problem' });
        phase = 'Researching';
        break;
      case 'research_completed': {
        const t = `Research done — ${num(e['findings']) ?? 0} findings`;
        compact({ kind: 'phase', text: t });
        verbose({ kind: 'phase', text: t });
        phase = 'Planning';
        break;
      }
      case 'pillars':
        verbose({ kind: 'phase', text: 'Defining quality pillars' });
        phase = 'Planning';
        break;
      case 'low_confidence_ask': {
        // Surface the confidence AS SOON AS the swarm asks (before plan_loaded), so the badge is visible at
        // the exact moment it pauses for the user — not only after the re-plan.
        const b = parseConfidence(e['plan_confidence_breakdown']);
        if (b) confidence = b;
        const pc = num(e['plan_confidence']);
        if (typeof pc === 'number') setConf(pc);
        const nq = arr(e['questions']).length;
        compact({
          kind: 'plan',
          text: `Paused — asking you ${nq} question${nq === 1 ? '' : 's'}`,
          tone: 'warn',
        });
        verbose({
          kind: 'plan',
          text: `Low confidence (${typeof pc === 'number' ? pc : '?'}/100) — asking the user ${nq} clarifying question${nq === 1 ? '' : 's'} before building`,
          tone: 'warn',
        });
        break;
      }
      case 'low_confidence_answered': {
        compact({ kind: 'plan', text: 'Got your answers — re-planning', tone: 'good' });
        verbose({ kind: 'plan', text: 'Answers received — re-planning with your input', tone: 'good' });
        break;
      }
      case 'confidence_retarget': {
        // The swarm is dynamically raising the meter (re-drafting to a consensus / researching the open
        // decisions). conf_after may be null (the new value lands on the next plan step) — either way we log
        // the action; the header pill climbs via setConf on this or the subsequent plan event.
        const before = num(e['conf_before']);
        const after = num(e['conf_after']);
        const signal = str(e['binding_signal']);
        const action = str(e['action']);
        const round = num(e['round']);
        const detail = str(e['detail']);
        if (typeof after === 'number') {
          setConf(after);
          if (confidence) {
            const next: ConfidenceBreakdown = { ...confidence };
            if (signal === 'agreement') next.agreement = after;
            else if (signal === 'spec_clarity') next.specClarity = after;
            next.final = Math.min(next.agreement, next.specClarity);
            confidence = next;
          }
        }
        const label =
          signal === 'spec_clarity' ? 'spec clarity' : signal === 'agreement' ? 'agreement' : 'confidence';
        const actionLabel =
          action === 'redraft'
            ? 're-drafting a consensus plan'
            : action === 're_research'
              ? 'researching the open decisions'
              : action === 'proceed_at_cap'
                ? 'proceeding at the round cap'
                : action || 'improving';
        const climbed = before != null && after != null && after > before;
        const sub =
          [
            before != null && after != null
              ? `${label} ${before} → ${after}`
              : before != null
                ? `${label} at ${before}`
                : null,
            detail || null,
          ]
            .filter(Boolean)
            .join(' · ') || undefined;
        compact({
          kind: 'retarget',
          text: `Retargeting confidence: ${actionLabel}`,
          tone: climbed ? 'good' : 'info',
          sub,
        });
        verbose({
          kind: 'retarget',
          text: `Retargeting confidence: ${actionLabel}${round != null ? ` (round ${round})` : ''}`,
          tone: climbed ? 'good' : 'info',
          sub,
        });
        break;
      }
      case 'plan_loaded': {
        const b = parseConfidence(e['plan_confidence_breakdown']);
        if (b) confidence = b;
        const pc = num(e['plan_confidence']);
        if (typeof pc === 'number') setConf(pc);
        const tasks = arr(e['tasks']) as Array<Record<string, unknown>>;
        plan = tasks.map((t) => ({
          id: str(t['id']),
          description: t['description'] ? str(t['description']) : undefined,
          files: arr(t['files']).map(String),
          deps: arr(t['deps']).map(String),
          difficulty: str(t['difficulty']),
        }));
        const n = num(e['task_count']) ?? tasks.length;
        compact({ kind: 'plan', text: `Plan ready — ${n} task${n === 1 ? '' : 's'}`, sub: plan.map((t) => t.id).join(', ') });
        verbose({ kind: 'plan', text: `Plan ready — ${n} task${n === 1 ? '' : 's'}`, tone: 'info' });
        for (const t of plan) {
          const bits = [t.difficulty && `${t.difficulty}`, t.deps.length && `after ${t.deps.join(', ')}`, t.files.length && t.files.join(', ')]
            .filter(Boolean)
            .join(' · ');
          verbose({ kind: 'plan', text: `· ${t.id}`, sub: bits || undefined });
        }
        phase = 'Building';
        break;
      }
      case 'task_dispatched': {
        const task = str(e['task_id']);
        const node = e['device'] ? nodeName(str(e['device'])) : '';
        const attempt = num(e['attempt']) ?? 0;
        compact({ kind: 'dispatch', text: `Building ${task}`, sub: node ? `on ${node}` : undefined });
        const owned = arr(e['owned_files']).map(String).join(', ');
        verbose({
          kind: 'dispatch',
          text: `Building ${task}${attempt > 0 ? ` (attempt ${attempt + 1})` : ''}`,
          sub: [node && `on ${node}`, owned].filter(Boolean).join(' — ') || undefined,
        });
        phase = 'Building';
        break;
      }
      case 'task_retry': {
        const t = `Retrying ${str(e['task_id'])}`;
        compact({ kind: 'retry', text: t, tone: 'warn' });
        // The failure reason is on `error`, not `reason` — the old key was always empty, dropping the "why".
        verbose({ kind: 'retry', text: t, tone: 'warn', sub: str(e['error']) || undefined });
        break;
      }
      case 'replanned': {
        // A dynamic-replan round that actually spliced in new tasks — the swarm noticed missing work and grew
        // the plan. The routine "checked, nothing to add" rounds (added empty) are noise and stay hidden.
        const added = arr(e['added']).map(String);
        if (added.length > 0) {
          const t = `Re-planned — added ${added.length} task${added.length === 1 ? '' : 's'}`;
          compact({ kind: 'retry', text: t, tone: 'warn' });
          verbose({ kind: 'retry', text: t, sub: added.join(', '), tone: 'warn' });
        }
        break;
      }
      case 'scheduler_stuck': {
        // Terminal deadlock: tasks remain but none can be dispatched (an unsatisfiable dep / a sink that never
        // unblocks). The run cannot finish — surface it loudly; it is effectively a failure end-state.
        const rem = num(e['remaining']) ?? 0;
        const t = `Scheduler stuck — ${rem} task${rem === 1 ? '' : 's'} blocked, run can't finish`;
        compact({ kind: 'fail', text: t, tone: 'bad' });
        verbose({ kind: 'fail', text: t, tone: 'bad' });
        break;
      }
      case 'judge_verdict': {
        const verdict = str(e['verdict']);
        const conf = num(e['confidence']);
        const hint = str(e['hint']);
        // Only surface a judge verdict in verbose when it's actionable (not a routine "ok/observed", and an
        // empty/missing verdict is the ok case too) OR it carries a hint — those are the moments the AI
        // judge catches spec-drift/looping/over-reading.
        if ((verdict !== 'ok' && verdict !== '') || hint) {
          verbose({
            kind: 'judge',
            text: `Judge: ${str(e['task_id'])} → ${verdict || 'ok'}${conf != null ? ` (${Math.round(conf * 100)}%)` : ''}`,
            sub: hint || undefined,
            tone: judgeTone(verdict),
          });
        }
        break;
      }
      case 'pre_review': {
        const had = !!e['had_findings'];
        verbose({
          kind: 'prereview',
          text: `Pre-review ${str(e['task_id'])}: ${had ? 'findings raised' : 'clean'}`,
          tone: had ? 'warn' : 'good',
        });
        break;
      }
      case 'task_completed': {
        const failed = /fail|error/i.test(str(e['status']));
        const task = str(e['task_id']);
        const secs = num(e['elapsed_ms']);
        const nCalls = arr(e['tool_calls']).length;
        compact({ kind: failed ? 'fail' : 'done', text: `${task} ${failed ? 'failed' : 'done'}`, tone: failed ? 'bad' : 'good' });
        const detail = [secs != null && `${Math.round(secs / 1000)}s`, nCalls && `${nCalls} tool calls`].filter(Boolean).join(' · ');
        verbose({ kind: failed ? 'fail' : 'done', text: `${task} ${failed ? 'failed' : 'done'}`, sub: detail || undefined, tone: failed ? 'bad' : 'good' });
        break;
      }
      case 'smoke': {
        const r = (e['result'] ?? {}) as Record<string, unknown>;
        const tests = (r['tests'] ?? {}) as Record<string, unknown>;
        const testsPass = 'kind' in tests ? str(tests['kind']) === 'pass' : null;
        smoke = {
          ran: !!r['ran'],
          testsPass,
          entryOk: typeof r['entry_ok'] === 'boolean' ? (r['entry_ok'] as boolean) : null,
          pyFiles: num(r['py_files']) ?? 0,
        };
        compact({ kind: 'phase', text: 'Running end-to-end smoke tests' });
        verbose({
          kind: 'smoke',
          text: 'End-to-end smoke test',
          sub: [
            smoke.testsPass == null ? null : `tests ${smoke.testsPass ? 'pass' : 'fail'}`,
            smoke.entryOk == null ? null : `entry ${smoke.entryOk ? 'ok' : 'broken'}`,
            smoke.pyFiles ? `${smoke.pyFiles} files` : null,
          ]
            .filter(Boolean)
            .join(' · '),
          tone: smoke.testsPass === false || smoke.entryOk === false ? 'bad' : 'good',
        });
        phase = 'Verifying';
        break;
      }
      case 'run_finished': {
        const report = (e['report'] ?? {}) as Record<string, unknown>;
        const done = arr(report['done']).length;
        // Only CORE failures decide the verdict: a failed BONUS task (optional extra work) must not make the
        // whole run read as "failed".
        const bonus = new Set(arr(report['bonus']).map(String));
        const coreFailed = arr(report['failed'])
          .map(String)
          .filter((id) => !bonus.has(id)).length;
        const phases = (e['phases'] ?? {}) as Record<string, unknown>;
        const perDevRaw = (report['per_device'] ?? {}) as Record<string, unknown>;
        const perDevice: DeviceStat[] = Object.entries(perDevRaw)
          .map(([device, v]) => {
            const d = (v ?? {}) as Record<string, unknown>;
            return {
              node: nodeName(device),
              device,
              dispatched: num(d['dispatched']) ?? 0,
              toolCalls: num(d['tool_calls']) ?? 0,
              busyMs: num(d['busy_ms']) ?? 0,
            };
          })
          .filter((d) => d.dispatched > 0 || d.toolCalls > 0)
          .sort((a, b) => a.node.localeCompare(b.node));
        summary = { done, failed: coreFailed, totalMin: num(phases['total_min']), perDevice };
        compact({ kind: 'phase', text: 'Build complete', tone: coreFailed ? 'warn' : 'good' });
        verbose({
          kind: 'phase',
          text: 'Build complete',
          sub: `${done} done${coreFailed ? ` · ${coreFailed} failed` : ''}`,
          tone: coreFailed ? 'warn' : 'good',
        });
        phase = 'Done';
        finished = true;
        break;
      }
      case 'run_overview': {
        overview = {
          generated: e['generated'] === true,
          runCommand: typeof e['run_command'] === 'string' ? (e['run_command'] as string) : null,
          runCommandLang:
            typeof e['run_command_lang'] === 'string' ? (e['run_command_lang'] as string) : null,
          runCommandVerified: e['run_command_verified'] === true,
          features: arr(e['features']).map(String),
          engage: typeof e['engage'] === 'string' ? (e['engage'] as string) : null,
          next: arr(e['next']).map(String),
        };
        break;
      }
      default:
        break;
    }
  }
  return {
    activity: feed.slice(-30),
    verbose: vfeed.slice(-200),
    meta,
    plan,
    smoke,
    phase,
    finished,
    confidence,
    confidenceTrail: confTrail,
    planConfidence,
    summary,
    startedAt,
    overview,
  };
}

type Digest = {
  tool_calls?: number;
  errors?: number;
  recent?: string[];
  last_text?: string;
  reasoning?: string;
  full_reasoning?: string;
  calls?: SwarmCall[];
};

function foldEvents(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>
): { lanes: TurnLane[]; totals: SwarmRunTotals; planLanes: TurnLane[] } {
  const tasks = new Map<string, TurnLane>();
  const descriptions = new Map<string, string>();
  let seq = 0;

  for (const e of events) {
    const type = String(e['event'] ?? '');
    if (type === 'plan_loaded') {
      const ts = Array.isArray(e['tasks']) ? (e['tasks'] as Array<Record<string, unknown>>) : [];
      for (const t of ts) {
        const d = t['description'] ? String(t['description']) : '';
        if (d) descriptions.set(String(t['id'] ?? ''), d);
      }
      continue;
    }
    const taskId = String(e['task_id'] ?? '');
    if (!taskId) continue;

    if (type === 'task_dispatched') {
      const prev = tasks.get(taskId);
      tasks.set(taskId, {
        taskId,
        device: String(e['device'] ?? prev?.device ?? '?'),
        model: e['model'] ? String(e['model']) : prev?.model,
        status: 'running',
        seq: seq++,
      });
    } else if (type === 'task_retry') {
      const prev = tasks.get(taskId);
      tasks.set(taskId, {
        taskId,
        device: String(e['from_device'] ?? prev?.device ?? '?'),
        model: prev?.model,
        status: 'running',
        // Keep the retry's failure text so a lane that ultimately fails/stalls can explain why.
        error: e['error'] ? String(e['error']) : prev?.error,
        attempts: prev?.attempts,
        seq: seq++,
      });
    } else if (type === 'task_completed') {
      const prev = tasks.get(taskId);
      const statusStr = String(e['status'] ?? '').toLowerCase();
      const status: TurnStatus =
        statusStr.includes('fail') || statusStr.includes('error') ? 'error' : 'done';
      const toolCalls = Array.isArray(e['tool_calls'])
        ? (e['tool_calls'] as unknown[]).length
        : prev?.toolCalls;
      tasks.set(taskId, {
        taskId,
        device: String(e['device'] ?? prev?.device ?? '?'),
        model: e['model'] ? String(e['model']) : prev?.model,
        status,
        toolCalls,
        elapsedMs: typeof e['elapsed_ms'] === 'number' ? (e['elapsed_ms'] as number) : undefined,
        attempts: typeof e['attempts'] === 'number' ? (e['attempts'] as number) : prev?.attempts,
        // A failed completion keeps the last retry's reason; a clean one clears it.
        error: status === 'error' ? prev?.error : undefined,
        seq: seq++,
      });
    }
  }

  const lanes = [...tasks.values()].map((t) => {
    const act = activity[t.taskId] as Digest | undefined;
    return {
      ...t,
      description: descriptions.get(t.taskId) ?? t.description,
      lastText: act?.last_text || t.lastText,
      recent: act?.recent ?? t.recent,
      reasoning: act?.reasoning ?? t.reasoning,
      fullReasoning: act?.full_reasoning ?? t.fullReasoning,
      calls: act?.calls ?? t.calls,
      toolCalls: act?.tool_calls ?? t.toolCalls,
      errors: act?.errors ?? t.errors,
    };
  });

  // DONE on top (finished history), IN-PROGRESS at the BOTTOM — the active front sits just above the chat
  // input where the eye is. Within a group, oldest-activity first so the freshest work sinks to the bottom.
  const order: Record<TurnStatus, number> = { done: 0, error: 1, running: 2 };
  lanes.sort((a, b) => order[a.status] - order[b.status] || a.seq - b.seq);

  const totals: SwarmRunTotals = {
    tasks: lanes.length,
    running: lanes.filter((l) => l.status === 'running').length,
    done: lanes.filter((l) => l.status === 'done').length,
    failed: lanes.filter((l) => l.status === 'error').length,
  };

  // PLAN-phase generation lanes: each parallel architect draft writes a `plandraft-N` digest (model + full
  // reasoning). Surface them as their OWN group so you can see what every model generated while decomposing
  // the app — the reasoning that was invisible before. NOT build tasks, so they're excluded from `totals`.
  // Running until the plan is chosen (plan_loaded / first dispatch), then done (kept, collapsed, for review).
  const planned = events.some(
    (e) => e['event'] === 'plan_loaded' || e['event'] === 'task_dispatched'
  );
  const planLanes: TurnLane[] = Object.keys(activity)
    .filter((k) => /^plandraft-\d+$/.test(k))
    .sort()
    .map((k, i) => {
      const d = (activity[k] ?? {}) as Digest & { model?: string };
      return {
        taskId: k,
        description: `Drafting the plan skeleton (candidate ${i + 1})`,
        device: d.model ?? 'planner',
        model: d.model,
        status: (planned ? 'done' : 'running') as TurnStatus,
        lastText: d.last_text,
        recent: d.recent,
        reasoning: d.reasoning,
        fullReasoning: d.full_reasoning,
        calls: d.calls,
        toolCalls: d.tool_calls,
        errors: d.errors,
        seq: i,
      };
    })
    .filter((l) => (l.fullReasoning || l.reasoning || l.lastText || '').trim().length > 0);

  return { lanes, totals, planLanes };
}

// Derive the per-phase TODO from the engine's deterministic event stream. Every checkbox is flipped by a
// scheduler/orchestrator EVENT, never by a model claiming it did something — that is what makes it honest.
// The load-bearing rule: a completed build task is 'unverified' (the app was never run), and only Verify's
// complete_result.passed&&verified earns a green 'done'.
function buildPhaseTodo(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>,
  opts: { clarifyPending: boolean }
): PhaseTodo[] {
  let gates: Record<string, unknown> = {};
  let scoutsN: number | null = null;
  let researchQ: number | null = null;
  let researchDone: number | null = null;
  let planConf: number | null = null;
  let taskCount: number | null = null;
  let pillarsN: number | null = null;
  const retargets: Array<{ round: number; action: string }> = [];
  let askedQ: number | null = null;
  let planned = false;
  let planLoaded = false;
  let contractsModules: number | null = null;
  const planTasks: Array<{ id: string; description?: string }> = [];
  const tstate = new Map<string, { state: TodoState; device?: string; error?: string }>();
  const judgeFailed = new Set<string>();
  const salvaged = new Set<string>();
  const replans: number[] = [];
  let schedulerStuck: number | null = null;
  const reportFailed = new Set<string>();
  let completeResult: { passed: boolean; verified: boolean; remaining?: number | null } | null = null;
  let completeRan = false;
  let smoke: { ran: boolean; kind?: string } | null = null;
  let repro: number | null = null;
  let reviewFix: { reproduced: number; accepted: number } | null = null;
  let astReview: number | null = null;

  for (const e of events) {
    const t = String(e['event'] ?? '');
    if (t === 'run_started' && e['gates'] && typeof e['gates'] === 'object')
      gates = e['gates'] as Record<string, unknown>;
    else if (t === 'scouts_planned') scoutsN = arr(e['lenses']).length || (num(e['count']) ?? 0);
    else if (t === 'research_planned') researchQ = num(e['count']) ?? arr(e['questions']).length;
    else if (t === 'research_completed') researchDone = num(e['findings']) ?? 0;
    else if (t === 'pillars') pillarsN = num(e['count']) ?? arr(e['pillars']).length;
    else if (t === 'confidence_retarget')
      retargets.push({ round: num(e['round']) ?? 0, action: str(e['action']) });
    else if (t === 'low_confidence_ask') askedQ = arr(e['questions']).length;
    else if (t === 'contracts') contractsModules = num(e['modules']) ?? 0;
    else if (t === 'plan_loaded') {
      planLoaded = true;
      planned = true;
      planConf = num(e['plan_confidence']) ?? planConf;
      const ts = arr(e['tasks']) as Array<Record<string, unknown>>;
      taskCount = ts.length;
      for (const tk of ts)
        planTasks.push({
          id: String(tk['id'] ?? ''),
          description: tk['description'] ? String(tk['description']) : undefined,
        });
    } else if (t === 'task_dispatched') {
      planned = true;
      const id = str(e['task_id']);
      if (id) tstate.set(id, { state: 'running', device: str(e['device']) });
    } else if (t === 'task_retry') {
      const id = str(e['task_id']);
      if (id) tstate.set(id, { ...(tstate.get(id) ?? { state: 'running' }), error: str(e['error']) });
    } else if (t === 'task_completed') {
      const id = str(e['task_id']);
      if (id) {
        const cur = tstate.get(id);
        const dev = str(e['device']) || cur?.device;
        if (str(e['status']) === 'failed')
          tstate.set(id, { device: dev, state: judgeFailed.has(id) ? 'judge_failed' : 'failed' });
        // BUILT but UNVERIFIED — the worker loop returned + passed a syntax gate; the app was NOT run.
        else tstate.set(id, { device: dev, state: 'unverified' });
      }
    } else if (t === 'judge_verdict') {
      const id = str(e['task_id']);
      const a = str(e['action']);
      if (a === 'failed') judgeFailed.add(id);
      if (a === 'salvaged') salvaged.add(id);
    } else if (t === 'replanned') {
      const added = arr(e['added']).length;
      if (added > 0) replans.push(added);
    } else if (t === 'scheduler_stuck') schedulerStuck = num(e['remaining']) ?? 0;
    else if (t === 'complete_verify') completeRan = true;
    else if (t === 'complete_result')
      completeResult = {
        passed: e['passed'] === true,
        verified: e['verified'] === true,
        remaining: num(e['remaining_findings']),
      };
    else if (t === 'smoke' || t === 'smoke_after_fix') {
      const r = (e['result'] ?? {}) as Record<string, unknown>;
      const tests = (r['tests'] ?? {}) as Record<string, unknown>;
      smoke = { ran: r['ran'] === true, kind: str(tests['kind']) };
    } else if (t === 'review_repro') repro = num(e['reproduced']) ?? repro;
    else if (t === 'review_fix_summary')
      reviewFix = { reproduced: num(e['reproduced']) ?? 0, accepted: num(e['accepted']) ?? 0 };
    else if (t === 'review' || t === 'review_after_fix')
      astReview = num(e['new_findings']) ?? arr(e['new_findings']).length ?? astReview;
  }

  // Second pass: task_completed(failed) may precede its judge_verdict in some orderings — reconcile.
  for (const [id, s] of tstate) if (s.state === 'failed' && judgeFailed.has(id)) s.state = 'judge_failed';

  const plandraftN = Object.keys(activity).filter((k) => /^plandraft-\d+$/.test(k)).length;
  const gateOn = (k: string) => gates[k] !== false;

  const it = (
    id: string,
    label: string,
    state: TodoState,
    detail?: string,
    device?: string
  ): PhaseTodoItem => ({ id, label, state, detail, device, advisory: state === 'advisory' });

  // ---- RESEARCH ----
  const researchHappened = scoutsN != null || researchQ != null || researchDone != null;
  const research: PhaseTodoItem[] = [];
  if (researchHappened || !planned) {
    research.push(it('r-start', 'Fleet configured, run started', 'done'));
    if (scoutsN != null) research.push(it('r-scouts', `Scouts dispatched — ${scoutsN} lenses`, 'done'));
    else if (researchQ != null)
      research.push(it('r-q', `Research questions scoped — ${researchQ}`, 'done'));
    if (researchDone != null)
      research.push(it('r-done', `Research finished — ${researchDone} findings returned`, 'done'));
    else if (researchHappened) research.push(it('r-run', 'Researching…', 'running'));
  } else {
    research.push(it('r-skip', 'No research this run', 'skipped'));
  }

  // ---- PLAN ----
  const plan: PhaseTodoItem[] = [];
  if (plandraftN > 0 || planLoaded)
    plan.push(
      it(
        'p-draft',
        `Drafting — ${Math.max(plandraftN, 1)} candidate${plandraftN === 1 ? '' : 's'}`,
        planned ? 'done' : 'running'
      )
    );
  if (planConf != null)
    plan.push(it('p-conf', `Confidence scored — ${planConf}/100`, 'done')); // the NUMBER, never a verdict
  for (const r of retargets)
    plan.push(it(`p-rt-${r.round}`, `Retarget round ${r.round} — ${r.action}`, 'done'));
  if (askedQ != null)
    plan.push(
      it(
        'p-ask',
        `Asked you ${askedQ} question${askedQ === 1 ? '' : 's'}`,
        opts.clarifyPending ? 'running' : 'done',
        opts.clarifyPending ? 'waiting on your answer' : undefined
      )
    );
  if (pillarsN != null) plan.push(it('p-pillars', `Quality pillars distilled — ${pillarsN}`, 'done'));
  if (planLoaded) plan.push(it('p-done', `Plan finalized — ${taskCount ?? 0} tasks`, 'done'));
  else if (plandraftN > 0) plan.push(it('p-run', 'Finalizing the plan…', 'running'));

  // ---- CONTRACTS ----
  const contracts: PhaseTodoItem[] = [];
  if (!gateOn('contracts')) contracts.push(it('c-off', 'Contract-freeze gate off', 'skipped'));
  else if (contractsModules != null)
    contracts.push(it('c-frozen', `Frozen interfaces — ${contractsModules} modules`, 'done'));
  else if (planLoaded)
    contracts.push(
      it('c-adv', 'Contract-freeze gate on', 'advisory', 'runs only for Python trees; not always observable')
    );

  // ---- BUILD ---- (per plan task — surfaces PENDING + BLOCKED tasks lanes can't show)
  const build: PhaseTodoItem[] = [];
  for (const tk of planTasks) {
    const s = tstate.get(tk.id);
    let state: TodoState;
    let detail: string | undefined;
    if (s) {
      state = s.state;
      if (state === 'unverified' && salvaged.has(tk.id)) detail = 'salvaged — judge cut a loop';
      else if (state === 'judge_failed') detail = 'judge decision';
      else if (s.error) detail = s.error.slice(0, 80);
    } else if (reportFailed.has(tk.id) || schedulerStuck != null) {
      state = 'blocked';
      detail = schedulerStuck != null ? 'scheduler stuck' : 'a dependency failed';
    } else {
      state = 'pending';
    }
    build.push(it(`b-${tk.id}`, tk.description || tk.id, state, detail, s?.device));
  }
  for (const n of replans) build.push(it(`b-replan-${n}`, `Re-planned +${n} tasks`, 'done'));
  if (schedulerStuck != null)
    build.push(it('b-stuck', `Scheduler blocked — ${schedulerStuck} task(s) unschedulable`, 'blocked'));

  // ---- VERIFY ---- (all events already emitted; the honest "it works" signal)
  const verify: PhaseTodoItem[] = [];
  const verifyGate = gateOn('complete') || gateOn('smoke');
  if (verifyGate) {
    let vs: TodoState;
    let vdetail: string | undefined;
    if (completeResult) {
      vs = completeResult.passed
        ? completeResult.verified
          ? 'done'
          : 'unverified'
        : 'failed';
      vdetail = completeResult.passed
        ? completeResult.verified
          ? 'app runs — verified end-to-end'
          : 'shipped — no oracle ran'
        : `${completeResult.remaining ?? 0} findings remain`;
    } else if (completeRan || smoke) vs = 'running';
    else vs = 'pending';
    // A would-be pass with no real test run is downgraded and never reads "tests pass".
    if (vs === 'done' && smoke && smoke.kind !== 'pass') {
      vs = 'unverified';
      vdetail = 'no tests ran';
    }
    verify.push(it('v-e2e', 'End-to-end verify', vs, vdetail));
  }
  if (repro != null) verify.push(it('v-repro', `Repro gate — ${repro} findings reproduced`, 'done'));
  if (reviewFix)
    verify.push(
      it('v-fix', `Review fixes — ${reviewFix.accepted} accepted / ${reviewFix.reproduced} reproduced`, 'done')
    );
  if (astReview != null)
    verify.push(it('v-ast', `Unwired-module review — ${astReview} new findings`, 'done'));

  // ---- DONE ----
  const done: PhaseTodoItem[] = [];
  const finishedEvent = events.some((e) => e['event'] === 'run_finished');
  if (schedulerStuck != null) done.push(it('d-blocked', 'Run blocked — scheduler deadlocked', 'blocked'));
  else if (finishedEvent) {
    const green = completeResult ? completeResult.passed && completeResult.verified : false;
    const anyHardFail = [...tstate.values()].some((s) => s.state === 'failed');
    done.push(
      it(
        'd-outcome',
        green ? 'Finished — app verified' : anyHardFail ? 'Finished — with failures' : 'Finished — unverified',
        green ? 'done' : anyHardFail ? 'failed' : 'unverified'
      )
    );
  }

  const rollUp = (items: PhaseTodoItem[]): TodoState => {
    const real = items.filter((i) => i.state !== 'advisory');
    if (real.some((i) => i.state === 'failed' || i.state === 'judge_failed' || i.state === 'blocked'))
      return 'failed';
    if (real.some((i) => i.state === 'running')) return 'running';
    if (real.length === 0) return 'pending';
    if (real.every((i) => i.state === 'skipped')) return 'skipped';
    if (real.some((i) => i.state === 'done')) return 'done';
    if (real.some((i) => i.state === 'unverified')) return 'unverified';
    return 'pending';
  };
  const mk = (key: PhaseKey, label: string, items: PhaseTodoItem[]): PhaseTodo => ({
    key,
    label,
    items,
    state: rollUp(items),
    active: false,
    counts: {
      done: items.filter((i) => i.state === 'done').length,
      total: items.filter((i) => i.state !== 'advisory').length,
    },
  });
  const phases: PhaseTodo[] = [
    mk('research', 'Research', research),
    mk('plan', 'Plan', plan),
    mk('contracts', 'Contracts', contracts),
    mk('build', 'Build', build),
    mk('verify', 'Verify', verify),
    mk('done', 'Done', done),
  ];
  // Active = the last phase that has started (any non-pending item) and isn't fully done — monotonic pipeline.
  let activeIdx = -1;
  phases.forEach((p, i) => {
    if (p.items.some((x) => x.state !== 'pending')) activeIdx = i;
  });
  if (activeIdx >= 0) phases[activeIdx].active = true;
  return phases;
}

export function useSwarmRun(workingDir: string | undefined, pollMs = 2000): SwarmRunState {
  const [state, setState] = useState<SwarmRunState>(EMPTY);
  // Keep the last non-empty run visible between polls so a finished run does not flicker away.
  const lastRunId = useRef<string | null>(null);

  useEffect(() => {
    if (!workingDir) {
      setState({ ...EMPTY, loading: false });
      return;
    }
    let alive = true;

    const tick = async () => {
      try {
        const data = await window.electron.readSwarmRun(workingDir);
        if (!alive) return;
        if (!data) {
          setState({ ...EMPTY, loading: false });
          lastRunId.current = null;
          return;
        }
        const { lanes, totals, planLanes } = foldEvents(data.events, data.activity);
        const phaseTodo = buildPhaseTodo(data.events, data.activity, {
          clarifyPending: !!data.clarify?.pending,
        });
        const {
          activity,
          verbose,
          meta,
          plan,
          smoke,
          phase,
          finished,
          planConfidence,
          confidence,
          confidenceTrail,
          summary,
          startedAt,
          overview,
        } = buildActivity(data.events);
        lastRunId.current = data.runId;
        setState({
          present: true,
          runId: data.runId,
          lanes,
          planLanes,
          phaseTodo,
          overview,
          totals,
          activity,
          verboseActivity: verbose,
          meta,
          plan,
          smoke,
          phase,
          planConfidence,
          confidence,
          confidenceTrail,
          inProgress: !finished,
          finished,
          summary,
          startedAt,
          clarify: data.clarify,
          mtime: data.mtime,
          heartbeat: data.heartbeat,
          loading: false,
        });
      } catch {
        if (alive) setState((s) => ({ ...s, loading: false }));
      }
    };

    void tick();
    const iv = setInterval(() => void tick(), pollMs);
    return () => {
      alive = false;
      clearInterval(iv);
    };
  }, [workingDir, pollMs]);

  return state;
}
