import type { BenchSeriesSnapshot } from '../../benchSeries';
import type { BenchSession } from './bridge';

export function acceptsSeriesUpdate(
  current: BenchSeriesSnapshot | null,
  incoming: BenchSeriesSnapshot | undefined
): boolean {
  if (!incoming) return current?.running !== true;
  if (!current || current.id !== incoming.id) return !current?.running;
  if (incoming.currentBuild < current.currentBuild) return false;
  return current.running || !incoming.running;
}

export function seriesSessionKey(session: BenchSession): string {
  return session.runId ?? `start-${session.startedAt}`;
}

export function seriesSessions(
  seriesId: string,
  sessions: readonly BenchSession[]
): BenchSession[] {
  return sessions
    .filter((session) => session.seriesId === seriesId)
    .sort((a, b) => (a.buildIndex ?? 0) - (b.buildIndex ?? 0));
}

export type BenchmarkHistoryGroup =
  | { key: string; kind: 'run'; sessions: [BenchSession] }
  | { key: string; kind: 'series'; sessions: BenchSession[] };

export function groupBenchmarkHistory(sessions: readonly BenchSession[]): BenchmarkHistoryGroup[] {
  const groups: BenchmarkHistoryGroup[] = [];
  const bySeries = new Map<string, BenchmarkHistoryGroup>();
  for (const session of sessions) {
    if (!session.seriesId) {
      groups.push({ key: seriesSessionKey(session), kind: 'run', sessions: [session] });
      continue;
    }
    let group = bySeries.get(session.seriesId);
    if (!group) {
      group = { key: session.seriesId, kind: 'series', sessions: [] };
      bySeries.set(session.seriesId, group);
      groups.push(group);
    }
    group.sessions.push(session);
  }
  for (const group of groups) {
    if (group.kind === 'series')
      group.sessions.sort((a, b) => (a.buildIndex ?? 0) - (b.buildIndex ?? 0));
  }
  return groups;
}
