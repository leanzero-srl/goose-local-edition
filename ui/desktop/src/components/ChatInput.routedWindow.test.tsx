import React from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeAll, beforeEach } from 'vitest';
import type { NodeServedTurnDto } from '@aaif/goose-sdk';
import ChatInput from './ChatInput';
import { ChatState } from '../types/chatState';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { ChatServedBy } from './chatServedBy/chatServedBy';

/**
 * Q-467: a chat on a STRATEGY showed its counter against the pool's MLX engine window ("42k /
 * 262k", the split's) while another node answered it — deepseek, or the Studio single. Compaction
 * and the context line read the served node's window since Q-463; the counter reads the same
 * record goosed keeps per session (`nodes/servedLast`), and says "window unknown" when that node
 * reported none. A chat that is not on a strategy keeps its own rule.
 */

const POOL_WINDOW = 262_144;
let served: ChatServedBy;
let servedRecord: NodeServedTurnDto | null | undefined;
vi.mock('./chatServedBy/useChatServedBy', () => ({
  useChatServedBy: () => ({ served, single: null, armed: true, turnInFlight: false, servedRecord }),
}));
vi.mock('./swarm/swarmContextLimit', () => ({
  fetchSwarmPoolContextLimit: async () => POOL_WINDOW,
}));
vi.mock('./noNodeNotice/ComposerReadiness', () => ({ ComposerReadinessStrip: () => null }));
vi.mock('./bottom_menu/ContextWindowIndicator', () => ({
  ContextWindowIndicator: ({
    tokenLimit,
    windowUnknown,
  }: {
    tokenLimit: number;
    windowUnknown?: boolean;
  }) =>
    windowUnknown ? (
      <div data-testid="context-indicator" data-window="unknown" />
    ) : tokenLimit ? (
      <div data-testid="context-indicator" data-limit={tokenLimit} />
    ) : null,
}));
vi.mock('./settings/models/bottom_bar/ModelsBottomBar', () => ({
  default: () => <div data-testid="models-bottom-bar" />,
}));
vi.mock('./bottom_menu/DirSwitcher', () => ({ DirSwitcher: () => null }));
vi.mock('./bottom_menu/CostTracker', () => ({ CostTracker: () => null }));
vi.mock('./MentionPopover', () => ({
  default: React.forwardRef(function MentionPopoverMock() {
    return null;
  }),
}));
vi.mock('../hooks/useAudioRecorder', () => ({
  useAudioRecorder: () => ({
    isEnabled: false,
    dictationProvider: null,
    isRecording: false,
    isTranscribing: false,
    startRecording: vi.fn(),
    stopRecording: vi.fn(),
  }),
}));
vi.mock('./ModelAndProviderContext', () => ({
  useModelAndProvider: () => ({
    getCurrentModelAndProvider: async () => ({ model: 'swarm', provider: 'swarm' }),
    currentModel: 'swarm',
    currentProvider: 'swarm',
  }),
}));
vi.mock('./alerts', () => ({
  useAlerts: () => ({ alerts: [], addAlert: vi.fn(), clearAlerts: vi.fn() }),
  AlertType: { Error: 'error', Warning: 'warning', Info: 'info' },
}));
vi.mock('./ui/ReportProblemDialog', async (original) => ({
  ...(await original<typeof import('./ui/ReportProblemDialog')>()),
  ReportProblemDialog: () => null,
}));

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}
beforeAll(() => {
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
});

/** The pool's split serves this Mac's chat at 262,144 — the number the counter used to show. */
const splitServing = (): ChatServedBy =>
  ({
    engine: 'split',
    model: 'mlx-community/Qwen3.6-27B-8bit',
    where: ['This Mac', 'Work'],
    peerNodeId: null,
    foreign: false,
    contextWindow: POOL_WINDOW,
    phase: 'idle',
    activity: null,
    work: null,
    busyWithOthers: null,
    busyIn: null,
    turnRequest: null,
    turnWait: null,
    readTps: null,
    readiness: { kind: 'ready' },
  }) as unknown as ChatServedBy;

