import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import type { LinkState } from '../../acp/leanzero-link';
import type { MlxServingIntent } from '../../acp/mlx-serving-intent';
import { MlxMountRefusedError } from '../../acp/mlx-engine';
import {
  settleRestoreLine,
  REMOTE_START_TRIES,
  latestRestoreLine,
  publishRestoreLine,
  restoreServing,
  retryRestore,
  runRestore,
  toRestoreReport,
  type RestoreDeps,
} from './mlxRestore';
import { isMlxRestoreReport, restoreTrayLine } from '../../utils/mlxRestoreReport';
import { buildMlxTrayModel } from '../../utils/mlxTray';
import { INITIAL_SNAPSHOT } from '../../utils/mlxEngineMonitor';

const QWEN = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
const REMOTE: MlxServingIntent = {
  kind: 'remoteSingle',
  modelId: QWEN,
  peer: 'worksmacstudio-lan-9c1e2a',
  peerName: "Work's Mac Studio",
};

const engine = (overrides: Partial<MlxEngineStatus>): MlxEngineStatus => ({
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 90,
  totalMemoryGb: 128,
  ...overrides,
});
const CONNECTED = { auth: { state: 'connected' }, nodeCount: 2 } as unknown as LinkState;
const RECONNECTING = {
  auth: { state: 'loggedIn', email: 'm@x.co' },
  nodeCount: 0,
  intent: { intent: 'connected' },
  reconnect: { state: 'reconnecting', startedAt: 't' },
} as unknown as LinkState;

/** Deps that answer from queues: each call takes the next answer, the last one repeats. */
function deps(answers: {
  intent?: Array<MlxServingIntent | null | { error: string }>;
  single?: MlxEngineStatus[];
  remote?: MlxRemoteSingleStatus[];
  remoteStart?: Array<{ started: boolean; refusal?: { code: string; message: string } }>;
  distributed?: Array<MlxDistributedStatus | null>;
  distributedStart?: Array<{
    started: boolean;
    refusal?: { code: string; message: string; node?: string; detail?: string };
  }>;
  link?: Array<LinkState | null>;
  mount?: () => Promise<void>;
}) {
  const take = <T>(queue: T[] | undefined, fallback: T): T =>
    queue && queue.length > 1 ? (queue.shift() as T) : (queue?.[0] ?? fallback);
  const d = {
    readIntent: vi.fn(async () => {
      const next = take(answers.intent, null);
      return next && 'error' in next
        ? { intent: null, error: next.error }
        : { intent: next as MlxServingIntent | null, error: null };
    }),
    singleStatus: vi.fn(async () => take(answers.single, engine({}))),
    mount: vi.fn(answers.mount ?? (async () => undefined)),
    remoteStatus: vi.fn(async () => take(answers.remote, { state: 'off' })),
    remoteStart: vi.fn(async () => ({
      ...take(answers.remoteStart, { started: true }),
      status: { state: 'mounting' },
    })),
    distributedStatus: vi.fn(async () => take(answers.distributed, null)),
    distributedStart: vi.fn(async () => take(answers.distributedStart, { started: true })),
    linkState: vi.fn(async () => take(answers.link, CONNECTED)),
    wait: vi.fn(async () => undefined),
  };
  return d as typeof d & RestoreDeps;
}

