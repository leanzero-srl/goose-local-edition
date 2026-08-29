import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { SwarmWatchRegistry, type SwarmDelta } from './swarmWatch';

// The design's isolation rule for main-process work: a temp dir, files appended by the test, deltas
// asserted. No run is started to find out whether the push works.
let root: string;
let swarmDir: string;
let activityDir: string;
let reg: SwarmWatchRegistry;
let sent: SwarmDelta[];

beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'swarm-watch-'));
  swarmDir = path.join(root, '.swarm');
  activityDir = path.join(swarmDir, 'activity');
  await fs.mkdir(activityDir, { recursive: true });
  await fs.writeFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"start"}\n');
  sent = [];
  reg = new SwarmWatchRegistry({ debounceMs: 20 });
  reg.ensure(1, { workingDir: root, swarmDir, eventsDir: swarmDir, runId: 'r1' }, (d) =>
    sent.push(d)
  );
  // MEASURED on macOS: a write in the first ~100ms after fs.watch returns is never delivered — the
  // FSEvents stream is not established yet — and the writes that set this directory up arrive just
  // after it is. That is the drop hazard itself, and the reason the renderer's poll stays. Settle,
  // then discard whatever the setup produced, so each test asserts only its own write.
  await new Promise((r) => setTimeout(r, 200));
  sent.length = 0;
});

afterEach(async () => {
  reg.releaseAll();
  await fs.rm(root, { recursive: true, force: true });
});

async function waitForDelta(atLeast: number): Promise<void> {
  const deadline = Date.now() + 4000;
  while (sent.length < atLeast && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 20));
  }
  expect(sent.length).toBeGreaterThanOrEqual(atLeast);
}

describe('SwarmWatchRegistry against a real filesystem', () => {
  it('pushes when the event log is appended to', async () => {
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"task_dispatched"}\n');
    await waitForDelta(1);
    expect(sent[0]).toMatchObject({ workingDir: root, runId: 'r1' });
  });

  it('pushes when a worker rewrites its activity digest', async () => {
    await fs.writeFile(path.join(activityDir, 'slice-store.json'), '{"last_text":"hello"}');
    await waitForDelta(1);
    // macOS attributes a write inside activity/ to the parent watcher as readily as to its own, so
    // `source` is diagnostic only — which watcher noticed is not something a receiver may act on.
    expect([swarmDir, activityDir]).toContain(sent[0].source);
    expect(sent[0]).toMatchObject({ workingDir: root, runId: 'r1' });
  });

  it('pushes again after the first delta, so a run keeps streaming', async () => {
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"a"}\n');
    await waitForDelta(1);
    await new Promise((r) => setTimeout(r, 40));
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"b"}\n');
    await waitForDelta(2);
  });

  it('goes silent once released — no handle survives the renderer', async () => {
    reg.release(1);
    expect(reg.size()).toBe(0);
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"after"}\n');
    await new Promise((r) => setTimeout(r, 200));
    expect(sent).toHaveLength(0);
  });

  it('never writes to the run it watches — a touched run would look alive forever', async () => {
    const before = (await fs.stat(path.join(swarmDir, 'run-r1.jsonl'))).mtimeMs;
    await new Promise((r) => setTimeout(r, 50));
    expect((await fs.stat(path.join(swarmDir, 'run-r1.jsonl'))).mtimeMs).toBe(before);
    expect(sent).toHaveLength(0);
  });
});
