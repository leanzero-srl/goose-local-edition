import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

/**
 * At launch, before this app spawns its own goosed: stop — per pid, on proof — every goosed an
 * EARLIER run of this same app left behind (Q-223).
 *
 * Measured 2026-09-27: the 3.0.61 goosed (pid 10891, `…/Goose Swarm.app/Contents/Resources/bin/goose
 * serve --tls --platform desktop --host 127.0.0.1 --port 54154`) survived its app's quit as an orphan
 * (ppid 1, 28–37% CPU for 1h22m) and kept its tailscaled, so the 3.0.62 app's Link could not come back
 * ("another goose on this Mac already holds the mesh"). goosed now exits when its app's end of stdin
 * closes, but a goosed built before that — or one whose app was SIGKILLed before the fix shipped — does
 * not, and nothing else will ever stop it.
 *
 * THE PROOF (all of it, per process, pure in `selectOrphanedGoosed`):
 * 1. its parent is launchd/init (ppid 1) — the app that spawned it is gone. The desktop is always
 *    goosed's direct parent (spawned with no shell), so a live app's goosed never has ppid 1;
 * 2. it runs as this user;
 * 3. its command line is THIS bundle's goose binary running `serve` in the desktop's own shape
 *    (`<goosePath> serve … --platform desktop … --host 127.0.0.1`) — a goosed from another bundle
 *    path (a worktree build, another install) is named in the log and left alone;
 * 4. it is not this process.
 * Before each signal the pid is re-read and must still be that process (same start time, still
 * ppid 1, same command line). SIGTERM first — goosed's own teardown stops its mesh daemon and its
 * engine, which is what frees Link — then SIGKILL on the pid only if it outlived the same grace the
 * app gives its own goosed on quit. Never a group kill (gate 4).
 */

export interface OrphanedGoosed {
  pid: number;
  startedAt: string;
  args: string;
}

export interface OrphanReapReport {
  reaped: { pid: number; signal: 'SIGTERM' | 'SIGKILL' }[];
  survived: number[];
  otherBundles: OrphanedGoosed[];
}

export interface OrphanReapLogger {
  info: (...args: unknown[]) => void;
  error: (...args: unknown[]) => void;
}

// `ps -o lstart=` is `%a %b %e %T %Y` ("Sun Sep 27 20:49:26 2026"; %e pads a one-digit day).
const PS_ROW =
  /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\w{3}\s+\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s+(.*)$/;

interface PsRow {
  pid: number;
  ppid: number;
  uid: number;
  startedAt: string;
  args: string;
}

const parsePs = (output: string): PsRow[] =>
  output.split('\n').flatMap((line) => {
    const m = PS_ROW.exec(line);
    return m
      ? [
          {
            pid: Number(m[1]),
            ppid: Number(m[2]),
            uid: Number(m[3]),
            startedAt: m[4].replace(/\s+/g, ' '),
            args: m[5],
          },
        ]
      : [];
  });

const isDesktopServe = (args: string): boolean =>
  / serve( |$)/.test(args) &&
  args.includes(' --platform desktop') &&
  args.includes(' --host 127.0.0.1');

/**
 * The proof as a pure function over `ps -axo pid=,ppid=,uid=,lstart=,args=` output. `mine` are the
 * orphans this app may stop; `otherBundles` are orphaned desktop goosed of ANOTHER bundle path, named
 * so the log says they exist, never touched.
 */
export function selectOrphanedGoosed(
  psOutput: string,
  goosePath: string,
  uid: number,
  selfPid: number
): { mine: OrphanedGoosed[]; otherBundles: OrphanedGoosed[] } {
  const mine: OrphanedGoosed[] = [];
  const otherBundles: OrphanedGoosed[] = [];
  for (const row of parsePs(psOutput)) {
    if (row.ppid !== 1 || row.uid !== uid || row.pid === selfPid || row.pid <= 1) continue;
    if (!isDesktopServe(row.args)) continue;
    const found = { pid: row.pid, startedAt: row.startedAt, args: row.args };
    if (row.args.startsWith(`${goosePath} serve `)) mine.push(found);
    else if (/\/goose serve /.test(row.args)) otherBundles.push(found);
  }
  return { mine, otherBundles };
}

const execFileAsync = promisify(execFile);
const PS_TIMEOUT_MS = 4000;

