import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import fs from 'node:fs';
import type { Socket } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { reapOrphanedGoosed, selectOrphanedGoosed } from './orphanGoosedReap';

const APP_GOOSE = '/Applications/Goose Swarm.app/Contents/Resources/bin/goose';
const UID = 501;
const SELF = 4242;

// `ps -axo pid=,ppid=,uid=,lstart=,args=` as measured on the owner's MacBook, 2026-09-27 22:10.
const PS = [
  // Q-223: the 3.0.61 goosed, orphaned — the one to stop.
  `10891     1   501 Sun Sep 27 20:49:26 2026     ${APP_GOOSE} serve --tls --platform desktop --host 127.0.0.1 --port 54154`,
  // The live app's own goosed: its parent (the app) is alive.
  ` 2352  2190   501 Sun Sep 27 22:08:23 2026     ${APP_GOOSE} serve --tls --platform desktop --host 127.0.0.1 --port 50192 --exit-when-stdin-closes`,
  // An agent's packaged build, orphaned too — another bundle path.
  `31207     1   501 Sun Sep 27 16:09:02 2026     /Users/mihaiperdum/Projects/goose/.claude/worktrees/agent-x/ui/desktop/out/Goose Swarm-darwin-arm64/Goose Swarm.app/Contents/Resources/bin/goose serve --tls --platform desktop --host 127.0.0.1 --port 61011`,
  // Its tailscaled — not a goosed at all.
  `11275 10891   501 Sun Sep 27 20:49:29 2026     /Applications/Goose Swarm.app/Contents/Resources/bin/tailscaled --tun=userspace-networking --statedir=/Users/mihaiperdum/.leanzero/tailscale`,
  // Same binary, orphaned, but not the desktop's shape (a terminal `goose serve` under nohup).
  ` 7001     1   501 Sat Sep  5 09:01:00 2026     ${APP_GOOSE} serve --port 3284`,
  // Another user's orphaned desktop goosed.
  ` 7002     1   502 Sun Sep 27 20:00:00 2026     ${APP_GOOSE} serve --tls --platform desktop --host 127.0.0.1 --port 1`,
  // The same binary running `swarm run`, orphaned — not `serve`.
  ` 7003     1   501 Sun Sep 27 20:00:00 2026     ${APP_GOOSE} swarm run --log-file /tmp/x/run.jsonl --platform desktop --host 127.0.0.1`,
].join('\n');

describe('selectOrphanedGoosed — the proof, as a pure function (Q-223)', () => {
  it('selects only this bundle’s orphaned desktop goosed of this user', () => {
    const { mine, otherBundles } = selectOrphanedGoosed(PS, APP_GOOSE, UID, SELF);
    expect(mine).toEqual([
      {
        pid: 10891,
        startedAt: 'Sun Sep 27 20:49:26 2026',
        args: `${APP_GOOSE} serve --tls --platform desktop --host 127.0.0.1 --port 54154`,
      },
    ]);
    expect(otherBundles.map((o) => o.pid)).toEqual([31207]);
  });

  it('never selects this process, init, or a goosed whose app is alive', () => {
    const self = `${SELF}     1   501 Sun Sep 27 20:49:26 2026     ${APP_GOOSE} serve --platform desktop --host 127.0.0.1 --port 1`;
    const init = `    1     1   501 Sun Sep 27 20:49:26 2026     ${APP_GOOSE} serve --platform desktop --host 127.0.0.1 --port 1`;
    expect(selectOrphanedGoosed([self, init].join('\n'), APP_GOOSE, UID, SELF).mine).toEqual([]);
    expect(selectOrphanedGoosed(PS, APP_GOOSE, UID, SELF).mine.some((o) => o.pid === 2352)).toBe(
      false
    );
  });

  it('normalises the space-padded day so the identity re-check compares equal', () => {
    const row = ` 7004     1   501 Sat Sep  5 09:01:00 2026     ${APP_GOOSE} serve --platform desktop --host 127.0.0.1 --port 9`;
    expect(selectOrphanedGoosed(row, APP_GOOSE, UID, SELF).mine[0].startedAt).toBe(
      'Sat Sep 5 09:01:00 2026'
    );
  });
});

