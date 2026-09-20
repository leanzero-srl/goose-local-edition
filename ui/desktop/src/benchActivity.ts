import { harnessPhase } from './benchPhase';

export interface BenchmarkActivityEntry {
  id: number;
  at: number;
  kind: 'tool' | 'phase';
  title: string;
  detail?: string;
  detailTruncated?: boolean;
}
export interface BenchmarkActivity {
  entries: BenchmarkActivityEntry[];
  raw: string;
  lastOutputAt: number | null;
  nextId: number;
  pendingTool: number | null;
  commandTool: number | null;
}
export const emptyBenchmarkActivity = (): BenchmarkActivity => ({
  entries: [],
  raw: '',
  lastOutputAt: null,
  nextId: 1,
  pendingTool: null,
  commandTool: null,
});
const ansi = new RegExp(String.fromCharCode(27) + '\\[[0-?]*[ -/]*[@-~]', 'g');

/** Console observations describe recorded actions, never tool success or model-authored status. */
export function appendBenchmarkActivity(
  state: BenchmarkActivity,
  input: string,
  stream: 'stdout' | 'stderr',
  at: number
): BenchmarkActivity {
  const line = input.slice(0, 8192).replace(ansi, '');
  const next = {
    ...state,
    raw: (state.raw + (stream === 'stderr' ? '[stderr] ' : '') + line + '\n').slice(-16000),
    lastOutputAt: at,
  };
  const phase = stream === 'stdout' ? harnessPhase(line) : null;
  const tool = stream === 'stdout' ? line.match(/^\s*▸\s+([a-zA-Z0-9_.:-]+)\s*$/) : null;
  if (phase || tool) {
    const entry: BenchmarkActivityEntry = {
      id: state.nextId,
      at,
      kind: phase ? 'phase' : 'tool',
      title: phase ? (phase === 'build' ? 'Model build started' : 'Scoring started') : tool![1],
    };
    return {
      ...next,
      entries: [...state.entries, entry].slice(-12),
      nextId: state.nextId + 1,
      pendingTool: tool ? entry.id : null,
      commandTool: null,
    };
  }
  if (state.commandTool !== null && stream === 'stdout') {
    if (!line.trim() || /^\s*[─━]{3,}/.test(line)) return { ...next, commandTool: null };
    return {
      ...next,
      entries: state.entries.map((entry) =>
        entry.id === state.commandTool
          ? {
              ...entry,
              detail: ((entry.detail ?? '') + '\n' + line).slice(0, 4000),
              detailTruncated:
                entry.detailTruncated || ((entry.detail ?? '') + '\n' + line).length > 4000,
            }
          : entry
      ),
    };
  }
  if (state.pendingTool !== null && stream === 'stdout') {
    const argument = line.match(/^\s+(?:command:|path:?|source:)\s+(.+)$/);
    if (argument)
      return {
        ...next,
        pendingTool: null,
        commandTool: /^\s+command:/.test(line) ? state.pendingTool : null,
        entries: state.entries.map((entry) =>
          entry.id === state.pendingTool
            ? {
                ...entry,
                detail: argument[1].slice(0, 4000),
                detailTruncated: argument[1].length > 4000,
              }
            : entry
        ),
      };
    if (!/^\s{4,}[a-zA-Z_]+:/.test(line)) next.pendingTool = null;
  }
  return next;
}