const record = (node: string, contextWindow?: number): NodeServedTurnDto => ({
  node,
  role: 'chat',
  rank: 1,
  tried: [],
  atMs: 1,
  ...(contextWindow != null ? { contextWindow } : {}),
});

const input = (sessionId: string, model: string, provider = 'swarm') => (
  <IntlTestWrapper>
    <ChatInput
      sessionId={sessionId}
      handleSubmit={vi.fn()}
      chatState={ChatState.Idle}
      setView={vi.fn()}
      sessionModel={model}
      sessionProvider={provider}
      sessionLoaded
      workingDir="/tmp"
      totalTokens={42_000}
    />
  </IntlTestWrapper>
);

const indicator = () => screen.getByTestId('context-indicator');
const settle = () => new Promise((r) => setTimeout(r, 20));

describe('a strategy chat counts against the node that served it (Q-467)', () => {
  beforeEach(() => {
    served = splitServing();
    servedRecord = undefined;
  });

  it('served by the Studio single at 131,072: that window, not the pool’s 262,144', async () => {
    servedRecord = record('studio', 131_072);
    render(input('s1', 'strategy:studio-chat'));
    await waitFor(() => expect(indicator().getAttribute('data-limit')).toBe('131072'));
  });

  it('served by deepseek, which reports no window: "window unknown", never the pool’s', async () => {
    servedRecord = record('deepseek-v4-1-flash-openrouter');
    render(input('s1', 'strategy:studio-chat'));
    await waitFor(() => expect(indicator().getAttribute('data-window')).toBe('unknown'));
    expect(indicator().getAttribute('data-limit')).toBeNull();
  });

  it('the Studio served turn 1, deepseek turn 2: the Studio’s window is not held', async () => {
    servedRecord = record('studio', 131_072);
    const view = render(input('s1', 'strategy:studio-chat'));
    await waitFor(() => expect(indicator().getAttribute('data-limit')).toBe('131072'));
    servedRecord = record('deepseek-v4-1-flash-openrouter');
    view.rerender(input('s1', 'strategy:studio-chat'));
    await waitFor(() => expect(indicator().getAttribute('data-window')).toBe('unknown'));
    servedRecord = undefined;
    view.rerender(input('s1', 'strategy:studio-chat'));
    await settle();
    expect(screen.queryByTestId('context-indicator')?.getAttribute('data-limit') ?? null).not.toBe(
      '131072'
    );
  });
});

describe('a node chat counts against its own node (Q-467)', () => {
  beforeEach(() => {
    served = splitServing();
    servedRecord = undefined;
  });

  it('node:studio served at 131,072: that window, not the pool’s 262,144', async () => {
    servedRecord = { ...record('studio', 131_072), role: undefined };
    render(input('s1', 'node:studio'));
    await waitFor(() => expect(indicator().getAttribute('data-limit')).toBe('131072'));
  });

  it('node:deepseek, which reports no window: "window unknown", never the pool’s', async () => {
    servedRecord = { ...record('deepseek-v4-1-flash-openrouter'), role: undefined };
    render(input('s1', 'node:deepseek-v4-1-flash-openrouter'));
    await waitFor(() => expect(indicator().getAttribute('data-window')).toBe('unknown'));
    expect(indicator().getAttribute('data-limit')).toBeNull();
  });
});

describe('a plain MLX/provider chat is unchanged (Q-467)', () => {
  beforeEach(() => {
    served = splitServing();
    servedRecord = undefined;
  });

  it('Auto (the pool) still reads the pool’s window', async () => {
    render(input('s1', 'swarm'));
    await waitFor(() => expect(indicator().getAttribute('data-limit')).toBe(String(POOL_WINDOW)));
  });

  it('the LeanZero MLX engine (one engine) still reads the engine that serves it', async () => {
    render(input('s1', 'qwen-27b', 'omlx'));
    await waitFor(() => expect(indicator().getAttribute('data-limit')).toBe(String(POOL_WINDOW)));
  });
});
