import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';

/**
 * Whether an LM Studio service is ALREADY running, answered without waking one.
 *
 * `lms ps` is not a liveness check: when no LM Studio process answers, the CLI relaunches the
 * service from `<home>/.internal/app-install-location.json` (`LM Studio --run-as-service`), and that
 * service can JIT-load a model. Measured on Gabee's Mac 2026-10-16: the Goose app (pid 71539, a
 * Forge 2.0 benchmark on the app's own MLX engine) was the parent of `~/.lmstudio/bin/lms ps --json`,
 * every quit of LM Studio was undone by the next poll, and the relaunched service held a ~35B model
 * beside the MLX engine running the benchmark.
 *
 * The running service advertises itself in `<home>/.internal/http-server.json` — `{host, pid, port}`,
 * the internal API `lms` talks to (port 41343 on this laptop, beside the OpenAI server's 1234). The
 * check is: that file parses, its pid is alive (`kill(pid, 0)` sends nothing), and its port accepts a
 * TCP connection. The port leg covers a stale file whose pid the OS has since reused. Nothing here
 * spawns a process. `<home>` is `~/.lmstudio` unless `~/.lmstudio-home-pointer` names another
 * directory, the same pointer LM Studio writes.
 */

export type LmStudioLiveness =
  | { live: true; host: string; pid: number; port: number }
  | { live: false; reason: 'no-service-file' | 'bad-service-file' | 'pid-gone' | 'port-closed' };

export interface LmStudioLivenessDeps {
  home: string;
  /** File contents, or null when the file cannot be read. */
  readFile: (p: string) => string | null;
  pidAlive: (pid: number) => boolean;
  tcpAccepts: (host: string, port: number) => Promise<boolean>;
}

export function lmStudioHome(home: string, readFile: (p: string) => string | null): string {
  const pointed = readFile(path.join(home, '.lmstudio-home-pointer'))?.trim();
  return pointed ? pointed : path.join(home, '.lmstudio');
}

export async function lmStudioLiveness(d: LmStudioLivenessDeps): Promise<LmStudioLiveness> {
  const file = path.join(lmStudioHome(d.home, d.readFile), '.internal', 'http-server.json');
  const raw = d.readFile(file);
  if (raw == null) return { live: false, reason: 'no-service-file' };
  let parsed: { host?: unknown; pid?: unknown; port?: unknown };
  try {
    parsed = JSON.parse(raw) as typeof parsed;
  } catch {
    return { live: false, reason: 'bad-service-file' };
  }
  const { host, pid, port } = parsed;
  if (
    typeof host !== 'string' ||
    !host ||
    typeof pid !== 'number' ||
    !Number.isInteger(pid) ||
    pid <= 0 ||
    typeof port !== 'number' ||
    !Number.isInteger(port) ||
    port <= 0
  ) {
    return { live: false, reason: 'bad-service-file' };
  }
  if (!d.pidAlive(pid)) return { live: false, reason: 'pid-gone' };
  if (!(await d.tcpAccepts(host, port))) return { live: false, reason: 'port-closed' };
  return { live: true, host, pid, port };
}

/** `kill(pid, 0)` delivers no signal; ESRCH means gone, EPERM means alive under another user. */
export function pidAliveViaSignalZero(
  kill: (pid: number, signal: number) => unknown
): (pid: number) => boolean {
  return (pid) => {
    try {
      kill(pid, 0);
      return true;
    } catch (err) {
      return (err as NodeJS.ErrnoException)?.code === 'EPERM';
    }
  };
}

export function readFileOrNull(p: string): string | null {
  try {
    return fs.readFileSync(p, 'utf8');
  } catch {
    return null;
  }
}

/** A loopback connect either lands or is refused at once; the timeout only bounds a filtered port. */
export function tcpAcceptsViaNet(
  connectTimeoutMs: number
): (host: string, port: number) => Promise<boolean> {
  return (host, port) =>
    new Promise<boolean>((resolve) => {
      const socket = net.createConnection({ host, port });
      const done = (accepted: boolean) => {
        socket.destroy();
        resolve(accepted);
      };
      socket.setTimeout(connectTimeoutMs, () => done(false));
      socket.once('connect', () => done(true));
      socket.once('error', () => done(false));
    });
}

export type FleetStatusMap = Record<string, string>;

export interface FleetStatusDeps {
  /** The 'showLmStudioFleet' setting, read per call so a toggle takes effect on the next poll. */
  surfaceEnabled: () => boolean;
  resolveBin: () => string | null;
  liveness: () => Promise<LmStudioLiveness>;
  runPs: (bin: string) => Promise<FleetStatusMap>;
  log: (message: string) => void;
}

/**
 * The body of main's `fleet-status` IPC handler. `lms ps` is spawned only when the LM Studio fleet
 * surface is on AND a service is already running; every other state answers the honest `{}` (the
 * renderer's "LM Studio did not report" view) and is logged once per change of state, not per tick.
 */
export function fleetStatusHandler(d: FleetStatusDeps): () => Promise<FleetStatusMap> {
  let lastGate: string | null = null;
  const note = (gate: string, message: string) => {
    if (gate !== lastGate) d.log(message);
    lastGate = gate;
  };
  return async () => {
    if (!d.surfaceEnabled()) {
      note('surface-off', '[fleet-status] showLmStudioFleet is off: `lms ps` is not spawned');
      return {};
    }
    const bin = d.resolveBin();
    if (!bin) {
      note('lms-absent', '[fleet-status] lms not found: `lms ps` is not spawned');
      return {};
    }
    const live = await d.liveness();
    if (!live.live) {
      note(
        live.reason,
        `[fleet-status] LM Studio is not running (${live.reason}): \`lms ps\` is not spawned, because it would start the LM Studio service`
      );
      return {};
    }
    note(
      'live',
      `[fleet-status] LM Studio service live (pid ${live.pid}, port ${live.port}): polling \`lms ps\``
    );
    return d.runPs(bin);
  };
}

export function parseLmsPs(stdout: string): FleetStatusMap {
  const arr = JSON.parse(stdout) as Array<{ identifier?: string; status?: string }>;
  const map: FleetStatusMap = {};
  for (const m of arr) {
    if (m.identifier && m.status) map[m.identifier] = m.status;
  }
  return map;
}
