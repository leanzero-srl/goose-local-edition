import path from 'node:path';

/**
 * Reaping a benchmark run's stray processes — per PID, by RUN-UNIQUE argv tokens.
 *
 * U-M8 (branch review, 2026-09-01): cancel ran `pkill -9 -f <workdir>`, a SUBSTRING match over every
 * process's argv — it SIGKILLed any `tail -f`, `less`, `code`, or tick.py that happened to hold the
 * path. And the REAPING gate (r2 died at INTEGRATE minute 139 to a killpg aimed at two orphans that
 * shared the engine's group) forbids group kills on groups we did not create. So: name the exact
 * tokens only this run's processes carry, match whole `ps` lines against them, and kill each pid.
 *
 * The tokens are what run_build.py puts on the command line and nothing else does:
 *   - the engine:  `goose swarm run … --log-file <workdir>/run.jsonl` (started in its own session,
 *     so the runner's group kill never reaches it — this is the process that keeps the fleet
 *     generating for a dead run);
 *   - the scorer's vendorsync child: `… --db <workdir>/graded.db`.
 */
export function benchRunArgvTokens(workdir: string): string[] {
  return [`--log-file ${path.join(workdir, 'run.jsonl')}`, `--db ${path.join(workdir, 'graded.db')}`];
}

/** PIDs from `ps -axo pid=,args=` output whose full argv carries one of `tokens`; never `selfPid`. */
export function pidsMatchingTokens(psOutput: string, tokens: string[], selfPid: number): number[] {
  const pids: number[] = [];
  for (const line of psOutput.split('\n')) {
    const m = line.match(/^\s*(\d+)\s+(.*)$/);
    if (!m) continue;
    const pid = Number(m[1]);
    if (pid === selfPid) continue;
    if (tokens.some((t) => m[2].includes(t))) pids.push(pid);
  }
  return pids;
}
