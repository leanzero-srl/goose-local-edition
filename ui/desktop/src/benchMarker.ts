import fsSync from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { ChildProcess } from 'node:child_process';

/**
 * The "a benchmark is running here" marker other machines read before placing work on this Mac.
 *
 * THE RULE (identical in crates/leanzero-link/src/load.rs `detect_benchmark` and
 * ~/Projects/machine-sync/bin/node_fleet.py PROBE; change all three together):
 * - the launcher writes `~/.leanzero/benchmarks/<pid>.json` = {pid, label, started_at (ISO-8601 UTC)}
 *   right after it spawns the runner, and removes it when the runner closes or fails to start;
 * - a marker counts only while its pid is ALIVE (kill 0 succeeds or says EPERM) AND that process
 *   started no later than `started_at` + 120 s (a crash leaves the file, a reused pid is a younger
 *   process, so neither reads as a benchmark forever);
 * - the refusal reason names the run: "benchmark running: <label> <started_at to the minute> (pid N)".
 *
 * Why a marker: the packaged app runs .../Resources/swarm-bench/bench/run_build.py, whose argv
 * never says "benchmark", so argv matching missed it and only a 2-min file-mtime check caught it.
 */
export const BENCH_MARKER_DIR = path.join(os.homedir(), '.leanzero', 'benchmarks');

export type BenchMarker = { pid: number; label: string; started_at: string };

export const benchMarkerPath = (dir: string, pid: number) => path.join(dir, `${pid}.json`);

/** "Gauntlet build run", "Forge rescore": what the refusal reason on another Mac shows. */
export const benchMarkerLabel = (familyWord: string, kind: 'build run' | 'rescore') =>
  `${familyWord} ${kind}`;

/**
 * Write the marker for `child` and remove it on close/error. Never throws: a marker that cannot be
 * written only weakens remote detection (the file-mtime and env checks remain), it must not stop a run.
 */
export function attachBenchMarker(
  child: ChildProcess,
  label: string,
  dir: string = BENCH_MARKER_DIR,
  now: () => Date = () => new Date()
): string | null {
  const pid = child.pid;
  if (!pid) return null;
  const file = benchMarkerPath(dir, pid);
  const body: BenchMarker = { pid, label, started_at: now().toISOString() };
  try {
    fsSync.mkdirSync(dir, { recursive: true });
    fsSync.writeFileSync(file, JSON.stringify(body));
  } catch {
    return null;
  }
  const remove = () => {
    try {
      fsSync.rmSync(file, { force: true });
    } catch {
      /* a stale marker is harmless: readers require a live pid */
    }
  };
  child.once('close', remove);
  child.once('error', remove);
  return file;
}