// Real processes: `sh` backgrounds the stand-in and exits, so launchd adopts it (ppid 1) — the shape
// an app that died without stopping its goosed leaves. The stand-in is a compiled program at
// `<tmp>/Goose Swarm.app/Contents/Resources/bin/goose` (a space in the path, like the real bundle):
// a binary, so `ps` shows ITS path first (a script would show its interpreter), and one that leaves
// its argv alone (macOS /usr/bin/yes rewrites its argv buffer and `ps` then prints garbage).
// macOS only: Linux may reparent orphans to a subreaper, not pid 1, and the proof then (rightly)
// keeps them.
//
// Q-243: an orphan is by design nobody's child, so nothing ends it when the test run dies — a
// SIGTERM-ignoring one leaked as ppid 1 for 12 minutes (pid 95645) when a run was interrupted under
// load. So the stand-in carries its own LIFELINE: it watches fd 3, the far end of a socket this
// vitest worker holds, and exits the moment that end closes — the kernel closes the worker's end
// however the worker ends (after hooks, a timeout, ^C, SIGKILL). Per-pid kills in afterEach stay the
// first line; the lifeline is what holds when afterEach never runs.
//
// Q-286: every wait here is on an EVENT the stand-in or the kernel produces, never on a window. The
// stand-in's fd 4 is its WITNESS, the far end of another socket this worker holds: it writes its pid
// there once it runs as the goose binary (after its exec — the `ps` rows the reap reads show the
// desktop's shape from then on), answers every byte written there with the same byte (a live,
// polling process: the round trip proves it alive NOW, and — after a signal was sent — that the
// signal did not end it), and the witness ENDS when the kernel closes the stand-in's last copy, i.e.
// when it has exited. Measured under load before this (2026-09-28): the marker scan
// `ps -axo pid=,ppid=,args=` overflowed execFileSync's 1 MiB default buffer while a release build
// and an engine ran (ENOBUFS — all four tests red at once), and the scan could find the pid while it
// was still the forked `sh` (its args not yet the goose binary's), so the reap found nothing.
let STAND_IN = '';
const desktopArgs = [
  'serve',
  '--tls',
  '--platform',
  'desktop',
  '--host',
  '127.0.0.1',
  '--port',
  '1',
];

const STAND_IN_SOURCE = `#include <errno.h>
#include <poll.h>
#include <stdio.h>
#include <unistd.h>
int main(void) {
  char b, line[32];
  int n = snprintf(line, sizeof line, "%d\\n", (int)getpid());
  if (write(4, line, n) != n) return 1;
  struct pollfd fds[2] = {{3, POLLIN, 0}, {4, POLLIN, 0}};
  for (;;) {
    if (poll(fds, 2, -1) < 0) {
      if (errno == EINTR) continue;
      return 1;
    }
    if (fds[0].revents && read(3, &b, 1) <= 0) return 0;
    if (fds[1].revents) {
      if (read(4, &b, 1) <= 0) return 0;
      if (write(4, &b, 1) != 1) return 1;
    }
  }
}
`;

const buildStandIn = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'q223-'));
  const bin = path.join(dir, 'Goose Swarm.app', 'Contents', 'Resources', 'bin');
  fs.mkdirSync(bin, { recursive: true });
  const source = path.join(dir, 'stand-in.c');
  fs.writeFileSync(source, STAND_IN_SOURCE);
  execFileSync('cc', ['-o', path.join(bin, 'goose'), source]);
  return { dir, goose: path.join(bin, 'goose') };
};

/** A stand-in's fds: 0–2 closed, 3 its lifeline, 4 its witness — both sockets this worker holds. */
const STAND_IN_STDIO: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] = [
  'ignore',
  'ignore',
  'ignore',
  'pipe',
  'pipe',
];

/** `sh` argv that backgrounds the stand-in as an orphan; fds 3 and 4 are inherited. */
const orphanShellArgs = (prefix: string, marker: string): string[] => [
  '-c',
  `${prefix} exec "$0" ${desktopArgs.join(' ')} ${marker} >/dev/null 2>&1 &`,
  STAND_IN,
];

