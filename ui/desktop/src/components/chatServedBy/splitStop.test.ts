import { describe, expect, it } from 'vitest';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import type { MountLookup } from '../noNodeNotice/mlxMount';
import { createIntl } from 'react-intl';
import {
  deriveChatServedBy,
  servedReady,
  splitStopAt,
  type ChatServedInputs,
  type SplitStop,
} from './chatServedBy';
import { splitStopHeadline, splitStopMemory, splitStopReason } from './splitStopText';
import {
  ALIAS,
  DIED_MS,
  HF,
  MACBOOK,
  RANK_DIED_MESSAGE,
  READY_MS,
  SPLIT_STOPPED_E2E2,
  STUDIO,
  WARN_MS,
} from './splitStop.fixtures';

const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const MLX_NODE: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};
const POOL: MountLookup = { state: 'ready', devices: [MLX_NODE], settings: SETTINGS, intent: null };
const STOPPED: MlxEngineStatus = {
  state: 'stopped',
  restartRequired: false,
  availableMemoryGb: 61.2,
  totalMemoryGb: 128,
};

const inputs = (over: Partial<ChatServedInputs>): ChatServedInputs => ({
  provider: 'swarm',
  lookup: POOL,
  single: STOPPED,
  distributed: SPLIT_STOPPED_E2E2,
  remote: null,
  remoteReadError: null,
  main: null,
  sessionId: 's-mine',
  turnInFlight: false,
  thisMac: 'This Mac',
  engineLabel: 'LeanZero MLX',
  ...over,
});

const intl = createIntl({ locale: 'en', defaultLocale: 'en' });

const STOP: SplitStop = {
  mac: STUDIO,
  cause: 'memory',
  availableGb: 3.9,
  totalGb: 96,
  atMs: DIED_MS,
  raw: RANK_DIED_MESSAGE,
  macs: [MACBOOK, STUDIO],
  modelId: HF,
};

describe('splitStopAt — the split that served chat stopped (Q-81, E2E #2)', () => {
  it('reads the supervisor’s events: the Studio’s rank died two seconds after its memory warning', () => {
    expect(splitStopAt(SPLIT_STOPPED_E2E2, null)).toEqual(STOP);
    expect(splitStopHeadline(intl, STOP)).toBe(
      'The split across your Macs stopped — Work’s Mac Studio ran out of memory'
    );
    expect(splitStopMemory(intl, STOP)).toBe(
      'Work’s Mac Studio had 3.9 GB of 96.0 GB free when it stopped.'
    );
  });

  it('a refusal written after the stop names it; one written while the split still served does not', () => {
    const refusedAt = Math.floor((DIED_MS + 600) / 1000) * 1000;
    expect(splitStopAt(SPLIT_STOPPED_E2E2, refusedAt + 999)?.mac).toBe(STUDIO);
    expect(splitStopAt(SPLIT_STOPPED_E2E2, WARN_MS)).toBeNull();
    // The answer the stop CUT began before it: its start, with no bound on when the stop came.
    expect(splitStopAt(SPLIT_STOPPED_E2E2, WARN_MS - 60_000, null)?.cause).toBe('memory');
    // A turn from before the split was ever ready is not its.
    expect(splitStopAt(SPLIT_STOPPED_E2E2, READY_MS - 1, null)).toBeNull();
  });

  it('the claim ends with the fact that ends it: a relaunch owns the Mac, another window owns it', () => {
    const relaunching: MlxDistributedStatus = {
      ...SPLIT_STOPPED_E2E2,
      mode: 'distributed',
      state: 'preflight',
    };
    expect(splitStopAt(relaunching, null)).toBeNull();
    const otherWindow: MlxDistributedStatus = {
      ...SPLIT_STOPPED_E2E2,
      owner: { state: 'answering', nodeNames: [MACBOOK, STUDIO] },
    };
    expect(splitStopAt(otherWindow, null)).toBeNull();
  });

  it('a stop the user asked for, or a split that never became ready, is not a failure to report', () => {
    const asked: MlxDistributedStatus = {
      ...SPLIT_STOPPED_E2E2,
      state: 'stopped',
      events: [
        ...SPLIT_STOPPED_E2E2.events.slice(0, 3),
        { atMs: WARN_MS, kind: 'stopRequested', message: 'stop requested' },
        { atMs: WARN_MS + 200, kind: 'stopped', message: 'verified' },
      ],
    };
    expect(splitStopAt(asked, null)).toBeNull();
    const neverReady: MlxDistributedStatus = {
      ...SPLIT_STOPPED_E2E2,
      events: SPLIT_STOPPED_E2E2.events.filter((e) => e.kind !== 'ready'),
    };
    expect(splitStopAt(neverReady, null)).toBeNull();
  });

  it('names what it knows and nothing more: a frozen rank, a hang with no Mac, a death with no warning', () => {
    const withDeath = (kind: string, node?: string) => ({
      ...SPLIT_STOPPED_E2E2,
      events: [
        ...SPLIT_STOPPED_E2E2.events.slice(0, 3),
        { atMs: DIED_MS, kind, node, message: `${kind} message` },
      ],
      lastError: null,
    });
    const frozen = splitStopAt(withDeath('rankFrozen', STUDIO), null)!;
    expect(splitStopReason(intl, frozen)).toBe('Work’s Mac Studio stopped responding');
    expect(splitStopMemory(intl, frozen)).toBeNull();
    const hang = splitStopAt(withDeath('hang'), null)!;
    expect(hang.mac).toBeNull();
    expect(splitStopReason(intl, hang)).toBe('the Macs stopped making progress');
    const died = splitStopAt(withDeath('rankDied', STUDIO), null)!;
    expect(died.raw).toBe('rankDied message');
    expect(splitStopReason(intl, died)).toBe('Work’s Mac Studio’s part of the model stopped');
  });
});

