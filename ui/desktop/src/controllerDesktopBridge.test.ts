import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs/promises';
import { createConnection } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import {
  BENCHMARK_LOG_CHUNK_BYTES,
  createDesktopBenchmarkHandler,
  queueSwarmNote,
  startDesktopBenchmarkBridge,
  type DesktopBenchmarkActive,
  type DesktopBenchmarkBridge,
  type DesktopBenchmarkOwner,
} from './controllerDesktopBridge';
import { upsertArchivedRow, type BenchSessionRow } from './benchSessions';

let root: string;
let rows: BenchSessionRow[];
let active: DesktopBenchmarkActive | null;
let owner: DesktopBenchmarkOwner;
let bridge: DesktopBenchmarkBridge | undefined;
const start = '2026-10-26T19:00:00.000Z';

beforeEach(async () => {
  root = await fs.mkdtemp(path.join(os.tmpdir(), 'bench-controller-test-'));
  const slot = path.join(root, 'slot');
  await fs.mkdir(slot);
  await fs.mkdir(path.join(root, 'sessions'));
  await fs.mkdir(path.join(root, 'completions'));
  await fs.mkdir(path.join(root, 'scoring-attempts'));
  rows = [
    {
      runId: 'run-a',
      scorerVersion: 'sb-7.2',
      startedAt: start,
      outcome: 'running',
      slot: true,
      slotDir: slot,
    },
  ];
  active = {
    runId: 'run-a',
    startedAt: start,
    workdir: slot,
    phase: 'build',
    lastLine: 'Building',
    noteSupported: true,
  };
  owner = {
    readSessions: vi.fn(async () => rows.map((row) => ({ ...row }))),
    sessionsRoot: () => path.join(root, 'sessions'),
    completionsRoot: () => path.join(root, 'completions'),
    scoringAttemptsRoot: () => path.join(root, 'scoring-attempts'),
    active: () => active,
    stop: vi.fn(() => {
      active = null;
      return { ok: true };
    }),
  };
});

afterEach(async () => {
  bridge?.close();
  bridge = undefined;
  await fs.rm(root, { recursive: true, force: true });
});

function command(overrides: Record<string, unknown> = {}) {
  return {
    command_id: 'command-a',
    activity_started_at: null,
    run_id: 'run-a',
    started_at: start,
    action: 'stop',
    ...overrides,
  };
}

function logParams(from = 0) {
  return { slot_dir: rows[0].slotDir ?? '', started_at: start, from };
}

function wire(raw: string | string[]): Promise<Record<string, unknown>> {
  const socketPath = bridge!.env.GOOSE_CONTROLLER_DESKTOP_SOCKET;
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath);
    let response = '';
    socket.on('error', reject);
    socket.on('data', (chunk) => {
      response += chunk.toString();
    });
    socket.on('end', () => {
      try {
        resolve(JSON.parse(response));
      } catch (error) {
        reject(error);
      }
    });
    socket.on('connect', () => {
      if (Array.isArray(raw)) for (const chunk of raw) socket.write(chunk);
      else socket.write(raw);
    });
  });
}

function request(
  method: string,
  params: unknown = {},
  secret = bridge!.env.GOOSE_CONTROLLER_DESKTOP_SECRET
) {
  return wire(JSON.stringify({ secret, method, params }) + '\n');
}

