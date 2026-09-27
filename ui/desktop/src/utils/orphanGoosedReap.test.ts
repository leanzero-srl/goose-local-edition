import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
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
// an app that died without stopping its goosed leaves. The stand-in is a compiled `pause()` at
// `<tmp>/Goose Swarm.app/Contents/Resources/bin/goose` (a space in the path, like the real bundle):
// a binary, so `ps` shows ITS path first (a script would show its interpreter), and one that leaves
// its argv alone (macOS /usr/bin/yes rewrites its argv buffer and `ps` then prints garbage).
// macOS only: Linux may reparent orphans to a subreaper, not pid 1, and the proof then (rightly)
// keeps them.
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

const buildStandIn = () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'q223-'));
  const bin = path.join(dir, 'Goose Swarm.app', 'Contents', 'Resources', 'bin');
  fs.mkdirSync(bin, { recursive: true });
  const source = path.join(dir, 'pause.c');
  fs.writeFileSync(source, '#include <unistd.h>\nint main(void) { for (;;) pause(); }\n');
  execFileSync('cc', ['-o', path.join(bin, 'goose'), source]);
  return { dir, goose: path.join(bin, 'goose') };
};

const orphanPidsOf = (marker: string): number[] =>
  execFileSync('ps', ['-axo', 'pid=,ppid=,args='], { encoding: 'utf8' })
    .split('\n')
    .flatMap((line) => {
      const m = /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line);
      return m && m[2] === '1' && m[3].includes(marker) ? [Number(m[1])] : [];
    });

const spawnOrphan = async (prefix: string, marker: string): Promise<number> => {
  execFileSync('sh', [
    '-c',
    `${prefix} exec "$0" ${desktopArgs.join(' ')} ${marker} >/dev/null 2>&1 &`,
    STAND_IN,
  ]);
  for (let i = 0; i < 100; i += 1) {
    const pids = orphanPidsOf(marker);
    if (pids.length === 1) return pids[0];
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  throw new Error(`the orphan ${marker} never appeared with ppid 1`);
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
  const started: number[] = [];
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
  });

  it('stops this bundle’s orphan with SIGTERM and leaves a live app’s goosed alone', async () => {
    const marker = `--q223-term-${process.pid}-${Date.now()}`;
    const orphan = await spawnOrphan('', marker);
    started.push(orphan);
    const owned = spawn(STAND_IN, [...desktopArgs, marker], { stdio: 'ignore' });
    children.push(owned);
    const logger = { info: vi.fn(), error: vi.fn() };

    const report = await reapOrphanedGoosed({ goosePath: STAND_IN, logger, sigkillAfterMs: 3000 });

    expect(report.reaped).toEqual([{ pid: orphan, signal: 'SIGTERM' }]);
    expect(report.survived).toEqual([]);
    expect(alive(orphan)).toBe(false);
    expect(owned.pid && alive(owned.pid)).toBe(true);
  });

  it('SIGKILLs, per pid, an orphan that ignores SIGTERM — only after the grace', async () => {
    const marker = `--q223-kill-${process.pid}-${Date.now()}`;
    const orphan = await spawnOrphan("trap '' TERM;", marker);
    started.push(orphan);
    const logger = { info: vi.fn(), error: vi.fn() };

    const before = Date.now();
    const report = await reapOrphanedGoosed({ goosePath: STAND_IN, logger, sigkillAfterMs: 400 });

    expect(Date.now() - before).toBeGreaterThanOrEqual(400);
    expect(report.reaped).toEqual([{ pid: orphan, signal: 'SIGKILL' }]);
    expect(alive(orphan)).toBe(false);
  });
});
