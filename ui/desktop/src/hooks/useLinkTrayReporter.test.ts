import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createIntl } from 'react-intl';
import type { LinkState, NodeState, NodesResponse } from '../acp/leanzero-link';
import type { MlxEngineStatus } from '../acp/mlx-engine';

const mockNodes = vi.fn();
const mockStatus = vi.fn();
const mockLive = vi.fn();

vi.mock('../acp/leanzero-link', async (importActual) => ({
  ...(await importActual<typeof import('../acp/leanzero-link')>()),
  leanzeroLinkNodes: (...a: unknown[]) => mockNodes(...a),
}));
vi.mock('../acp/mlx-engine', async (importActual) => ({
  ...(await importActual<typeof import('../acp/mlx-engine')>()),
  mlxEngineStatus: (...a: unknown[]) => mockStatus(...a),
}));
vi.mock('../components/leanzero-swarm/mlxLiveStats', async (importActual) => ({
  ...(await importActual<typeof import('../components/leanzero-swarm/mlxLiveStats')>()),
  readMlxLiveStatus: (...a: unknown[]) => mockLive(...a),
}));

import { readMacsTrayReport } from './useLinkTrayReporter';
import { parseMlxLiveStatus } from '../components/leanzero-swarm/mlxLiveStats';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

const node = (overrides: Partial<NodeState>): NodeState => ({
  node_id: 'n',
  hostname: 'host',
  status: { type: 'Idle' },
  sessions_active: 0,
  updated_at: '2026-09-24T12:00:00Z',
  ...overrides,
});

const ROSTER: NodesResponse = {
  self: node({ node_id: 'self-node', computer_name: 'Mihai’s MacBook' }),
  peers: [
    node({ node_id: 'studio-1', computer_name: 'Work’s Mac Studio' }),
    node({
      node_id: 'mini-2',
      computer_name: 'Mini',
      allows: { manage_models: false, answer_chat: false, run_split: false },
    }),
  ],
};

const running = (modelId: string): MlxEngineStatus => ({
  state: 'running',
  modelId,
  baseUrl: 'http://127.0.0.1:8095',
  restartRequired: false,
  availableMemoryGb: 60,
  totalMemoryGb: 128,
});

const connected = { auth: { state: 'connected' } } as unknown as LinkState;

beforeEach(() => {
  vi.clearAllMocks();
  mockNodes.mockResolvedValue(ROSTER);
});

describe('the tray reads every Mac the way My Macs does', () => {
  it('one line per Mac: this Mac’s live rate, a peer’s model, a switched-off peer never asked', async () => {
    mockStatus.mockImplementation(async (nodeId?: string) =>
      nodeId === 'studio-1'
        ? { ...running('mlx-community/Qwen3-30B-A3B-4bit'), baseUrl: undefined }
        : running('Mihai-LeanZero/Qwen3.8-27B')
    );
    mockLive.mockResolvedValue(
      parseMlxLiveStatus({
        status: 'running',
        uptime_s: 12,
        num_running: 1,
        num_waiting: 0,
        requests: [
          {
            request_id: 'r1',
            phase: 'generation',
            status: 'running',
            completion_tokens: 40,
            tokens_per_second: 21.9,
          },
        ],
      })
    );
    const report = await readMacsTrayReport(intl, connected);
    expect(report?.openLabel).toBe('Open My Macs');
    expect(report?.lines).toEqual([
      {
        name: 'Mihai’s MacBook',
        phase: 'writing',
        text: 'Mihai’s MacBook — Qwen3.8-27B · 21.9 tok/s',
      },
      { name: 'Work’s Mac Studio', phase: 'idle', text: 'Work’s Mac Studio — Qwen3-30B-A3B-4bit' },
      { name: 'Mini', phase: null, text: 'Mini — Off' },
    ]);
    expect(mockStatus).not.toHaveBeenCalledWith('mini-2');
    // Only this Mac's engine is read live; a peer's activity is never guessed from ours.
    expect(mockLive).toHaveBeenCalledTimes(1);
  });

  it('a Mac whose status read fails is Can’t read in red — never silently idle', async () => {
    mockStatus.mockImplementation(async (nodeId?: string) => {
      if (nodeId === 'studio-1') throw new Error('connection refused');
      return { ...running('m'), state: 'stopped', modelId: undefined };
    });
    const report = await readMacsTrayReport(intl, connected);
    expect(report?.lines[0]).toMatchObject({
      phase: 'unloaded',
      text: 'Mihai’s MacBook — No model loaded',
    });
    expect(report?.lines[1]).toMatchObject({
      phase: 'failed',
      text: 'Work’s Mac Studio — Can’t read',
    });
  });

  it('not on the mesh: no Mac lines, the tray keeps its Link line', async () => {
    const report = await readMacsTrayReport(intl, {
      auth: { state: 'loggedIn' },
    } as unknown as LinkState);
    expect(report).toBeNull();
    expect(mockNodes).not.toHaveBeenCalled();
  });
});
