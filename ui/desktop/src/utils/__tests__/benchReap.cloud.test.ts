import { expect, it } from 'vitest';
import { spawn, execFileSync } from 'node:child_process';
import { once } from 'node:events';
import { benchmarkCancellationPids } from '../benchReap';
it('captures and reaps a real cloud child in a separate session before its runner', async () => {
  const code = `const {spawn}=require('node:child_process'); const c=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore'}); console.log(c.pid); setInterval(()=>{},1000);`;
  const runner = spawn(process.execPath, ['-e', code], {
    detached: true,
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  let childPid: number | undefined;
  try {
    const [line] = await once(runner.stdout!, 'data');
    childPid = Number(String(line).trim());
    const snapshot = execFileSync('ps', ['-axo', 'pid=,ppid=,args='], { encoding: 'utf8' });
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
    const closed = once(runner, 'close');
    for (const pid of pids) process.kill(pid, 'SIGKILL');
    await closed;
    const remaining = execFileSync('ps', ['-axo', 'pid=,stat='], { encoding: 'utf8' })
      .split('\n')
      .find((line) => Number(line.trim().split(/\s+/)[0]) === childPid);
    expect(!remaining || /Z/.test(remaining)).toBe(true);
  } finally {
    for (const pid of [childPid, runner.pid])
      if (pid)
        try {
          process.kill(pid, 'SIGKILL');
        } catch {
          /* The test already reaped this process. */
        }
  }
});