/** This worker's end of a stand-in's witness (its fd 4). */
class Witness {
  private text = '';
  private ended = false;
  private waiters: (() => void)[] = [];
  /** The kernel closed the stand-in's last copy: it has exited. */
  readonly exited: Promise<void>;

  constructor(private readonly end: Socket) {
    // Held, and never what keeps the worker alive.
    end.unref();
    end.setEncoding('utf8');
    end.on('data', (chunk: string) => {
      this.text += chunk;
      this.wake();
    });
    this.exited = new Promise((resolve) => {
      const over = () => {
        this.ended = true;
        this.wake();
        resolve();
      };
      end.once('end', over);
      end.once('close', over);
    });
  }

  private wake() {
    for (const waiter of this.waiters.splice(0)) waiter();
  }

  private async until(done: () => boolean, what: string): Promise<void> {
    while (!done()) {
      if (this.ended)
        throw new Error(
          `the stand-in exited before ${what}; its witness read ${JSON.stringify(this.text)}`
        );
      await new Promise<void>((resolve) => this.waiters.push(resolve));
    }
  }

  /** The pid the stand-in wrote once it ran as the goose binary. */
  async pid(): Promise<number> {
    await this.until(() => this.text.includes('\n'), 'it reported its pid');
    return Number(this.text.slice(0, this.text.indexOf('\n')));
  }

  /** A byte out and the same byte back: the stand-in is alive and polling now. */
  async roundTrip(): Promise<void> {
    const before = this.text.length;
    this.end.write('.');
    await this.until(() => this.text.length > before, 'it answered a round trip');
  }

  destroy() {
    this.end.destroy();
  }
}

// Every lifeline and witness this worker holds and every stand-in pid it saw — afterEach reads all.
const lifelines: Socket[] = [];
const witnesses: Witness[] = [];
const started: number[] = [];

const holdLifeline = (child: ChildProcess) => {
  const end = child.stdio[3] as Socket;
  // Held, never read, and never what keeps the worker alive.
  end.unref();
  lifelines.push(end);
};

const holdWitness = (end: Socket): Witness => {
  const witness = new Witness(end);
  witnesses.push(witness);
  return witness;
};

const ppidOf = (pid: number): number =>
  Number(execFileSync('ps', ['-o', 'ppid=', '-p', String(pid)], { encoding: 'utf8' }).trim());

/** Whether `pid` is a live process running with `marker` — an exited one (a zombie launchd has not
 * reaped yet) is not. */
const runsWith = (pid: number, marker: string): boolean => {
  try {
    const row = execFileSync('ps', ['-ww', '-o', 'stat=,args=', '-p', String(pid)], {
      encoding: 'utf8',
    }).trim();
    return !row.startsWith('Z') && row.includes(marker);
  } catch (error) {
    // `ps -p` exits 1 when the pid is gone — that IS the answer; anything else is not one.
    if ((error as { status?: unknown }).status === 1) return false;
    throw error;
  }
};

interface StandIn {
  pid: number;
  witness: Witness;
}

const spawnOrphan = async (prefix: string, marker: string): Promise<StandIn> => {
  const sh = spawn('sh', orphanShellArgs(prefix, marker), { stdio: STAND_IN_STDIO });
  const shExited = once(sh, 'exit');
  holdLifeline(sh);
  const witness = holdWitness(sh.stdio[4] as Socket);
  const pid = await witness.pid();
  // Recorded the moment it is known, so a test that fails or times out after this still reaps it.
  started.push(pid);
  // The `sh` that forked it has exited, so launchd has adopted it.
  await shExited;
  expect(ppidOf(pid)).toBe(1);
  return { pid, witness };
};

const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