describe('restoreServing — what served before the relaunch comes back the way a click starts it', () => {
  it('nothing recorded (never started, or the owner stopped it): nothing is started', async () => {
    const d = deps({ intent: [null] });
    const onRestoring = vi.fn();
    expect(await restoreServing(d, onRestoring)).toEqual({ phase: 'idle' });
    expect(onRestoring).not.toHaveBeenCalled();
    expect(d.mount).not.toHaveBeenCalled();
    expect(d.remoteStart).not.toHaveBeenCalled();
    expect(d.distributedStart).not.toHaveBeenCalled();
  });

  it('an unreadable record is a named failure, never "nothing to restore"', async () => {
    const d = deps({ intent: [{ error: 'the record ... is unreadable: expected value' }] });
    expect(await restoreServing(d, vi.fn())).toEqual({
      phase: 'failed',
      what: null,
      reason: { code: 'said', text: 'the record ... is unreadable: expected value' },
    });
  });

  it('this Mac: Mount through the gate, followed through Make room and mounting to running', async () => {
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }],
      single: [
        engine({}),
        engine({ state: 'stopped', load: { phase: 'makingRoom', weightsBytes: 30 } } as never),
        engine({ state: 'mounting', modelId: QWEN }),
        engine({ state: 'running', modelId: QWEN }),
      ],
    });
    const onRestoring = vi.fn();
    expect(await restoreServing(d, onRestoring)).toEqual({ phase: 'idle' });
    expect(onRestoring).toHaveBeenCalledWith({ kind: 'single', modelId: QWEN, peerName: null });
    expect(d.mount).toHaveBeenCalledWith(QWEN);
  });

  it('already serving it (another window, or goosed kept it): nothing is started', async () => {
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }],
      single: [engine({ state: 'running', modelId: QWEN })],
    });
    expect(await restoreServing(d, vi.fn())).toEqual({ phase: 'idle' });
    expect(d.mount).not.toHaveBeenCalled();
  });

  it('the memory gate refuses: the gate’s own words are the failure', async () => {
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }],
      mount: async () => {
        throw new MlxMountRefusedError({
          fit: { modelId: QWEN, verdict: 'block', message: 'model needs 30.6 GB, 12.0 GB free' },
        });
      },
    });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      what: { kind: 'single', modelId: QWEN },
      reason: { code: 'said', text: 'model needs 30.6 GB, 12.0 GB free' },
    });
  });

  it('the engine fails while loading: its own error', async () => {
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }],
      single: [
        engine({}),
        engine({ state: 'mounting', modelId: QWEN }),
        engine({ state: 'failed', modelId: QWEN, lastError: 'rapid-mlx exited 137' }),
      ],
    });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      reason: { code: 'said', text: 'rapid-mlx exited 137' },
    });
  });

  it('another Mac: waits for LeanZero Link’s own reconnect, rides out the roster catching up, follows to ready', async () => {
    const d = deps({
      intent: [REMOTE],
      remote: [{ state: 'off' }, { state: 'mounting' }, { state: 'ready' }],
      link: [RECONNECTING, RECONNECTING, CONNECTED],
      remoteStart: [
        { started: false, refusal: { code: 'unknownPeer', message: 'unknown peer' } },
        { started: true },
      ],
    });
    const onRestoring = vi.fn();
    expect(await restoreServing(d, onRestoring)).toEqual({ phase: 'idle' });
    expect(onRestoring).toHaveBeenCalledWith({
      kind: 'remoteSingle',
      modelId: QWEN,
      peerName: "Work's Mac Studio",
    });
    expect(d.remoteStart).toHaveBeenCalledTimes(2);
    expect(d.remoteStart).toHaveBeenLastCalledWith('worksmacstudio-lan-9c1e2a', QWEN);
  });

  it('Link’s reconnect failed: named, and nothing is asked of the other Mac', async () => {
    const failed = {
      auth: { state: 'loggedIn', email: 'm@x.co' },
      nodeCount: 0,
      intent: { intent: 'connected' },
      reconnect: { state: 'failed', reason: 'tailscaled did not start', at: 't' },
    } as unknown as LinkState;
    const d = deps({ intent: [REMOTE], link: [failed] });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      reason: { code: 'linkDown', detail: 'tailscaled did not start' },
    });
    expect(d.remoteStart).not.toHaveBeenCalled();
  });

  it('a peer that keeps refusing for a real reason is not retried: the refusal is the failure', async () => {
    const d = deps({
      intent: [REMOTE],
      remoteStart: [
        {
          started: false,
          refusal: { code: 'chatServingDisabled', message: 'Answer chat is off on the Studio' },
        },
      ],
    });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      reason: { code: 'said', text: 'Answer chat is off on the Studio' },
    });
    expect(d.remoteStart).toHaveBeenCalledTimes(1);
  });

  it('a peer that never appears: the retries end and the last refusal is said', async () => {
    const d = deps({
      intent: [REMOTE],
      remoteStart: [{ started: false, refusal: { code: 'unknownPeer', message: 'unknown peer' } }],
    });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      reason: { code: 'said', text: 'unknown peer' },
    });
    expect(d.remoteStart).toHaveBeenCalledTimes(REMOTE_START_TRIES);
  });

  it('the split: the saved config’s start (its preflight), followed to serving', async () => {
    const up = { mode: 'distributed', state: 'serving', modelId: FLASH } as MlxDistributedStatus;
    const d = deps({
      intent: [{ kind: 'split', modelId: FLASH }],
      distributed: [
        { mode: 'single', state: 'stopped' } as MlxDistributedStatus,
        { mode: 'distributed', state: 'starting' } as MlxDistributedStatus,
        up,
      ],
    });
    expect(await restoreServing(d, vi.fn())).toEqual({ phase: 'idle' });
    expect(d.distributedStart).toHaveBeenCalledTimes(1);
  });

  it('the split refused (its preflight): goose’s words', async () => {
    const d = deps({
      intent: [{ kind: 'split', modelId: FLASH }],
      distributed: [{ mode: 'single', state: 'stopped' } as MlxDistributedStatus],
      distributedStart: [
        { started: false, refusal: { code: 'preflightFailed', message: 'memory: short 4 GB' } },
      ],
    });
    expect(await restoreServing(d, vi.fn())).toMatchObject({
      phase: 'failed',
      what: { kind: 'split', modelId: FLASH },
      reason: { code: 'said', text: 'memory: short 4 GB' },
    });
  });

  // Q-77: after an update the previous split's ranks were still exiting when the restore's start
  // ran, and the restore gave up at once with internals in the words.
  const SHUTTING_DOWN = {
    started: false,
    refusal: {
      code: 'previousSplitShuttingDown',
      message:
        'The previous split is still shutting down on Mihai Macbook — start it again when that finishes',
      node: 'Mihai Macbook',
      detail: 'Mihai Macbook: rank pid 9425 (its parent pid 4242 still runs: shutting down)',
    },
  };

  it('the previous split still shutting down: the line says so and the start is asked again until it goes through', async () => {
    const up = { mode: 'distributed', state: 'serving', modelId: FLASH } as MlxDistributedStatus;
    const d = deps({
      intent: [{ kind: 'split', modelId: FLASH }],
      distributed: [{ mode: 'single', state: 'stopped' } as MlxDistributedStatus, up],
      distributedStart: [SHUTTING_DOWN, SHUTTING_DOWN, { started: true }],
    });
    const onRestoring = vi.fn();
    expect(await restoreServing(d, onRestoring)).toEqual({ phase: 'idle' });
    expect(d.distributedStart).toHaveBeenCalledTimes(3);
    const what = { kind: 'split', modelId: FLASH, peerName: null };
    expect(onRestoring.mock.calls).toEqual([
      [what],
      [what, 'Mihai Macbook'],
      [what],
      [what, 'Mihai Macbook'],
      [what],
    ]);
  });

  it('a Stop while it waits for the previous split ends the wait quietly — nothing is started', async () => {
    const d = deps({
      intent: [{ kind: 'split', modelId: FLASH }, null],
      distributed: [{ mode: 'single', state: 'stopped' } as MlxDistributedStatus],
      distributedStart: [SHUTTING_DOWN],
    });
    expect(await restoreServing(d, vi.fn())).toEqual({ phase: 'idle' });
    expect(d.distributedStart).toHaveBeenCalledTimes(1);
  });

  it('another MLX split (not goose’s): the refusal’s words, the pid only behind Details', async () => {
    const d = deps({
      intent: [{ kind: 'split', modelId: FLASH }],
      distributed: [{ mode: 'single', state: 'stopped' } as MlxDistributedStatus],
      distributedStart: [
        {
          started: false,
          refusal: {
            code: 'foreignSplit',
            message:
              'Another MLX split (not goose’s) is running on Mihai Macbook — stop it to start this one',
            node: 'Mihai Macbook',
            detail: 'pid 9425 `/Applications/Xcode.app/…/Python -c import base64,sys;exe`',
          },
        },
      ],
    });
    const result = await restoreServing(d, vi.fn());
    expect(result).toEqual({
      phase: 'failed',
      what: { kind: 'split', modelId: FLASH, peerName: null },
      reason: {
        code: 'said',
        text: 'Another MLX split (not goose’s) is running on Mihai Macbook — stop it to start this one',
        detail: 'pid 9425 `/Applications/Xcode.app/…/Python -c import base64,sys;exe`',
      },
    });
    expect(d.distributedStart).toHaveBeenCalledTimes(1);
  });

  it('the owner stops it while it comes up (the record is gone): that was a choice, no failure', async () => {
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }, null],
      single: [engine({}), engine({ state: 'mounting', modelId: QWEN }), engine({})],
    });
    expect(await restoreServing(d, vi.fn())).toEqual({ phase: 'idle' });
  });
});