const psTable = async (pids?: number[]): Promise<string> => {
  const select = pids ? ['-p', pids.join(',')] : ['-ax'];
  try {
    // -ww: never cut a long bundle path; LC_ALL=C: `lstart` in the one format PS_ROW reads.
    const { stdout } = await execFileAsync(
      'ps',
      ['-ww', ...select, '-o', 'pid=,ppid=,uid=,lstart=,args='],
      {
        encoding: 'utf8',
        // The process table's own size is the bound: a fixed buffer is what a busy Mac overflows
        // (Q-286 — one engine's argv measured 262 KB, and `ps -ax` passed 1 MiB under a build).
        maxBuffer: Infinity,
        env: { ...process.env, LC_ALL: 'C' },
        // The launch awaits this scan: a `ps` that never answers must not hold the app. Transport,
        // like the benchmark cancel's ps (main.ts) — it bounds a system call, never model work.
        timeout: PS_TIMEOUT_MS,
      }
    );
    return stdout;
  } catch (error) {
    // `ps -p` exits 1 when none of the pids exist — that IS the answer (all gone). Anything else
    // (a timeout, a signal) is not an answer and must not read as "gone".
    const failed = error as { code?: unknown; stdout?: string };
    if (pids && failed.code === 1 && typeof failed.stdout === 'string') return failed.stdout;
    throw error;
  }
};

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** The per-pid grace tick — the same 100 ms leg goosed's own supervisors poll on. */
const REAP_POLL_MS = 100;

export async function reapOrphanedGoosed(options: {
  goosePath: string;
  logger: OrphanReapLogger;
  /** How long an orphan's own teardown may take before SIGKILL: the app's goosed quit grace. */
  sigkillAfterMs: number;
}): Promise<OrphanReapReport> {
  const { goosePath, logger, sigkillAfterMs } = options;
  const report: OrphanReapReport = { reaped: [], survived: [], otherBundles: [] };
  if (process.platform === 'win32' || typeof process.getuid !== 'function') {
    logger.info('[orphan-goosed] not looked for: the ps/ppid proof is POSIX-only');
    return report;
  }
  const uid = process.getuid();
  const { mine, otherBundles } = selectOrphanedGoosed(await psTable(), goosePath, uid, process.pid);
  report.otherBundles = otherBundles;
  for (const other of otherBundles) {
    logger.info(
      `[orphan-goosed] left alone: pid ${other.pid} is an orphaned goosed of ANOTHER bundle (${other.args}) — not this app's to stop`
    );
  }
  if (mine.length === 0) {
    logger.info(`[orphan-goosed] none from ${goosePath}`);
    return report;
  }

  const stillThere = async (targets: OrphanedGoosed[]): Promise<OrphanedGoosed[]> => {
    if (targets.length === 0) return [];
    const now = selectOrphanedGoosed(
      await psTable(targets.map((t) => t.pid)),
      goosePath,
      uid,
      process.pid
    ).mine;
    return targets.filter((t) =>
      now.some((n) => n.pid === t.pid && n.startedAt === t.startedAt && n.args === t.args)
    );
  };

  const signal = (targets: OrphanedGoosed[], sig: 'SIGTERM' | 'SIGKILL') => {
    for (const t of targets) {
      try {
        process.kill(t.pid, sig);
        logger.info(
          `[orphan-goosed] ${sig} pid ${t.pid} (started ${t.startedAt}, parent gone): ${t.args}`
        );
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ESRCH') {
          logger.error(`[orphan-goosed] ${sig} pid ${t.pid} failed: ${String(error)}`);
        }
      }
    }
  };

  const waitGone = async (targets: OrphanedGoosed[]): Promise<OrphanedGoosed[]> => {
    let left = targets;
    for (let waited = 0; left.length > 0 && waited < sigkillAfterMs; waited += REAP_POLL_MS) {
      await sleep(REAP_POLL_MS);
      left = await stillThere(left);
    }
    return left;
  };

  const termed = await stillThere(mine);
  signal(termed, 'SIGTERM');
  const ignoredTerm = await waitGone(termed);
  const killed = await stillThere(ignoredTerm);
  signal(killed, 'SIGKILL');
  const survivors = await waitGone(killed);

  for (const t of termed) {
    if (survivors.some((s) => s.pid === t.pid)) continue;
    report.reaped.push({
      pid: t.pid,
      signal: ignoredTerm.some((k) => k.pid === t.pid) ? 'SIGKILL' : 'SIGTERM',
    });
  }
  report.survived = survivors.map((s) => s.pid);
  if (report.survived.length > 0) {
    logger.error(
      `[orphan-goosed] still running after SIGTERM and SIGKILL: ${report.survived.join(', ')}`
    );
  }
  logger.info(
    `[orphan-goosed] stopped ${report.reaped.length} orphaned goosed of ${goosePath}: ${
      report.reaped.map((r) => `${r.pid} (${r.signal})`).join(', ') || 'none'
    }`
  );
  return report;
}