describe.skipIf(process.platform !== 'darwin')('reapOrphanedGoosed — real processes', () => {
  const children: ChildProcess[] = [];
  let standInDir = '';
  beforeAll(() => {
    const built = buildStandIn();
    standInDir = built.dir;
    STAND_IN = built.goose;
  });
  afterAll(() => fs.rmSync(standInDir, { recursive: true, force: true }));
  afterEach(() => {
    for (const pid of started.splice(0)) if (alive(pid)) process.kill(pid, 'SIGKILL');
    for (const child of children.splice(0)) child.kill('SIGKILL');
    for (const end of lifelines.splice(0)) end.destroy();
    for (const witness of witnesses.splice(0)) witness.destroy();
  });

  it('stops this bundle’s orphan with SIGTERM and leaves a live app’s goosed alone', async () => {
    const marker = `--q223-term-${process.pid}-${Date.now()}`;
    const orphan = await spawnOrphan('', marker);
    const owned = spawn(STAND_IN, [...desktopArgs, marker], { stdio: STAND_IN_STDIO });
    children.push(owned);
    holdLifeline(owned);
    const ownedWitness = holdWitness(owned.stdio[4] as Socket);
    // The live app's goosed runs as the goose binary before the scan looks.
    expect(await ownedWitness.pid()).toBe(owned.pid);
    const logger = { info: vi.fn(), error: vi.fn() };

    const report = await reapOrphanedGoosed({ goosePath: STAND_IN, logger, sigkillAfterMs: 3000 });

    expect(report.reaped).toEqual([{ pid: orphan.pid, signal: 'SIGTERM' }]);
    expect(report.survived).toEqual([]);
    await orphan.witness.exited;
    await ownedWitness.roundTrip();
  });

  it('SIGKILLs, per pid, an orphan that ignores SIGTERM — only after the grace', async () => {
    const marker = `--q223-kill-${process.pid}-${Date.now()}`;
    const orphan = await spawnOrphan("trap '' TERM;", marker);
    const logger = { info: vi.fn(), error: vi.fn() };

    const before = Date.now();
    const report = await reapOrphanedGoosed({ goosePath: STAND_IN, logger, sigkillAfterMs: 400 });

    expect(Date.now() - before).toBeGreaterThanOrEqual(400);
    expect(report.reaped).toEqual([{ pid: orphan.pid, signal: 'SIGKILL' }]);
    await orphan.witness.exited;
  });

  it('Q-243: a SIGTERM-ignoring stand-in dies with the run that made it, even when no after hook runs', async () => {
    // The run is a separate process here, so it can be SIGKILLed the way an interrupted vitest worker
    // dies: no afterEach, no afterAll. It makes the orphan exactly as spawnOrphan does — the lifeline
    // its own socket, the witness passed through from this worker (the run's fd 3, closed in the run
    // once handed on, so the witness ends with the stand-in alone) — and reports once the `sh` that
    // backgrounded it is gone; its own stdin watch ends it if THIS worker dies.
    const marker = `--q243-abort-${process.pid}-${Date.now()}`;
    const run = spawn(
      process.execPath,
      [
        '-e',
        `process.stdin.on('end', () => process.exit(0)); process.stdin.resume();
         const { spawn } = require('node:child_process');
         const sh = spawn('sh', JSON.parse(process.argv[1]), { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 3] });
         require('node:fs').closeSync(3);
         sh.on('exit', () => console.log('spawned'));`,
        JSON.stringify(orphanShellArgs("trap '' TERM;", marker)),
      ],
      { stdio: ['pipe', 'pipe', 'ignore', 'pipe'] }
    );
    children.push(run);
    const witness = holdWitness(run.stdio[3] as Socket);
    const spawned = once(run.stdout!, 'data');
    const orphan = await witness.pid();
    started.push(orphan);
    const [line] = await spawned;
    expect(String(line)).toContain('spawned');
    expect(ppidOf(orphan)).toBe(1);
    // It is the case that leaked: an orphan (ppid 1) that ignores SIGTERM — it answers after one.
    process.kill(orphan, 'SIGTERM');
    await witness.roundTrip();

    const exited = once(run, 'exit');
    process.kill(run.pid!, 'SIGKILL');
    await exited;

    await witness.exited;
    expect(runsWith(orphan, marker)).toBe(false);
  });

  it('Q-243: the run lives on while its worker holds the lifeline', async () => {
    const marker = `--q243-held-${process.pid}-${Date.now()}`;
    const orphan = await spawnOrphan('', marker);
    await orphan.witness.roundTrip();
    // Dropping this worker's end is the worker dying, as far as the stand-in can tell.
    lifelines.splice(0).forEach((end) => end.destroy());
    await orphan.witness.exited;
    expect(runsWith(orphan.pid, marker)).toBe(false);
  });
});