describe('deriveChatServedBy — the split stopped, said everywhere (Q-81)', () => {
  it('E2E #2: the chip names the split in red and the bar is the split’s — never "No model is mounted"', () => {
    const served = deriveChatServedBy(inputs({}));
    expect(served).toEqual({
      engine: 'none',
      model: HF,
      where: [MACBOOK, STUDIO],
      peerNodeId: null,
      foreign: false,
      contextWindow: null,
      phase: 'failed',
      activity: null,
      busyWithOthers: null,
      turnRequest: null,
      readTps: null,
      readiness: {
        kind: 'split-stopped',
        stop: STOP,
        status: SPLIT_STOPPED_E2E2,
        instead: { kind: 'switch', mount: HF },
      },
    });
    expect(servedReady(served)).toBe(false);
  });

  it('the omlx provider rides the same rule', () => {
    const served = deriveChatServedBy(inputs({ provider: 'omlx' }));
    expect(served.readiness.kind).toBe('split-stopped');
  });

  it('"Run on one Mac instead" taken: this Mac mounting or serving is what chat is on now', () => {
    const mounting = deriveChatServedBy(
      inputs({ single: { ...STOPPED, state: 'mounting', modelId: HF, servedModelId: ALIAS } })
    );
    expect(mounting.readiness).toMatchObject({ kind: 'unmounted', fact: 'mounting' });
    const running = deriveChatServedBy(
      inputs({ single: { ...STOPPED, state: 'running', modelId: HF, servedModelId: ALIAS } })
    );
    expect(running.readiness).toEqual({ kind: 'ready' });
    expect(running.engine).toBe('single');
  });

  it('a pool node that wants another model was never on the split: the plain unmounted bar', () => {
    const served = deriveChatServedBy(
      inputs({
        lookup: {
          ...POOL,
          devices: [{ ...MLX_NODE, model_id: 'other-model' }],
          settings: { ...SETTINGS, servedModelName: 'other-model' },
        },
      })
    );
    expect(served.readiness.kind).toBe('unmounted');
  });
});
