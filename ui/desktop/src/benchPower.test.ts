import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { expect, it, vi } from 'vitest';
import { spawnBenchmarkWithPower } from './benchPower';
const power = () => ({start:vi.fn(() => 42),stop:vi.fn()});
it('holds the assertion across actual runner build and score output, then releases on close', async () => {
  const blocker=power();
  const {child}=spawnBenchmarkWithPower(blocker,()=>spawn(process.execPath,['-e',"process.stdout.write('build\\n');process.stdin.once('data',()=>{process.stdout.write('score\\n');process.stdin.once('data',()=>process.exit(0))})"]));
  await once(child.stdout!,'data');
  expect(blocker.start).toHaveBeenCalledWith('prevent-app-suspension');
  expect(blocker.stop).not.toHaveBeenCalled();
  child.stdin!.write('score');await once(child.stdout!,'data');
  expect(blocker.stop).not.toHaveBeenCalled();
  child.stdin!.write('finish');await once(child,'close');
  expect(blocker.stop).toHaveBeenCalledExactlyOnceWith(42);
});
it('releases on cancellation without double release when the child closes', async () => {
  const blocker=power();
  const {child,releasePower}=spawnBenchmarkWithPower(blocker,()=>spawn(process.execPath,['-e','process.stdin.resume()']));
  const closed=once(child,'close');child.kill('SIGKILL');releasePower();await closed;
  expect(blocker.stop).toHaveBeenCalledExactlyOnceWith(42);
});
it('releases on asynchronous missing executable and subsequent close', async () => {
  const blocker=power();
  const {child}=spawnBenchmarkWithPower(blocker,()=>spawn('/nonexistent/benchmark-runner'));
  await new Promise<void>(resolve=>child.once('close',()=>resolve()));
  expect(blocker.stop).toHaveBeenCalledExactlyOnceWith(42);
});
it('releases when spawn throws synchronously', () => {
  const blocker=power();
  expect(()=>spawnBenchmarkWithPower(blocker,()=>{throw new Error('spawn failed')})).toThrow('spawn failed');
  expect(blocker.stop).toHaveBeenCalledExactlyOnceWith(42);
});
