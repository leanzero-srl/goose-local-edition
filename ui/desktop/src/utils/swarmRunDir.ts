import fs from 'node:fs/promises';
import path from 'node:path';
import { expandTilde } from './pathUtils';

export interface ResolvedSwarmDir {
  /** The `.swarm` directory this run's files actually live in. */
  swarmDir: string;
  /** The run named by the breadcrumb, when there is one. */
  pinnedRunId: string | null;
  /** `run-<id>.jsonl` for {@link pinnedRunId}. */
  pinnedRunFile: string | null;
  /** A breadcrumb was present. Runs older than the breadcrumb mechanism have none, and the
   *  benchmark layout (a `run.jsonl` beside `.swarm`) is only trusted when one exists. */
  hadBreadcrumb: boolean;
}

/**
 * Where a working directory's CURRENT swarm run lives.
 *
 * The engine writes `.swarm/current-run.json` {run_id, dir} at the START of every run, in the
 * directory it was spawned in. FOLLOW IT rather than guessing. Guessing "newest run-*.jsonl in this
 * dir" had two failure modes: it re-rendered a FINISHED run from hours ago as the live panel the
 * moment a new turn began, and it could not see a run at all once the engine redirected the build
 * out of the spawn dir (it refuses to treat $HOME as an app tree).
 *
 * Shared by every main-process consumer — the run reader and the fs.watch push — so the two can
 * never disagree about which run is live.
 */
export async function resolveSwarmDir(workingDir: string): Promise<ResolvedSwarmDir> {
  const swarmDir = path.join(expandTilde(workingDir), '.swarm');
  const out: ResolvedSwarmDir = {
    swarmDir,
    pinnedRunId: null,
    pinnedRunFile: null,
    hadBreadcrumb: false,
  };
  try {
    const ptr = JSON.parse(await fs.readFile(path.join(swarmDir, 'current-run.json'), 'utf8')) as {
      run_id?: string;
      dir?: string;
    };
    out.hadBreadcrumb = true;
    if (ptr?.dir) {
      const dir = path.join(expandTilde(ptr.dir), '.swarm');
      const redirected = await fs
        .stat(dir)
        .then(() => true)
        .catch(() => false);
      if (redirected) out.swarmDir = dir;
    }
    if (ptr?.run_id) {
      out.pinnedRunId = ptr.run_id;
      out.pinnedRunFile = `run-${ptr.run_id}.jsonl`;
    }
  } catch {
    /* no breadcrumb (older run, or the engine has not written it yet) — the spawn dir stands */
  }
  return out;
}
