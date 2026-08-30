import { describe, it, expect, beforeAll, beforeEach, afterEach } from 'vitest';
import fs from 'node:fs/promises';
import fsSync from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { SwarmWatchRegistry, type SwarmDelta } from './swarmWatch';

/**
 * PRECONDITION: does this machine's FSEvents actually deliver right now? Measured 2026-08-30: raw
 * fs.watch probes on identical fresh dirs delivered on one run and nothing on the next — the daemon
 * itself was degraded (long uptime, Electron + LM Studio + many watchers). These tests assert OUR
 * registry pushes when THE OS delivers; when the OS delivers nothing even to a raw watcher, a red
 * here reads as a watcher regression and sends someone hunting a bug that is not in the tree. The
 * product survives the same outage by design — the 500ms poll is the net (swarmWatch.ts's own
 * comment) — so the honest verdict is SKIP WITH REASON, never a fake green or a lying red.
 */
let fseventsAlive = false;
beforeAll(async () => {
  const base = path.join(os.homedir(), '.cache', 'goose-test');
  await fs.mkdir(base, { recursive: true });
  const probeRoot = await fs.mkdtemp(path.join(base, 'fsevents-probe-'));
  const probeFile = path.join(probeRoot, 'probe.jsonl');
  await fs.writeFile(probeFile, 'a\n');
  const seen: string[] = [];
  const w = fsSync.watch(probeRoot, (ev) => seen.push(ev));
  await new Promise((r) => setTimeout(r, 400));
  await fs.appendFile(probeFile, 'b\n');
  const deadline = Date.now() + 2500;
  while (seen.length === 0 && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 50));
  }
  w.close();
  await fs.rm(probeRoot, { recursive: true, force: true });
  fseventsAlive = seen.length > 0;
  if (!fseventsAlive) {
    console.warn(
      'swarmWatch.realfs: SKIPPING push-delivery assertions — a raw fs.watch saw ZERO events for a ' +
        'direct append (macOS FSEvents not delivering on this machine right now). The registry is ' +
        'not exonerated by these skips; re-run when the probe passes. The app itself degrades to ' +
        'the 500ms poll.'
    );
  }
}, 15000);

// The design's isolation rule for main-process work: a temp dir, files appended by the test, deltas
// asserted. No run is started to find out whether the push works.
let root: string;
let swarmDir: string;
let activityDir: string;
let reg: SwarmWatchRegistry;
let sent: SwarmDelta[];

beforeEach(async () => {
  // A HOME-anchored temp root, not os.tmpdir(): macOS FSEvents delivery for /var/folders paths can
  // go dead machine-wide (measured 2026-08-30 — a raw fs.watch probe saw ZERO events there while an
  // identical probe under $HOME delivered instantly). The app never watches tmp — it watches real
  // project dirs — so tmp-rooted fixtures test a path the product does not use, and their failures
  // read as watcher regressions when they are tmpfs quirks.
  const base = path.join(os.homedir(), '.cache', 'goose-test');
  await fs.mkdir(base, { recursive: true });
  root = await fs.mkdtemp(path.join(base, 'swarm-watch-'));
  swarmDir = path.join(root, '.swarm');
  activityDir = path.join(swarmDir, 'activity');
  await fs.mkdir(activityDir, { recursive: true });
  await fs.writeFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"start"}\n');
  sent = [];
  reg = new SwarmWatchRegistry({ debounceMs: 20 });
  reg.ensure('1', { workingDir: root, swarmDir, eventsDir: swarmDir, runId: 'r1' }, (d) =>
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
  it('pushes when the event log is appended to', async (ctx) => {
    // Runtime skip, not it.skipIf: skipIf reads the flag at COLLECTION time, before the beforeAll
    // probe has run, which would skip these unconditionally on every machine — a fake green.
    if (!fseventsAlive) ctx.skip();
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"task_dispatched"}\n');
    await waitForDelta(1);
    expect(sent[0]).toMatchObject({ workingDir: root, runId: 'r1' });
  });

  it('does NOT push for an activity digest rewrite — that churn belongs to the poll', async () => {
    // The engine rewrites these ~2.5x/sec per lane. Pushing on them drove the main process from a
    // fixed 2 reads/sec to roughly 10, and each read re-parses every digest in full because digests
    // are rewritten in place rather than appended — the incremental reader has nothing to skip.
    // A digest change is therefore left to the 500ms poll, which already bounds its cost.
    await fs.writeFile(path.join(activityDir, 'slice-store.json'), '{"last_text":"hello"}');
    await new Promise((r) => setTimeout(r, 400));
    expect(sent).toHaveLength(0);
  });

  it('pushes again after the first delta, so a run keeps streaming', async (ctx) => {
    if (!fseventsAlive) ctx.skip();
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"a"}\n');
    await waitForDelta(1);
    await new Promise((r) => setTimeout(r, 40));
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"b"}\n');
    await waitForDelta(2);
  });

  it('goes silent once released — no handle survives the renderer', async () => {
    reg.release('1');
    expect(reg.size()).toBe(0);
    await fs.appendFile(path.join(swarmDir, 'run-r1.jsonl'), '{"event":"after"}\n');
    await new Promise((r) => setTimeout(r, 200));
    expect(sent).toHaveLength(0);
  });

  it('never writes to the run it watches — a touched run would look alive forever', async () => {
    const before = (await fs.stat(path.join(swarmDir, 'run-r1.jsonl'))).mtimeMs;
    await new Promise((r) => setTimeout(r, 50));
    expect((await fs.stat(path.join(swarmDir, 'run-r1.jsonl'))).mtimeMs).toBe(before);
    // The no-delta half only means something when the daemon delivers ON TIME. Degraded FSEvents
    // (the probed state) delivers setup writes LATE — a ghost event landing here is the OS, not a
    // registry write; the mtime assertion above is the part of this invariant that is ours alone.
    if (fseventsAlive) expect(sent).toHaveLength(0);
  });
});