describe('desktop benchmark owner commands', () => {
  it('rejects stale run ids and launch stamps before the cancellation consumer', async () => {
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(handle('benchmark.command', command({ run_id: 'run-b' }))).rejects.toMatchObject({
      code: 'stale_run',
    });
    await expect(
      handle('benchmark.command', command({ started_at: 'yesterday' }))
    ).rejects.toMatchObject({ code: 'stale_run' });
    expect(owner.stop).not.toHaveBeenCalled();
    await expect(handle('benchmark.command', command())).resolves.toEqual({
      checkpoint_id: 'command-a',
      state: 'delivered',
    });
    expect(owner.stop).toHaveBeenCalledTimes(1);
  });

  it('serializes mutations and checks the active identity inside the mutation', async () => {
    const handle = createDesktopBenchmarkHandler(owner);
    const first = handle('benchmark.command', command());
    const second = handle(
      'benchmark.command',
      command({ action: 'note', text: 'Do not use yesterday’s run' })
    );
    await expect(first).resolves.toMatchObject({ state: 'delivered' });
    await expect(second).rejects.toMatchObject({ code: 'stale_run' });
    expect(await fs.readdir(rows[0].slotDir!)).toEqual([]);
  });

  it('writes the real swarm inbox and reports waiting rather than consumed delivery', async () => {
    const handle = createDesktopBenchmarkHandler(owner);
    const receipt = (await handle(
      'benchmark.command',
      command({ action: 'note', text: '  Inspect the failing endpoint  ' })
    )) as { checkpoint_id: string; state: string };
    expect(receipt).toMatchObject({ state: 'waiting' });
    expect(receipt.checkpoint_id).toMatch(/^\d{13}-[a-f\d-]+\.json$/);
    const file = path.join(rows[0].slotDir!, '.swarm', 'inbox', receipt.checkpoint_id);
    expect(JSON.parse(await fs.readFile(file, 'utf8'))).toEqual({
      text: 'Inspect the failing endpoint',
      command_id: 'command-a',
      human_assisted: true,
      source: 'link_controller',
    });
    expect(await fs.readdir(path.dirname(file))).toEqual([receipt.checkpoint_id]);
    expect((await fs.stat(file)).mode & 0o777).toBe(0o600);
  });

  it('keeps local notes unique when queued in the same millisecond', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(1793041200000);
    try {
      const first = queueSwarmNote(rows[0].slotDir!, 'First');
      const second = queueSwarmNote(rows[0].slotDir!, 'Second');
      expect(first).not.toBe(second);
      expect(await fs.readdir(path.join(rows[0].slotDir!, '.swarm', 'inbox'))).toHaveLength(2);
    } finally {
      vi.restoreAllMocks();
    }
  });

  it('rejects notes without a consumer, arbitrary commands, and failed stops', async () => {
    active!.noteSupported = false;
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(
      handle('benchmark.command', command({ action: 'note', text: 'Some note' }))
    ).rejects.toMatchObject({ code: 'unsupported_action' });
    await expect(handle('benchmark.command', command({ action: 'launch' }))).rejects.toMatchObject({
      code: 'unsupported_action',
    });
    owner.stop = () => ({ ok: false, error: 'process inspection failed' });
    await expect(handle('benchmark.command', command())).rejects.toMatchObject({
      code: 'stop_failed',
    });
    expect(await fs.readdir(rows[0].slotDir!)).toEqual([]);
  });

  it('refuses a prior build command when the same benchmark starts a scoring attempt', async () => {
    active!.activityStartedAt = '2026-10-26T21:00:00.000Z';
    active!.noteSupported = false;
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(handle('benchmark.command', command())).rejects.toMatchObject({
      code: 'stale_run',
    });
    expect(owner.stop).not.toHaveBeenCalled();
    await expect(
      handle('benchmark.command', command({ activity_started_at: active!.activityStartedAt }))
    ).resolves.toMatchObject({ state: 'delivered' });
    expect(owner.stop).toHaveBeenCalledTimes(1);
  });

  it('rejects a run inbox redirected outside the registered workdir', async () => {
    await fs.symlink(root, path.join(rows[0].slotDir!, '.swarm'));
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(
      handle('benchmark.command', command({ action: 'note', text: 'Cannot escape' }))
    ).rejects.toMatchObject({ code: 'unavailable' });
    await expect(fs.stat(path.join(root, 'inbox'))).rejects.toMatchObject({ code: 'ENOENT' });
  });
});

