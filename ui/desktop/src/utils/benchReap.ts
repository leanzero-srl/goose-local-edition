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
 * The tokens are what run_build.py and the scorers put on the command line and nothing else does:
 *   - the engine:  `goose swarm run … --log-file <workdir>/run.jsonl` (started in its own session,
 *     so the runner's group kill never reaches it — this is the process that keeps the fleet
 *     generating for a dead run);
 *   - the scorer's app-under-test children, by the RUN-UNIQUE db PATH PREFIX `<workdir>/graded`,
 *     because the flag and the file name differ per tier (gate 8 refutation of the first cut,
 *     2026-09-02): the sb-6 scorer spawns `vendorsync --db <workdir>/graded.db`, while on sb-7
 *     run_build.py:291 sets the db to `<workdir>/graded-sb7-db` and score_sb7.py spawns
 *     `app.ledgerd` / `app.notifierd` with `--db-dir <workdir>/graded-sb7-db` (start_new_session,
 *     so no group we own reaches them). Matching `--db <workdir>/graded.db` alone left ledgerd and
 *     notifierd holding their ports after a cancel during sb-7 scoring;
 *   - score_sb7.py's two further app instances, `--db-dir <workdir>/sb7-empty-db` (the empty/D3
 *     probe) and `--db-dir <workdir>/sb7-combined-db` (the combined-entrypoint smoke) — the same
 *     class, alive during the scoring window a cancel can land in.
 * `<workdir>/graded` matches `<workdir>/graded.db` and `<workdir>/graded-sb7-db` and nothing under a
 * sibling workdir (`<workdir>b/graded…` fails the slash). `goose serve --platform desktop --host
 * 127.0.0.1 --port N` carries none of these.
 */
export function benchRunArgvTokens(workdir: string): string[] {
  return [
    `--log-file ${path.join(workdir, 'run.jsonl')}`,
    path.join(workdir, 'graded'),
    path.join(workdir, 'sb7-empty-db'),
    path.join(workdir, 'sb7-combined-db'),
  ];
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