describe('the restore’s one line, and what main is told', () => {
  afterEach(() => publishRestoreLine({ phase: 'idle' }));

  it('runRestore says "restoring" while it runs and clears the line once it serves; main hears both', async () => {
    const report = vi.fn();
    (window.electron as unknown as { mlxRestoreReport: typeof report }).mlxRestoreReport = report;
    const seen: string[] = [];
    const d = deps({
      intent: [{ kind: 'single', modelId: QWEN }],
      single: [engine({}), engine({ state: 'running', modelId: QWEN })],
    });
    d.mount.mockImplementation(async () => {
      seen.push(latestRestoreLine().phase);
    });
    await runRestore(d);
    expect(seen).toEqual(['restoring']);
    expect(latestRestoreLine()).toEqual({ phase: 'idle' });
    expect(report.mock.calls.map((c) => c[0]?.phase ?? null)).toEqual(['restoring', null]);
  });

  it('Try again runs the same restore with the calls the launch used', async () => {
    const d = deps({ intent: [null] });
    await runRestore(d);
    expect(d.readIntent).toHaveBeenCalledTimes(1);
    retryRestore();
    await vi.waitFor(() => expect(d.readIntent).toHaveBeenCalledTimes(2));
  });

  it('the report and the tray line: where, and why not', () => {
    const restoring = toRestoreReport({
      phase: 'restoring',
      what: { kind: 'remoteSingle', modelId: QWEN, peerName: "Work's Mac Studio" },
    })!;
    expect(isMlxRestoreReport(restoring)).toBe(true);
    expect(restoreTrayLine(restoring)).toBe(
      "Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio…"
    );
    const failed = toRestoreReport({
      phase: 'failed',
      what: { kind: 'single', modelId: QWEN, peerName: null },
      reason: { code: 'linkDown', detail: 'loggedOut' },
    })!;
    expect(restoreTrayLine(failed)).toBe(
      'Could not restore Qwen3.8-27B-Atlassian-Q8-mlx on this Mac: LeanZero Link is not connected (loggedOut)'
    );
    expect(isMlxRestoreReport({ ...failed, phase: 'done' })).toBe(false);
    const waiting = toRestoreReport({
      phase: 'restoring',
      what: { kind: 'split', modelId: QWEN, peerName: null },
      waitingOn: 'Mihai Macbook',
    })!;
    expect(isMlxRestoreReport(waiting)).toBe(true);
    expect(restoreTrayLine(waiting)).toBe(
      'The previous split is still shutting down on Mihai Macbook — goose restores it when that finishes'
    );

    const tray = buildMlxTrayModel(INITIAL_SNAPSHOT, {
      canAct: true,
      mountModelId: QWEN,
      distributed: null,
      restore: restoring,
    });
    expect(tray.title).toBe('Restoring…');
    expect(tray.phase).toBe('loading');
    expect(tray.items[0]).toEqual({
      type: 'info',
      label: "Restoring Qwen3.8-27B-Atlassian-Q8-mlx on Work's Mac Studio…",
      phase: 'loading',
    });
  });
});

