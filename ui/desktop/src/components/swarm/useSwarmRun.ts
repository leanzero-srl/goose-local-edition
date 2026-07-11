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

export type ActivityKind = 'phase' | 'plan' | 'dispatch' | 'done' | 'fail' | 'retry' | 'review';
export interface ActivityItem {
  kind: ActivityKind;
  text: string;
  sub?: string;
  seq: number;
}

export interface SwarmRunState {
  present: boolean;
  runId: string | null;
  lanes: TurnLane[];
  totals: SwarmRunTotals;
  /** A human-readable timeline of what the swarm is doing — shown even during PLANNING (before any worker
   *  executes), so the user isn't left staring at a blank "working on it". */
  activity: ActivityItem[];
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

/** Turn the run event stream into a friendly activity timeline + the current phase. This covers the whole
 *  build — the planning events (research, plan-ready) that the lane-fold ignores are the ones that make
 *  a build visible before any worker starts executing. */
function buildActivity(events: Array<Record<string, unknown>>): {
  activity: ActivityItem[];
  phase: string;
  finished: boolean;
} {
  const feed: ActivityItem[] = [];
  let phase = 'Starting…';
  let finished = false;
  let seq = 0;
  for (const e of events) {
    const type = String(e['event'] ?? '');
    switch (type) {
      case 'run_started':
        feed.push({ kind: 'phase', text: 'Starting the build', seq: seq++ });
        phase = 'Starting';
        break;
      case 'scouts_planned':
        feed.push({
          kind: 'phase',
          text: 'Planning research',
          sub: Array.isArray(e['lenses']) ? (e['lenses'] as string[]).join(', ') : undefined,
          seq: seq++,
        });
        phase = 'Planning research';
        break;
      case 'research_planned':
        feed.push({ kind: 'phase', text: 'Researching the problem', seq: seq++ });
        phase = 'Researching';
        break;
      case 'research_completed':
        feed.push({ kind: 'phase', text: `Research done — ${Number(e['findings'] ?? 0)} findings`, seq: seq++ });
        phase = 'Planning';
        break;
      case 'pillars':
        feed.push({ kind: 'phase', text: 'Defining quality pillars', seq: seq++ });
        phase = 'Planning';
        break;
      case 'plan_loaded': {
        const tasks = Array.isArray(e['tasks']) ? (e['tasks'] as Array<Record<string, unknown>>) : [];
        const n = typeof e['task_count'] === 'number' ? (e['task_count'] as number) : tasks.length;
        feed.push({
          kind: 'plan',
          text: `Plan ready — ${n} task${n === 1 ? '' : 's'}`,
          sub: tasks.map((t) => String(t['id'] ?? '')).filter(Boolean).join(', '),
          seq: seq++,
        });
        phase = 'Building';
        break;
      }
      case 'task_dispatched':
        feed.push({
          kind: 'dispatch',
          text: `Building ${String(e['task_id'] ?? '')}`,
          sub: e['device'] ? `on ${nodeName(String(e['device']))}` : undefined,
          seq: seq++,
        });
        phase = 'Building';
        break;
      case 'task_retry':
        feed.push({ kind: 'retry', text: `Retrying ${String(e['task_id'] ?? '')}`, seq: seq++ });
        break;
      case 'task_completed': {
        const failed = /fail|error/i.test(String(e['status'] ?? ''));
        feed.push({
          kind: failed ? 'fail' : 'done',
          text: `${String(e['task_id'] ?? '')} ${failed ? 'failed' : 'done'}`,
          seq: seq++,
        });
        break;
      }
      case 'smoke':
        feed.push({ kind: 'phase', text: 'Running end-to-end smoke tests', seq: seq++ });
        phase = 'Verifying';
        break;
      case 'run_finished':
        feed.push({ kind: 'phase', text: 'Build complete', seq: seq++ });
        phase = 'Done';
        finished = true;
        break;
      default:
        break; // judge_verdict / pre_review etc. are background noise — omitted from the feed
    }
  }
  return { activity: feed.slice(-30), phase, finished };
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
        const { activity, phase, finished } = buildActivity(data.events);
        lastRunId.current = data.runId;
        setState({
          present: true,
          runId: data.runId,
          lanes,
          totals,
          activity,
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
