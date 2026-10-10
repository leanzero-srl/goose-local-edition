import { execFile } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import { parseCallBudget, telemetryEntrantCalls } from './benchTray';

/**
 * Where a single-model entrant's telemetry sink lives while it runs (run_build.py): an isolated tier
 * moves it beside the tree, into bench_isolation.prepare's runtime dir `<parent>/.<workdir>-runtime/`,
 * because the entrant may clean its own tree; every other entrant writes `<workdir>/.swarm/`. The
 * isolated sink is read first; the tree's copy is where land_telemetry puts it after the entrant exits.
 */
export function benchTelemetryCandidates(workdir: string): string[] {
  return [
    path.join(path.dirname(workdir), `.${path.basename(workdir)}-runtime`, 'telemetry.jsonl'),
    path.join(workdir, '.swarm', 'telemetry.jsonl'),
  ];
}

/**
 * Counts the entrant's calls incrementally: only bytes appended since the last read are parsed, up to
 * the last whole line. The sink's identity is path + inode + birthtime (never path + size — a file
 * replaced by a longer one reads as an append, .claude/rules/swarm-reader.md); a sink that moved, was
 * replaced or shrank restarts the count from its first byte.
 */
export class BenchTelemetryCounter {
  private identity: string | null = null;
  private offset = 0;
  private calls = 0;

  constructor(
    private readonly candidates: string[],
    private readonly model: string
  ) {}

  /** The count so far, or null when no sink exists yet (nothing is claimed before it does). */
  async read(): Promise<number | null> {
    for (const candidate of this.candidates) {
      let size: number;
      let identity: string;
      try {
        const stat = await fs.stat(candidate);
        size = stat.size;
        identity = `${candidate}:${stat.ino}:${stat.birthtimeMs}`;
      } catch {
        continue;
      }
      if (identity !== this.identity || size < this.offset) {
        this.identity = identity;
        this.offset = 0;
        this.calls = 0;
      }
      if (size > this.offset) {
        const handle = await fs.open(candidate, 'r');
        try {
          const chunk = Buffer.alloc(size - this.offset);
          const { bytesRead } = await handle.read(chunk, 0, chunk.length, this.offset);
          const end = chunk.subarray(0, bytesRead).lastIndexOf(0x0a);
          if (end >= 0) {
            this.calls += telemetryEntrantCalls(
              chunk.subarray(0, end).toString('utf8'),
              this.model
            );
            this.offset += end + 1;
          }
        } finally {
          await handle.close();
        }
      }
      return this.calls;
    }
    return null;
  }
}

/** The isolated tier's own call_budget from the payload's Python (isolated_tiers — bench_budget.CALL_BUDGET
 *  unless the tier publishes its own, as forge-2.0 does), the one source of the number; null on any failure. */
export function readBenchCallBudget(
  python: string,
  benchDir: string,
  env: NodeJS.ProcessEnv,
  scorerVersion: string
): Promise<number | null> {
  return new Promise((resolve) => {
    execFile(
      python,
      [
        '-B',
        '-c',
        'import sys, isolated_tiers; print(isolated_tiers.BY_VERSION[sys.argv[1]].call_budget)',
        scorerVersion,
      ],
      { cwd: benchDir, env: { ...env, PYTHONDONTWRITEBYTECODE: '1' } },
      (error, stdout) => resolve(error ? null : parseCallBudget(String(stdout)))
    );
  });
}