describe('desktop benchmark catalog and registered logs', () => {
  it('keeps a null-id launch addressable and publishes live identity from the actual owner', async () => {
    rows[0].runId = null;
    const handle = createDesktopBenchmarkHandler(owner);
    const catalog = (await handle('benchmark.catalog', {})) as {
      sessions: BenchSessionRow[];
      active: DesktopBenchmarkActive | null;
    };
    expect(catalog.sessions[0]).toMatchObject({
      runId: 'run-a',
      startedAt: start,
      slotDir: rows[0].slotDir,
    });
    expect(catalog.active).toMatchObject({ runId: 'run-a', noteSupported: true });
    active = null;
    expect(await handle('benchmark.catalog', {})).toMatchObject({ active: null });
    owner.readSessions = async () => {
      throw new Error('corrupt catalog');
    };
    await expect(handle('benchmark.catalog', {})).rejects.toThrow('corrupt catalog');
  });

  it('keeps byte cursors bounded without splitting UTF-8, and live EOF is not done', async () => {
    const contents = 'a'.repeat(BENCHMARK_LOG_CHUNK_BYTES - 1) + '🙂\nTAIL';
    await fs.writeFile(path.join(rows[0].slotDir!, 'run.jsonl'), contents);
    const handle = createDesktopBenchmarkHandler(owner);
    const first = (await handle('benchmark.log', logParams())) as {
      next: number;
      data: string;
      done: boolean;
    };
    expect(first.next).toBe(BENCHMARK_LOG_CHUNK_BYTES - 1);
    expect(first.data).toBe('a'.repeat(BENCHMARK_LOG_CHUNK_BYTES - 1));
    expect(first.done).toBe(false);
    const second = (await handle('benchmark.log', logParams(first.next))) as {
      next: number;
      data: string;
      done: boolean;
    };
    expect(second.data).toBe('🙂\nTAIL');
    expect(second.next).toBe(Buffer.byteLength(contents));
    expect(second.done).toBe(false);
    active = null;
    expect(await handle('benchmark.log', logParams(second.next))).toMatchObject({
      from: second.next,
      next: second.next,
      data: '',
      done: true,
    });
    await expect(handle('benchmark.log', logParams(second.next + 1))).rejects.toMatchObject({
      code: 'invalid_cursor',
    });
    await expect(handle('benchmark.log', logParams(first.next + 1))).rejects.toMatchObject({
      code: 'invalid_cursor',
    });
  });

  it('resolves the same launch after archive without leaking the new slot contents', async () => {
    const slot = rows[0].slotDir!;
    await fs.writeFile(path.join(slot, 'run.jsonl'), 'original launch');
    const params = logParams();
    await fs.rename(slot, path.join(owner.sessionsRoot(), 'run-a'));
    rows = upsertArchivedRow(rows, {
      runId: 'run-a',
      slotDir: slot,
      startedAt: start,
      outcome: 'finished',
      endedAt: start,
    });
    await fs.mkdir(slot);
    await fs.writeFile(path.join(slot, 'run.jsonl'), 'unrelated new launch');
    active = { ...active!, runId: 'run-b', startedAt: '2026-10-26T20:00:00.000Z' };
    const handle = createDesktopBenchmarkHandler(owner);
    expect(await handle('benchmark.log', params)).toMatchObject({
      data: 'original launch',
      done: true,
    });
    await expect(handle('benchmark.log', { ...params, slot_dir: root })).rejects.toMatchObject({
      code: 'not_found',
    });
    await expect(handle('benchmark.log', { ...params, run_id: 'run-b' })).rejects.toMatchObject({
      code: 'stale_run',
    });
  });

  it('reads the cloud harness console from startup and never switches to an event stream', async () => {
    rows[0].runId = 'cloud-starting';
    active!.runId = 'cloud-starting';
    rows[0].completionReceipt = path.join(owner.completionsRoot(), 'completion.json');
    await fs.writeFile(path.join(owner.completionsRoot(), 'completion.log'), 'runner booting');
    const handle = createDesktopBenchmarkHandler(owner);
    expect(await handle('benchmark.log', logParams())).toMatchObject({
      data: 'runner booting',
      done: false,
    });
    await fs.writeFile(path.join(rows[0].slotDir!, 'run.jsonl'), 'engine events');
    expect(await handle('benchmark.log', logParams())).toMatchObject({ data: 'runner booting' });
  });

  it('publishes actual swarm log growth and retains note consumption evidence', async () => {
    rows[0].completionReceipt = path.join(owner.completionsRoot(), 'native.json');
    await fs.writeFile(path.join(owner.completionsRoot(), 'native.log'), 'console');
    const handle = createDesktopBenchmarkHandler(owner);
    expect(await handle('benchmark.catalog', {})).toMatchObject({ active: { logBytes: null } });
    const log = path.join(rows[0].slotDir!, 'run.jsonl');
    await fs.writeFile(log, '');
    expect(await handle('benchmark.catalog', {})).toMatchObject({ active: { logBytes: 0 } });
    const receipt = (await handle(
      'benchmark.command',
      command({ action: 'note', text: 'Use this context' })
    )) as { checkpoint_id: string };
    const event =
      JSON.stringify({
        event: 'user_notes_delivered',
        notes: [receipt.checkpoint_id],
        task_id: 'worker-a',
        attempt: 1,
      }) + '\n';
    await fs.appendFile(log, event);
    expect(await handle('benchmark.catalog', {})).toMatchObject({
      active: { logBytes: Buffer.byteLength(event) },
    });
    expect(await handle('benchmark.log', logParams())).toMatchObject({ data: event, done: false });
  });

  it('serves bounded tails and older intervals at UTF-8 boundaries', async () => {
    const text = 'before🙂middle🙂after';
    await fs.writeFile(path.join(rows[0].slotDir!, 'run.jsonl'), text);
    const handle = createDesktopBenchmarkHandler(owner);
    expect(await handle('benchmark.log', { ...logParams(7), to: 18 })).toMatchObject({
      from: 10,
      next: 16,
      data: 'middle',
    });
    expect(
      await handle('benchmark.log', { started_at: start, slot_dir: rows[0].slotDir, tail_bytes: 7 })
    ).toMatchObject({ from: 20, next: 25, data: 'after' });
    await expect(handle('benchmark.log', { ...logParams(0), to: 26 })).rejects.toMatchObject({
      code: 'invalid_cursor',
    });
    await expect(
      handle('benchmark.log', { ...logParams(0), tail_bytes: 10 })
    ).rejects.toMatchObject({ code: 'bad_request' });
  });

  it('stats and reads the current scoring attempt without returning the original build log', async () => {
    await fs.writeFile(path.join(rows[0].slotDir!, 'run.jsonl'), 'original build');
    active!.activityStartedAt = '2026-10-26T21:00:00.000Z';
    active!.noteSupported = false;
    const attemptId = '11111111-1111-4111-8111-111111111111';
    rows[0].scoringAttempts = [{ id: attemptId, startedAt: active!.activityStartedAt! }];
    const attempt = path.join(owner.scoringAttemptsRoot(), attemptId);
    await fs.mkdir(attempt);
    const file = path.join(attempt, 'harness-console.log');
    const handle = createDesktopBenchmarkHandler(owner);
    expect(await handle('benchmark.catalog', {})).toMatchObject({
      active: { activityStartedAt: active!.activityStartedAt, logBytes: null },
    });
    await fs.writeFile(file, 'rescoring');
    expect(await handle('benchmark.catalog', {})).toMatchObject({ active: { logBytes: 9 } });
    expect(await handle('benchmark.log', logParams())).toMatchObject({ data: 'rescoring' });
    expect(
      await handle('benchmark.log', { ...logParams(), activity_started_at: null })
    ).toMatchObject({ data: 'original build' });
    expect(
      await handle('benchmark.log', {
        ...logParams(),
        activity_started_at: active!.activityStartedAt,
      })
    ).toMatchObject({ data: 'rescoring', done: false });
    rows[0].scoringAttempts = [];
    await expect(
      handle('benchmark.log', { ...logParams(), activity_started_at: active!.activityStartedAt })
    ).rejects.toMatchObject({ code: 'stale_run' });
  });

  it('retains final rescore output and old-page cursors after completion, bridge restart, and a later attempt', async () => {
    const attemptA = {
      id: '11111111-1111-4111-8111-111111111111',
      startedAt: '2026-10-26T21:00:00.000Z',
    };
    const attemptB = {
      id: '22222222-2222-4222-8222-222222222222',
      startedAt: '2026-10-26T22:00:00.000Z',
    };
    rows[0].scoringAttempts = [attemptA];
    const slot = rows[0].slotDir!;
    const index = path.join(root, 'sessions.json');
    const persist = () => fs.writeFile(index, JSON.stringify({ sessions: rows }));
    owner.readSessions = async () => JSON.parse(await fs.readFile(index, 'utf8')).sessions;
    const attemptDir = path.join(owner.scoringAttemptsRoot(), attemptA.id);
    await fs.mkdir(attemptDir);
    const consoleLog = path.join(attemptDir, 'harness-console.log');
    await fs.writeFile(path.join(slot, 'run.jsonl'), 'ORIGINAL BUILD LOG\n');
    const progress = 'earlier output\n'.repeat(12000) + 'RESCORE PROGRESS\n';
    await fs.writeFile(consoleLog, progress);
    active!.activityStartedAt = attemptA.startedAt;
    active!.noteSupported = false;
    await persist();
    bridge = await startDesktopBenchmarkBridge(owner);
    const selected = { slot_dir: slot, started_at: start, activity_started_at: attemptA.startedAt };
    const live = await request('benchmark.log', {
      ...selected,
      tail_bytes: BENCHMARK_LOG_CHUNK_BYTES,
    });
    expect(live).toMatchObject({ ok: true, result: { done: false } });
    const olderEnd = (live.result as { from: number }).from;
    expect(olderEnd).toBeGreaterThan(0);
    await fs.appendFile(consoleLog, 'RESCORE FINAL RESULT\n');
    active = null;
    rows[0].outcome = 'finished';
    await persist();
    bridge.close();
    bridge = await startDesktopBenchmarkBridge(owner);
    expect(await request('benchmark.catalog')).toMatchObject({
      ok: true,
      result: { active: null, sessions: [{ logActivityStartedAt: attemptA.startedAt }] },
    });
    const final = await request('benchmark.log', {
      ...selected,
      tail_bytes: BENCHMARK_LOG_CHUNK_BYTES,
    });
    expect(final).toMatchObject({ ok: true, result: { done: true } });
    expect((final.result as { data: string }).data).toContain('RESCORE FINAL RESULT');
    expect((final.result as { data: string }).data).not.toContain('ORIGINAL BUILD LOG');
    expect(
      await request('benchmark.log', { slot_dir: slot, started_at: start, tail_bytes: 21 })
    ).toMatchObject({ ok: true, result: { data: 'RESCORE FINAL RESULT\n', done: true } });
    expect(await request('benchmark.log', { ...selected, from: 0, to: olderEnd })).toMatchObject({
      ok: true,
      result: { from: 0, next: olderEnd, data: progress.slice(0, olderEnd) },
    });
    rows[0].scoringAttempts.push(attemptB);
    await fs.mkdir(path.join(owner.scoringAttemptsRoot(), attemptB.id));
    await fs.writeFile(
      path.join(owner.scoringAttemptsRoot(), attemptB.id, 'harness-console.log'),
      'SECOND ATTEMPT'
    );
    active = {
      runId: 'run-a',
      startedAt: start,
      workdir: slot,
      phase: 'score',
      lastLine: null,
      noteSupported: false,
      activityStartedAt: attemptB.startedAt,
    };
    await persist();
    expect(await request('benchmark.catalog')).toMatchObject({
      result: { sessions: [{ logActivityStartedAt: attemptB.startedAt }] },
    });
    expect(await request('benchmark.log', { ...selected, tail_bytes: 21 })).toMatchObject({
      ok: true,
      result: { data: 'RESCORE FINAL RESULT\n', done: true },
    });
    expect(
      await request('benchmark.command', command({ activity_started_at: attemptA.startedAt }))
    ).toMatchObject({ ok: false, error: { code: 'stale_run' } });
    expect(owner.stop).not.toHaveBeenCalled();
  });

  it('does not substitute build output for a registered missing or invalid attempt', async () => {
    const attempt = {
      id: '11111111-1111-4111-8111-111111111111',
      startedAt: '2026-10-26T21:00:00.000Z',
    };
    rows[0].scoringAttempts = [attempt];
    active = null;
    await fs.writeFile(path.join(rows[0].slotDir!, 'run.jsonl'), 'ORIGINAL BUILD LOG');
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(handle('benchmark.log', logParams())).rejects.toMatchObject({ code: 'ENOENT' });
    rows[0].scoringAttempts[0].id = '../slot';
    await expect(handle('benchmark.log', logParams())).rejects.toMatchObject({
      code: 'unavailable',
    });
  });

  it('rejects missing logs and symlinks to unrelated files', async () => {
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(handle('benchmark.log', logParams())).rejects.toMatchObject({ code: 'ENOENT' });
    await fs.writeFile(path.join(root, 'private'), 'secret');
    await fs.symlink(path.join(root, 'private'), path.join(rows[0].slotDir!, 'run.jsonl'));
    await expect(handle('benchmark.log', logParams())).rejects.toMatchObject({
      code: 'unavailable',
    });
    await expect(handle('benchmark.log', logParams(-1))).rejects.toMatchObject({
      code: 'bad_request',
    });
  });
});

