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
  device: string;
  model?: string;
  status: TurnStatus;
  lastText?: string;
  recent?: string[];
  reasoning?: string;
  calls?: SwarmCall[];
  toolCalls?: number;
  errors?: number;
  elapsedMs?: number;
  seq: number;
}

export interface SwarmRunTotals {
  tasks: number;
  running: number;
  done: number;
  failed: number;
}

export type ActivityKind =
  | 'phase'
  | 'plan'
  | 'dispatch'
  | 'done'
  | 'fail'
  | 'retry'
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

export interface SwarmRunState {
  present: boolean;
  runId: string | null;
  lanes: TurnLane[];
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
  /** True while a run is underway (started, not finished, and its files are still fresh). */
  inProgress: boolean;
  mtime: number | null;
  loading: boolean;
}

const EMPTY: SwarmRunState = {
  present: false,
  runId: null,
  lanes: [],
  totals: { tasks: 0, running: 0, done: 0, failed: 0 },
  activity: [],
  verboseActivity: [],
  meta: null,
  plan: [],
  smoke: null,
  phase: '',
  inProgress: false,
  mtime: null,
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
} {
  const feed: ActivityItem[] = [];
  const vfeed: ActivityItem[] = [];
  let phase = 'Starting…';
  let finished = false;
  let cseq = 0;
  let vseq = 0;
  let meta: RunMeta | null = null;
  let plan: PlanTask[] = [];
  let smoke: SmokeResult | null = null;
  const compact = (it: Omit<ActivityItem, 'seq'>) => feed.push({ ...it, seq: cseq++ });
  const verbose = (it: Omit<ActivityItem, 'seq'>) => vfeed.push({ ...it, seq: vseq++ });

  for (const e of events) {
    const type = String(e['event'] ?? '');
    switch (type) {
      case 'run_started': {
        const pool = arr(e['pool']).map((d) => nodeName(str((d as Record<string, unknown>)['id'])));
        meta = {
          prompt: str(e['prompt']),
          plannerModel: str(e['planner_model']),
          endpoint: str(e['endpoint']),
          nodes: pool,
          gates: !!e['gates'] || !!e['assured'],
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
      case 'plan_loaded': {
        const tasks = arr(e['tasks']) as Array<Record<string, unknown>>;
        plan = tasks.map((t) => ({
          id: str(t['id']),
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
        verbose({ kind: 'retry', text: t, tone: 'warn', sub: str(e['reason']) || undefined });
        break;
      }
      case 'judge_verdict': {
        const verdict = str(e['verdict']);
        const conf = num(e['confidence']);
        const hint = str(e['hint']);
        // Only surface a judge verdict in verbose when it's actionable (not a routine "ok/observed") OR it
        // carries a hint — those are the moments the AI judge catches spec-drift/looping/over-reading.
        if (verdict !== 'ok' || hint) {
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
        const failedN = arr(report['failed']).length;
        compact({ kind: 'phase', text: 'Build complete', tone: failedN ? 'warn' : 'good' });
        verbose({
          kind: 'phase',
          text: 'Build complete',
          sub: `${done} done${failedN ? ` · ${failedN} failed` : ''}`,
          tone: failedN ? 'warn' : 'good',
        });
        phase = 'Done';
        finished = true;
        break;
      }
      default:
        break;
    }
  }
  return { activity: feed.slice(-30), verbose: vfeed.slice(-200), meta, plan, smoke, phase, finished };
}

type Digest = {
  tool_calls?: number;
  errors?: number;
  recent?: string[];
  last_text?: string;
  reasoning?: string;
  calls?: SwarmCall[];
};

function foldEvents(
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown>
): { lanes: TurnLane[]; totals: SwarmRunTotals } {
  const tasks = new Map<string, TurnLane>();
  let seq = 0;

  for (const e of events) {
    const type = String(e['event'] ?? '');
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
        seq: seq++,
      });
    }
  }

  const lanes = [...tasks.values()].map((t) => {
    const act = activity[t.taskId] as Digest | undefined;
    return {
      ...t,
      lastText: act?.last_text || t.lastText,
      recent: act?.recent ?? t.recent,
      reasoning: act?.reasoning ?? t.reasoning,
      calls: act?.calls ?? t.calls,
      toolCalls: act?.tool_calls ?? t.toolCalls,
      errors: act?.errors ?? t.errors,
    };
  });

  // Running first, then most-recent activity first — the freshest turn loops surface at the top.
  const order: Record<TurnStatus, number> = { running: 0, error: 1, done: 2 };
  lanes.sort((a, b) => order[a.status] - order[b.status] || b.seq - a.seq);

  const totals: SwarmRunTotals = {
    tasks: lanes.length,
    running: lanes.filter((l) => l.status === 'running').length,
    done: lanes.filter((l) => l.status === 'done').length,
    failed: lanes.filter((l) => l.status === 'error').length,
  };
  return { lanes, totals };
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
        const { lanes, totals } = foldEvents(data.events, data.activity);
        const { activity, verbose, meta, plan, smoke, phase, finished } = buildActivity(data.events);
        lastRunId.current = data.runId;
        setState({
          present: true,
          runId: data.runId,
          lanes,
          totals,
          activity,
          verboseActivity: verbose,
          meta,
          plan,
          smoke,
          phase,
          inProgress: !finished,
          mtime: data.mtime,
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
