import { expect, it } from 'vitest';
import { pollUntil } from '../../test/testClock';
import { spawn, spawnSync, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { benchmarkCancellationPids } from '../benchReap';

// Q-243: both processes are detached (their own sessions — the shape under test), so nothing ends
// them if this run is interrupted before `finally`. Each exits when its stdin closes: the runner's
// stdin is this worker's pipe, the child's is the runner's — the kernel closes both ends however the
// worker and the runner die.
const followsStdin = `process.stdin.on('end',()=>process.exit(0)); process.stdin.resume();`;

const startRunner = () => {
  const child = `${followsStdin} setInterval(()=>{},1000)`;
  const code = `${followsStdin} const {spawn}=require('node:child_process'); const c=spawn(process.execPath,['-e',${JSON.stringify(child)}],{detached:true,stdio:['pipe','ignore','ignore']}); console.log(c.pid); setInterval(()=>{},1000);`;
  return spawn(process.execPath, ['-e', code], {
    detached: true,
    stdio: ['pipe', 'pipe', 'ignore'],
  });
};

const killEach = (pids: (number | undefined)[]) => {
  for (const pid of pids)
    if (pid)
      try {
        process.kill(pid, 'SIGKILL');
      } catch {
        /* The test already reaped this process. */
      }
};

const alive = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};

// One pid's state, not the full table: `ps -axo` overflowed execFileSync's buffer on a busy Mac (Q-290).
const goneOrZombie = (pid: number): boolean => {
  const stat = spawnSync('ps', ['-o', 'stat=', '-p', String(pid)], {
    encoding: 'utf8',
  }).stdout.trim();
  return stat === '' || stat.includes('Z');
};

it('captures and reaps a real cloud child in a separate session before its runner', async () => {
  const runner = startRunner();
  let childPid: number | undefined;
  try {
    const [line] = await once(runner.stdout!, 'data');
    childPid = Number(String(line).trim());
    const snapshot = execFileSync('ps', ['-axo', 'pid=,ppid=,args='], {
      encoding: 'utf8',
      // Q-290: a busy Mac's full table overflows the default 1 MiB (ENOBUFS).
      maxBuffer: Infinity,
    });
    const pids = benchmarkCancellationPids(snapshot, runner.pid!, '/not-a-real-run', process.pid);
    expect(pids).toContain(childPid);
    expect(pids[pids.length - 1]).toBe(runner.pid);
    expect(pids).not.toContain(process.pid);
    const groups = execFileSync('ps', ['-o', 'pgid=', '-p', `${runner.pid},${childPid}`], {
      encoding: 'utf8',
    })
      .trim()
      .split(/\s+/);
    expect(new Set(groups).size).toBe(2);
    // The child first, while its runner still holds the child's stdin open: its end can then only
    // be the reap's SIGKILL, never the stdin lifeline. The kernel ends a SIGKILLed process when it
    // next runs, which on a loaded Mac is not "by the next line" (Q-383) — so poll its state.
    for (const pid of pids.slice(0, -1)) process.kill(pid, 'SIGKILL');
    await pollUntil(() => goneOrZombie(childPid!), `the reaped child ${childPid} to end`);
    expect(alive(runner.pid!)).toBe(true);
    const closed = once(runner, 'close');
    process.kill(runner.pid!, 'SIGKILL');
    await closed;
    expect(goneOrZombie(childPid!)).toBe(true);
  } finally {
    killEach([childPid, runner.pid]);
  }
});

it('Q-243: an interrupted run leaves neither the runner nor its detached child behind', async () => {
  const runner = startRunner();
  let childPid: number | undefined;
  try {
    const [line] = await once(runner.stdout!, 'data');
    childPid = Number(String(line).trim());
    expect(alive(childPid)).toBe(true);
    // The worker's end of the runner's stdin closes — what the kernel does when the worker dies.
    const exited = once(runner, 'exit');
    runner.stdin!.end();
    await exited;
    await pollUntil(() => !alive(childPid!), `the detached child ${childPid} to follow its runner`);
    expect(alive(childPid)).toBe(false);
  } finally {
    killEach([childPid, runner.pid]);
  }
});