describe('desktop benchmark Unix socket', () => {
  it('uses private permissions, authenticates every request, and accepts newline-framed fragments', async () => {
    bridge = await startDesktopBenchmarkBridge(owner);
    const socket = bridge.env.GOOSE_CONTROLLER_DESKTOP_SOCKET;
    expect((await fs.stat(socket)).mode & 0o777).toBe(0o600);
    expect((await fs.stat(path.dirname(socket))).mode & 0o777).toBe(0o700);
    expect(await request('benchmark.catalog', {}, 'wrong')).toMatchObject({
      ok: false,
      error: { code: 'unauthorized' },
    });
    expect(owner.readSessions).not.toHaveBeenCalled();
    const raw =
      JSON.stringify({
        secret: bridge.env.GOOSE_CONTROLLER_DESKTOP_SECRET,
        method: 'benchmark.catalog',
        params: {},
      }) + '\n';
    expect(await wire([raw.slice(0, 10), raw.slice(10)])).toMatchObject({
      ok: true,
      result: { sessions: [{ runId: 'run-a' }] },
    });
    bridge.close();
    bridge = undefined;
    await expect(fs.stat(socket)).rejects.toMatchObject({ code: 'ENOENT' });
  });

  it('refuses unknown methods, oversized requests, malformed JSON and extra request frames', async () => {
    bridge = await startDesktopBenchmarkBridge(owner);
    expect(await request('benchmark.launch')).toMatchObject({
      ok: false,
      error: { code: 'unsupported_method' },
    });
    expect(await wire('x'.repeat(65 * 1024) + '\n')).toMatchObject({
      ok: false,
      error: { code: 'request_too_large' },
    });
    expect(await wire('{\n')).toMatchObject({ ok: false });
    expect(await wire('{}\n{}\n')).toMatchObject({ ok: false, error: { code: 'bad_request' } });
    expect(owner.stop).not.toHaveBeenCalled();
  });

  it('returns bounded replies and safe errors without exposing the bridge secret', async () => {
    bridge = await startDesktopBenchmarkBridge(owner);
    active!.lastLine = 'x'.repeat(1024 * 1024);
    expect(await request('benchmark.catalog')).toMatchObject({
      ok: false,
      error: { code: 'response_too_large' },
    });
    owner.readSessions = async () => {
      throw new Error(bridge!.env.GOOSE_CONTROLLER_DESKTOP_SECRET);
    };
    const response = await request('benchmark.catalog');
    expect(response).toMatchObject({ ok: false, error: { code: 'unavailable' } });
    expect(JSON.stringify(response)).not.toContain(bridge.env.GOOSE_CONTROLLER_DESKTOP_SECRET);
  });
});