describe('settleRestoreLine — a failed line clears once what it names serves', () => {
  const MODEL = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
  const failedOnStudio = () =>
    publishRestoreLine({
      phase: 'failed',
      what: { kind: 'remoteSingle', modelId: MODEL, peerName: "Work's Mac Studio" },
      reason: { code: 'said', text: 'engineUnreachable' },
    });

  it('the Studio serving the model clears "Could not restore … on Work’s Mac Studio"', () => {
    failedOnStudio();
    settleRestoreLine({
      single: null,
      remote: { state: 'ready', modelId: MODEL } as MlxRemoteSingleStatus,
      distributed: null,
    });
    expect(latestRestoreLine()).toEqual({ phase: 'idle' });
  });

  it('still mounting, another model, or this Mac serving instead: the line stays', () => {
    failedOnStudio();
    settleRestoreLine({
      single: { state: 'running', modelId: MODEL } as MlxEngineStatus,
      remote: { state: 'mounting', modelId: MODEL } as MlxRemoteSingleStatus,
      distributed: null,
    });
    settleRestoreLine({
      single: null,
      remote: { state: 'ready', modelId: 'other/model' } as MlxRemoteSingleStatus,
      distributed: null,
    });
    expect(latestRestoreLine().phase).toBe('failed');
  });
});
