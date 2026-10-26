import { randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import { createServer, type Socket } from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { benchRunDataDir, type BenchSessionRow } from './benchSessions';

const REQUEST_BYTES = 64 * 1024;
const RESPONSE_BYTES = 1024 * 1024;
export const BENCHMARK_LOG_CHUNK_BYTES = 128 * 1024;

export interface DesktopBenchmarkActive {
  runId: string | null;
  startedAt: string;
  workdir: string;
  phase: string;
  lastLine: string | null;
  noteSupported: boolean;
  activityStartedAt?: string | null;
}

export interface DesktopBenchmarkOwner {
  readSessions: () => Promise<BenchSessionRow[]>;
  sessionsRoot: () => string;
  completionsRoot: () => string;
  scoringAttemptsRoot: () => string;
  active: () => DesktopBenchmarkActive | null;
  stop: () => { ok: boolean; error?: string };
}

class BridgeError extends Error {
  constructor(
    readonly code: string,
    message: string
  ) {
    super(message);
  }
}

const object = (value: unknown): Record<string, unknown> => {
  if (!value || typeof value !== 'object' || Array.isArray(value))
    throw new BridgeError('bad_request', 'Expected an object');
  return value as Record<string, unknown>;
};

const requiredString = (params: Record<string, unknown>, key: string): string => {
  const value = params[key];
  if (typeof value !== 'string' || !value.trim())
    throw new BridgeError('bad_request', `${key} must be a nonempty string`);
  return value;
};

/** Same inbox and filename ordering as read_user_notes in the swarm engine. */
export function queueSwarmNote(
  workingDir: string,
  text: string,
  provenance: {
    command_id?: string;
    device_id?: string;
    human_assisted?: boolean;
    source?: string;
  } = {}
): string {
  const trimmed = text.trim();
  if (!trimmed) throw new BridgeError('bad_request', 'Note is empty');
  const root = fs.realpathSync(workingDir);
  const swarm = path.join(root, '.swarm');
  fs.mkdirSync(swarm, { recursive: true });
  if (!fs.realpathSync(swarm).startsWith(root + path.sep))
    throw new BridgeError('unavailable', 'Swarm notes directory is outside the run');
  const inbox = path.join(swarm, 'inbox');
  fs.mkdirSync(inbox, { recursive: true });
  if (!fs.realpathSync(inbox).startsWith(root + path.sep))
    throw new BridgeError('unavailable', 'Swarm inbox is outside the run');
  const id = `${Date.now()}-${randomUUID()}.json`;
  const temporary = path.join(inbox, `${id}.tmp`);
  const fd = fs.openSync(temporary, 'wx', 0o600);
  try {
    fs.writeFileSync(fd, JSON.stringify({ text: trimmed, ...provenance }));
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
  fs.renameSync(temporary, path.join(inbox, id));
  return id;
}

function matchingActive(row: BenchSessionRow, active: DesktopBenchmarkActive | null): boolean {
  return Boolean(
    active &&
    row.startedAt === active.startedAt &&
    (row.slotDir === active.workdir || (row.runId != null && row.runId === active.runId))
  );
}

function completeUtf8End(bytes: Buffer): number {
  let start = bytes.length - 1;
  while (start >= 0 && (bytes[start] & 0xc0) === 0x80) start--;
  if (start < 0) return 0;
  const lead = bytes[start];
  const length = lead < 0x80 ? 1 : lead < 0xe0 ? 2 : lead < 0xf0 ? 3 : 4;
  return bytes.length - start < length ? start : bytes.length;
}

async function openRegisteredLog(
  row: BenchSessionRow,
  owner: DesktopBenchmarkOwner,
  activityStartedAt: string | null
) {
  const dataDir = benchRunDataDir(row, owner.sessionsRoot());
  if (!dataDir) throw new BridgeError('unavailable', 'Benchmark has no log directory yet');
  // Cloud run IDs are minted before spawn; native swarm IDs begin null. This pins one source
  // from startup, including user_notes_delivered evidence for swarm, without swapping byte cursors.
  const consoleLog = row.runId?.startsWith('cloud-') === true && row.completionReceipt;
  const attempts = (row.scoringAttempts ?? []).filter(
    (attempt) => attempt.startedAt === activityStartedAt
  );
  if (activityStartedAt !== null && attempts.length !== 1)
    throw new BridgeError('stale_run', 'The scoring attempt is not registered to this benchmark');
  const attempt = activityStartedAt === null ? undefined : attempts[0];
  if (
    attempt &&
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(attempt.id)
  )
    throw new BridgeError('unavailable', 'The scoring attempt directory is invalid');
  const root = attempt
    ? owner.scoringAttemptsRoot()
    : consoleLog
      ? owner.completionsRoot()
      : dataDir;
  const candidate = attempt
    ? path.join(root, attempt.id, 'harness-console.log')
    : consoleLog
      ? consoleLog.replace(/\.json$/, '.log')
      : path.join(dataDir, 'run.jsonl');
  const realRoot = await fsp.realpath(root);
  const realFile = await fsp.realpath(candidate);
  if (!realFile.startsWith(realRoot + path.sep))
    throw new BridgeError('unavailable', 'Benchmark log is outside its registered directory');
  const handle = await fsp.open(realFile, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW);
  try {
    const stat = await handle.stat();
    if (!stat.isFile()) throw new BridgeError('unavailable', 'Benchmark log is not a file');
    return { handle, stat };
  } catch (error) {
    await handle.close();
    throw error;
  }
}

interface LogRange {
  from?: number;
  to?: number;
  tailBytes?: number;
}

async function readRegisteredLog(
  row: BenchSessionRow,
  range: LogRange,
  owner: DesktopBenchmarkOwner,
  activityStartedAt: string | null
): Promise<{ from: number; next: number; data: string; done: boolean }> {
  const { handle, stat } = await openRegisteredLog(row, owner, activityStartedAt);
  try {
    const from = range.tailBytes == null ? range.from! : Math.max(0, stat.size - range.tailBytes);
    const to = range.to ?? stat.size;
    if (from > stat.size || to > stat.size)
      throw new BridgeError('invalid_cursor', 'Benchmark log cursor is past EOF');
    const bytes = Buffer.alloc(Math.min(BENCHMARK_LOG_CHUNK_BYTES, to - from));
    const read = await handle.read(bytes, 0, bytes.length, from);
    const chunk = bytes.subarray(0, read.bytesRead);
    const bounded = range.to != null || range.tailBytes != null;
    let begin = 0;
    while (begin < chunk.length && (chunk[begin] & 0xc0) === 0x80) begin++;
    if (begin > 3 || (begin > 0 && !bounded))
      throw new BridgeError('invalid_cursor', 'Benchmark log cursor splits a UTF-8 character');
    const live = owner.active();
    const active =
      matchingActive(row, live) && (live?.activityStartedAt ?? null) === activityStartedAt;
    const rest = chunk.subarray(begin);
    const end =
      begin +
      (bounded || from + chunk.length < stat.size || active ? completeUtf8End(rest) : rest.length);
    return {
      from: from + begin,
      next: from + end,
      data: chunk.subarray(begin, end).toString('utf8'),
      done: !active && from + end >= stat.size,
    };
  } finally {
    await handle.close();
  }
}

async function assertLogMapping(row: BenchSessionRow, owner: DesktopBenchmarkOwner) {
  if (row.runId?.startsWith('cloud-') && row.completionReceipt) return;
  const matches = (await owner.readSessions()).filter(
    (entry) => entry.startedAt === row.startedAt && (entry.slotDir ?? '') === (row.slotDir ?? '')
  );
  if (
    matches.length !== 1 ||
    matches[0].runId !== row.runId ||
    benchRunDataDir(matches[0], owner.sessionsRoot()) !== benchRunDataDir(row, owner.sessionsRoot())
  )
    throw new BridgeError('stale_run', 'Benchmark log moved; read the launch again');
}

async function readNoteReceipt(
  owner: DesktopBenchmarkOwner,
  params: Record<string, unknown>
): Promise<{ state: 'waiting' | 'delivered'; next: number }> {
  const startedAt = requiredString(params, 'started_at');
  const runId = requiredString(params, 'run_id');
  requiredString(params, 'command_id');
  const checkpoint = requiredString(params, 'checkpoint_id');
  if (params.activity_started_at !== null || typeof params.slot_dir !== 'string')
    throw new BridgeError(
      'bad_request',
      'Note receipts require the original registered swarm launch'
    );
  if (!Number.isSafeInteger(params.from) || Number(params.from) < 0)
    throw new BridgeError('invalid_cursor', 'Receipt cursor must be a nonnegative safe integer');
  const rows = (await owner.readSessions()).filter(
    (row) =>
      row.startedAt === startedAt && (row.slotDir ?? '') === params.slot_dir && row.runId === runId
  );
  if (rows.length !== 1 || runId.startsWith('cloud-'))
    throw new BridgeError('not_found', 'The original swarm launch is absent or ambiguous');
  const { handle, stat } = await openRegisteredLog(rows[0], owner, null);
  try {
    const from = Number(params.from);
    if (from > stat.size) throw new BridgeError('invalid_cursor', 'Receipt log was truncated');
    const bytes = Buffer.alloc(Math.min(BENCHMARK_LOG_CHUNK_BYTES, stat.size - from));
    const read = await handle.read(bytes, 0, bytes.length, from);
    const chunk = bytes.subarray(0, read.bytesRead);
    await assertLogMapping(rows[0], owner);
    const boundary = chunk.lastIndexOf(10) + 1;
    if (!boundary && chunk.length === BENCHMARK_LOG_CHUNK_BYTES)
      throw new BridgeError('receipt_event_too_large', 'An event exceeds the bounded receipt scan');
    let next = from;
    for (const line of chunk.subarray(0, boundary).toString('utf8').split('\n')) {
      next += Buffer.byteLength(line) + 1;
      if (!line) continue;
      let event: Record<string, unknown>;
      try {
        event = object(JSON.parse(line));
      } catch {
        throw new BridgeError(
          'invalid_receipt_event',
          'The owner event log contains an invalid complete event'
        );
      }
      if (
        event.event === 'user_notes_delivered' &&
        event.run_id === runId &&
        Array.isArray(event.notes) &&
        event.notes.includes(checkpoint)
      )
        return { state: 'delivered', next };
    }
    return { state: 'waiting', next: from + boundary };
  } finally {
    await handle.close();
  }
}

export function createDesktopBenchmarkHandler(owner: DesktopBenchmarkOwner) {
  let mutation: Promise<unknown> = Promise.resolve();
  return async (method: string, value: unknown): Promise<unknown> => {
    const params = object(value);
    if (method === 'benchmark.catalog') {
      const sessions = await owner.readSessions();
      const snapshot = owner.active();
      const row = sessions.find((entry) => matchingActive(entry, snapshot));
      let logBytes: number | null = null;
      if (row) {
        try {
          const { handle, stat } = await openRegisteredLog(
            row,
            owner,
            snapshot?.activityStartedAt ?? null
          );
          logBytes = stat.size;
          await handle.close();
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        }
      }
      const active = owner.active();
      const sameLaunch =
        active &&
        snapshot &&
        active.runId === snapshot.runId &&
        active.startedAt === snapshot.startedAt &&
        active.workdir === snapshot.workdir &&
        (active.activityStartedAt ?? null) === (snapshot.activityStartedAt ?? null);
      return {
        sessions: sessions.map((entry) => ({
          ...entry,
          ...(matchingActive(entry, active) ? { runId: active!.runId, outcome: 'running' } : {}),
          logActivityStartedAt:
            entry.scoringAttempts?.[entry.scoringAttempts.length - 1]?.startedAt ?? null,
        })),
        active: active ? { ...active, logBytes: sameLaunch ? logBytes : null } : null,
      };
    }
    if (method === 'benchmark.command') {
      const commandId = requiredString(params, 'command_id');
      const runId = requiredString(params, 'run_id');
      const startedAt = requiredString(params, 'started_at');
      if (params.activity_started_at !== null && typeof params.activity_started_at !== 'string')
        throw new BridgeError('bad_request', 'activity_started_at must be null or a string');
      const action = params.action;
      if (action !== 'stop' && action !== 'note')
        throw new BridgeError('unsupported_action', 'Only stop and note are supported');
      const text = action === 'note' ? requiredString(params, 'text') : '';
      const deviceId =
        params.device_id === undefined ? undefined : requiredString(params, 'device_id');
      const result = mutation.then(() => {
        const active = owner.active();
        if (
          !active ||
          active.runId !== runId ||
          active.startedAt !== startedAt ||
          (active.activityStartedAt ?? null) !== params.activity_started_at
        )
          throw new BridgeError('stale_run', 'The active benchmark launch changed');
        // No await between the identity check and the consumer: a new launch cannot inherit a command.
        if (action === 'stop') {
          const stopped = owner.stop();
          if (!stopped.ok)
            throw new BridgeError('stop_failed', stopped.error ?? 'Benchmark stop failed');
          return { checkpoint_id: commandId, state: 'delivered' };
        }
        if (!active.noteSupported)
          throw new BridgeError('unsupported_action', 'This benchmark has no live note consumer');
        const checkpointId = queueSwarmNote(active.workdir, text, {
          command_id: commandId,
          ...(deviceId ? { device_id: deviceId } : {}),
          human_assisted: true,
          source: 'link_controller',
        });
        return {
          checkpoint_id: checkpointId,
          state: 'waiting',
          reason: 'user_note_queued',
        };
      });
      mutation = result.catch(() => {});
      return result;
    }
    if (method === 'benchmark.receipt') return readNoteReceipt(owner, params);
    if (method === 'benchmark.log') {
      const startedAt = requiredString(params, 'started_at');
      if (typeof params.slot_dir !== 'string')
        throw new BridgeError('bad_request', 'slot_dir must be a string');
      const range: LogRange = {};
      if (params.tail_bytes !== undefined) {
        if (
          params.from !== undefined ||
          params.to !== undefined ||
          !Number.isSafeInteger(params.tail_bytes) ||
          Number(params.tail_bytes) <= 0 ||
          Number(params.tail_bytes) > BENCHMARK_LOG_CHUNK_BYTES
        )
          throw new BridgeError('bad_request', 'tail_bytes must name one bounded tail interval');
        range.tailBytes = Number(params.tail_bytes);
      } else {
        if (!Number.isSafeInteger(params.from) || Number(params.from) < 0)
          throw new BridgeError('bad_request', 'from must be a nonnegative safe integer');
        range.from = Number(params.from);
        if (params.to !== undefined) {
          if (!Number.isSafeInteger(params.to) || Number(params.to) < range.from)
            throw new BridgeError('bad_request', 'to must be a safe integer at or after from');
          range.to = Number(params.to);
        }
      }
      const rows = await owner.readSessions();
      const matches = rows.filter(
        (row) => row.startedAt === startedAt && (row.slotDir ?? '') === params.slot_dir
      );
      if (matches.length !== 1)
        throw new BridgeError('not_found', 'Benchmark launch is absent or ambiguous');
      const row = matches[0];
      if (params.run_id != null && params.run_id !== row.runId)
        throw new BridgeError('stale_run', 'Benchmark run identity changed');
      if (
        params.activity_started_at !== undefined &&
        params.activity_started_at !== null &&
        typeof params.activity_started_at !== 'string'
      )
        throw new BridgeError('bad_request', 'activity_started_at must be null or a string');
      const activityStartedAt =
        params.activity_started_at === undefined
          ? (row.scoringAttempts?.[row.scoringAttempts.length - 1]?.startedAt ?? null)
          : (params.activity_started_at as string | null);
      const result = await readRegisteredLog(row, range, owner, activityStartedAt);
      // Refuse an archival race before returning text or advancing its byte cursor.
      await assertLogMapping(row, owner);
      return { ...result, activity_started_at: activityStartedAt };
    }
    throw new BridgeError('unsupported_method', 'Desktop method is not supported');
  };
}

export interface DesktopBenchmarkBridge {
  env: { GOOSE_CONTROLLER_DESKTOP_SOCKET: string; GOOSE_CONTROLLER_DESKTOP_SECRET: string };
  close: () => void;
}

export async function startDesktopBenchmarkBridge(
  owner: DesktopBenchmarkOwner
): Promise<DesktopBenchmarkBridge> {
  const directory = await fsp.mkdtemp(path.join(os.tmpdir(), 'goose-controller-'));
  await fsp.chmod(directory, 0o700);
  const socketPath = path.join(directory, 'desktop.sock');
  const secret = randomBytes(32).toString('hex');
  const secretBytes = Buffer.from(secret);
  const handle = createDesktopBenchmarkHandler(owner);
  const sockets = new Set<Socket>();
  const server = createServer((socket) => {
    sockets.add(socket);
    socket.once('close', () => sockets.delete(socket));
    socket.on('error', () => socket.destroy());
    // Transport-only bounds: they never constrain benchmark/model work.
    socket.setTimeout(30_000, () => socket.destroy());
    let pending = Buffer.alloc(0);
    let dispatched = false;
    const respond = (response: unknown) => {
      let encoded = JSON.stringify(response) + '\n';
      if (Buffer.byteLength(encoded) > RESPONSE_BYTES)
        encoded =
          JSON.stringify({
            ok: false,
            error: {
              code: 'response_too_large',
              message: 'Desktop response exceeds the transport limit',
            },
          }) + '\n';
      socket.end(encoded);
    };
    socket.on('data', (chunk: Buffer) => {
      if (dispatched) return;
      if (pending.length + chunk.length > REQUEST_BYTES) {
        dispatched = true;
        respond({
          ok: false,
          error: {
            code: 'request_too_large',
            message: 'Desktop request exceeds the transport limit',
          },
        });
        return;
      }
      pending = Buffer.concat([pending, chunk]);
      const newline = pending.indexOf(10);
      if (newline < 0) return;
      dispatched = true;
      void (async () => {
        try {
          if (
            pending
              .subarray(newline + 1)
              .toString()
              .trim()
          )
            throw new BridgeError('bad_request', 'One request per connection is required');
          const request = object(JSON.parse(pending.subarray(0, newline).toString('utf8')));
          const received =
            typeof request.secret === 'string' ? Buffer.from(request.secret) : Buffer.alloc(0);
          if (received.length !== secretBytes.length || !timingSafeEqual(received, secretBytes))
            throw new BridgeError('unauthorized', 'Desktop authentication failed');
          const result = await handle(requiredString(request, 'method'), request.params);
          respond({ ok: true, result });
        } catch (error) {
          respond({
            ok: false,
            error: {
              code: error instanceof BridgeError ? error.code : 'unavailable',
              message:
                error instanceof BridgeError ? error.message : 'Desktop benchmark operation failed',
            },
          });
        }
      })();
    });
  });
  try {
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      server.listen(socketPath, () => {
        server.off('error', reject);
        resolve();
      });
    });
    await fsp.chmod(socketPath, 0o600);
  } catch (error) {
    server.close();
    await fsp.rm(directory, { recursive: true, force: true });
    throw error;
  }
  return {
    env: { GOOSE_CONTROLLER_DESKTOP_SOCKET: socketPath, GOOSE_CONTROLLER_DESKTOP_SECRET: secret },
    close: () => {
      for (const socket of sockets) socket.destroy();
      server.close();
      fs.rmSync(directory, { recursive: true, force: true });
    },
  };
}