describe('benchmark note consumption receipts', () => {
  it('refuses a reused slot cursor and finds the delivery in the archived launch on retry', async () => {
    const slot = rows[0].slotDir!;
    const delivered =
      '\n' +
      JSON.stringify({ event: 'user_notes_delivered', run_id: 'run-a', notes: ['note-a.json'] }) +
      '\n';
    await fs.writeFile(path.join(slot, 'run.jsonl'), delivered);
    const params = {
      ...logParams(),
      run_id: 'run-a',
      activity_started_at: null,
      command_id: 'command-a',
      checkpoint_id: 'note-a.json',
    };
    vi.mocked(owner.readSessions).mockImplementationOnce(async () => {
      const oldRows = rows.map((row) => ({ ...row }));
      await fs.rename(slot, path.join(owner.sessionsRoot(), 'run-a'));
      rows = rows.map((row) => ({ ...row, slot: false }));
      await fs.mkdir(slot);
      await fs.writeFile(
        path.join(slot, 'run.jsonl'),
        JSON.stringify({ event: 'progress', run_id: 'run-b', text: 'x'.repeat(300) }) + '\n'
      );
      return oldRows;
    });
    const handle = createDesktopBenchmarkHandler(owner);
    await expect(handle('benchmark.receipt', params)).rejects.toMatchObject({ code: 'stale_run' });
    await expect(handle('benchmark.receipt', params)).resolves.toEqual({
      state: 'delivered',
      next: Buffer.byteLength(delivered),
    });
  });

  it('requires the actual matching run event and complete line, including after bridge restart', async () => {
    const log = path.join(rows[0].slotDir!, 'run.jsonl');
    const event = (run: string, checkpoint: string) =>
      JSON.stringify({ event: 'user_notes_delivered', run_id: run, notes: [checkpoint] });
    await fs.writeFile(
      log,
      event('other-run', 'note-a.json') + '\n' + event('run-a', 'different.json') + '\n'
    );
    bridge = await startDesktopBenchmarkBridge(owner);
    const params = {
      ...logParams(),
      run_id: 'run-a',
      activity_started_at: null,
      command_id: 'command-a',
      checkpoint_id: 'note-a.json',
    };
    const first = await request('benchmark.receipt', params);
    expect(first).toMatchObject({ ok: true, result: { state: 'waiting' } });
    const from = (first.result as { next: number }).next;
    await fs.appendFile(log, event('run-a', 'note-a.json'));
    expect(await request('benchmark.receipt', { ...params, from })).toMatchObject({
      ok: true,
      result: { state: 'waiting', next: from },
    });
    await fs.appendFile(log, '\n');
    active = null;
    bridge.close();
    bridge = await startDesktopBenchmarkBridge(owner);
    expect(await request('benchmark.receipt', { ...params, from })).toMatchObject({
      ok: true,
      result: { state: 'delivered' },
    });
    expect(await request('benchmark.receipt', { ...params, run_id: 'replacement' })).toMatchObject({
      ok: false,
      error: { code: 'not_found' },
    });
  });

  it('bounds scans and never converts EOF, truncation or malformed events into delivery', async () => {
    const log = path.join(rows[0].slotDir!, 'run.jsonl');
    const line = JSON.stringify({ event: 'progress', text: 'x'.repeat(1024) }) + '\n';
    await fs.writeFile(log, line.repeat(200));
    const handle = createDesktopBenchmarkHandler(owner);
    const params = {
      ...logParams(),
      run_id: 'run-a',
      activity_started_at: null,
      command_id: 'command-a',
      checkpoint_id: 'note-a.json',
    };
    const first = (await handle('benchmark.receipt', params)) as { state: string; next: number };
    expect(first.state).toBe('waiting');
    expect(first.next).toBeGreaterThan(0);
    expect(first.next).toBeLessThanOrEqual(BENCHMARK_LOG_CHUNK_BYTES);
    expect(first.next % Buffer.byteLength(line)).toBe(0);
    await fs.writeFile(log, '');
    await expect(
      handle('benchmark.receipt', { ...params, from: first.next })
    ).rejects.toMatchObject({ code: 'invalid_cursor' });
    active = null;
    expect(await handle('benchmark.receipt', params)).toEqual({ state: 'waiting', next: 0 });
    await fs.writeFile(log, 'invalid\n');
    await expect(handle('benchmark.receipt', params)).rejects.toMatchObject({
      code: 'invalid_receipt_event',
    });
    await fs.writeFile(log, 'x'.repeat(BENCHMARK_LOG_CHUNK_BYTES));
    await expect(handle('benchmark.receipt', params)).rejects.toMatchObject({
      code: 'receipt_event_too_large',
    });
  });
});
