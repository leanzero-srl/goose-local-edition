import { useEffect, useRef, useState } from 'react';

/**
 * Reads the LIVE swarm run for a working directory (via the `read-swarm-run` IPC) and folds its event
 * stream + per-turn activity digests into one lane PER TASK — the "turn loops" the user inspects. A
 * `goose swarm run` writes <cwd>/.swarm/run-<id>.jsonl (task_dispatched / task_completed / task_retry …)
 * plus <cwd>/.swarm/activity/<task>.json ({tool_calls, errors, recent[], last_text}); this turns that raw
 * data into what each node is DOING right now. Polls so an in-flight run animates.
 */

export type TurnStatus = 'running' | 'done' | 'error';

export interface TurnLane {
  taskId: string;
  device: string;
  model?: string;
  status: TurnStatus;
  lastText?: string;
  recent?: string[];
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

export interface SwarmRunState {
  present: boolean;
  runId: string | null;
  lanes: TurnLane[];
  totals: SwarmRunTotals;
  mtime: number | null;
  loading: boolean;
}

const EMPTY: SwarmRunState = {
  present: false,
  runId: null,
  lanes: [],
  totals: { tasks: 0, running: 0, done: 0, failed: 0 },
  mtime: null,
  loading: true,
};

type Digest = { tool_calls?: number; errors?: number; recent?: string[]; last_text?: string };

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
        lastRunId.current = data.runId;
        setState({
          present: true,
          runId: data.runId,
          lanes,
          totals,
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
