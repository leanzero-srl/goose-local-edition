import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineSettings, MlxEngineStatus } from '../../acp/mlx-engine';
import { mlxDistributedStatus, type MlxDistributedStatus } from '../../acp/mlx-distributed';
import { mlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import { createUserMessage, type Message } from '../../types/message';
import { assertStudioClean } from '../lz/assertStudioClean';
import GooseMessage from '../GooseMessage';
import NoNodeNotice from './NoNodeNotice';
import { parseNoNodeError } from './parseNoNodeError';
import { SPLIT_RECORD_MARKER, takeSplitRecord } from '../chatServedBy/splitRecord';
import { SPLIT_STOPPED_E2E2 } from '../chatServedBy/splitStop.fixtures';

const mockStatus = vi.fn<() => Promise<MlxEngineStatus>>();
const mockSettings = vi.fn<() => Promise<MlxEngineSettings>>();
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: () => mockStatus(),
  mlxEngineMount: vi.fn(),
  mlxEngineSettingsRead: () => mockSettings(),
}));
const mockExtMethod = vi.fn();
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));
const mockRead = vi.fn();
vi.mock('../ConfigContext', () => ({
  useConfig: () => ({ read: mockRead }),
}));

const HF = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const ALIAS = 'mihai-qwen3.8-27b-atlassian-q8-mlx';
const STUDIO = 'Work’s Mac Studio';
const MACBOOK = 'Mihai Macbook';

/**
 * E2E #3 (installed 3.0.47, 2026-09-26, UTC+3): the 27B tensor split served from 09:4x; the turn
 * began at 09:50; at 10:07:57 the progress-ratio rule SIGTERMed both ranks (Q-114). The chat then
 * showed "Network error: … no [DONE] after 9771 data frames" and, for the next five messages, the
 * router's refusal. The record is what agents/split_record.rs writes into each failure text.
 */
const READY_MS = Date.UTC(2026, 8, 26, 6, 44, 10, 0);
const TURN_MS = Date.UTC(2026, 8, 26, 6, 50, 2, 511);
const HANG_MS = Date.UTC(2026, 8, 26, 7, 7, 57, 212);
const HANG_WORDS =
  'progress-ratio rule: samples 40, median 2016 ms, bound 20160 ms (10× median), silent 20298 ms — the rank-0 step counter (last Some(3041)) and every rank\'s CPU time stood still; rank ps stats ["R 97%", "S 0%"]';

const HUNG_EVENTS = [
  {
    atMs: READY_MS,
    kind: 'ready',
    node: null,
    message: 'the readiness completion ended with [DONE]',
  },
  { atMs: HANG_MS, kind: 'hang', node: null, message: HANG_WORDS },
  {
    atMs: HANG_MS,
    kind: 'streamWithoutDone',
    node: null,
    message: '1 in-flight request(s) cut by the hang',
  },
  { atMs: HANG_MS + 1_400, kind: 'stopped', node: null, message: 'after hang: verified' },
];

function recordLine(over: Record<string, unknown> = {}): string {
  return `${SPLIT_RECORD_MARKER}${JSON.stringify({
    v: 1,
    state: 'failed',
    turnStartedMs: TURN_MS,
    modelId: HF,
    nodes: [MACBOOK, STUDIO],
    configNodes: null,
    events: HUNG_EVENTS,
    ...over,
  })}`;
}

const NETWORK_ERROR =
  'Network error: Stream decode error: stream ended before completion: no finish_reason and no [DONE] after 9771 data frames — the answer is incomplete';
const RESEND = 'Please resend your message to try again.';
const cutText = (record: string | null) =>
  record ? `${NETWORK_ERROR}\n\n${record}\n\n${RESEND}` : `${NETWORK_ERROR}\n\n${RESEND}`;

const REFUSAL =
  "Ran into this error: Execution error: swarm chat: no node can serve this turn — mihai-mlx: MLX engine is not listening on http://127.0.0.1:8090 — mount it in the MLX window (this process's manager: stopped; error sending request for url (http://127.0.0.1:8090/v1/models)).";
const RETRY_CLOSER = 'Please retry if you think this is a transient or recoverable error.';
const refusalText = (record: string | null) =>
  record ? `${REFUSAL}\n\n${record}\n\n${RETRY_CLOSER}` : `${REFUSAL}\n\n${RETRY_CLOSER}`;

const SETTINGS: MlxEngineSettings = {
  modelId: HF,
  servedModelName: ALIAS,
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const DEVICE = {
  id: 'mihai-mlx',
  model_id: ALIAS,
  weight: 2,
  enabled: true,
  engine: 'mlx-sidecar',
};

function wrap(ui: React.ReactElement) {
  return render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter initialEntries={['/pair']}>
        <Routes>
          <Route path="/pair" element={ui} />
        </Routes>
      </MemoryRouter>
    </IntlProvider>
  );
}

let nextId = 0;
function assistant(text: string | string[], createdMs: number): Message {
  const parts = Array.isArray(text) ? text : [text];
  return {
    id: `a${nextId++}`,
    role: 'assistant',
    created: Math.floor(createdMs / 1000),
    content: parts.map((t) => ({ type: 'text' as const, text: t })),
    metadata: { userVisible: true, agentVisible: true },
  };
}

function show(message: Message, history: Message[], append = vi.fn()) {
  return wrap(
    <GooseMessage
      sessionId="s1"
      message={message}
      messages={history}
      toolCallNotifications={new Map()}
      append={append}
      isStreaming={false}
    />
  );
}

async function publishSplit(status: MlxDistributedStatus | null) {
  if (status) mockExtMethod.mockResolvedValueOnce({ status });
  else mockExtMethod.mockRejectedValueOnce(new Error('no split read'));
  await act(async () => {
    await mlxDistributedStatus().catch(() => undefined);
  });
}

async function publishRoute(state: string) {
  mockExtMethod.mockResolvedValueOnce({
    status: {
      state,
      peer: 'remote-worksmacstudio-lan-9c1e2a',
      peerHostname: 'worksmacstudio-lan-9c1e2a',
      peerComputerName: STUDIO,
      modelId: HF,
      servedModelId: ALIAS,
    },
  });
  await act(async () => {
    await mlxRemoteSingleStatus();
  });
}

beforeEach(async () => {
  vi.clearAllMocks();
  mockStatus.mockResolvedValue({
    state: 'stopped',
    restartRequired: false,
    availableMemoryGb: 61.2,
    totalMemoryGb: 128,
  });
  mockRead.mockResolvedValue({ devices: [DEVICE] });
  mockSettings.mockResolvedValue(SETTINGS);
  // After the relaunch: the split's events are gone, the route is off.
  await publishSplit(null);
  await publishRoute('off');
});

afterEach(async () => {
  await publishSplit(null);
  await publishRoute('off');
});

describe('takeSplitRecord — the record comes out of the text and the text is what it was', () => {
  it('leaves the error and the closer exactly as a failure with no record reads', () => {
    const taken = takeSplitRecord(cutText(recordLine()));
    expect(taken.kind).toBe('record');
    expect(taken.text).toBe(cutText(null));
    if (taken.kind !== 'record') return;
    expect(taken.record.turnStartedMs).toBe(TURN_MS);
    expect(taken.record.macs).toEqual([MACBOOK, STUDIO]);
    expect(taken.record.events.map((e) => e.kind)).toEqual([
      'ready',
      'hang',
      'streamWithoutDone',
      'stopped',
    ]);
  });

  it('an unreadable record stays in the text, seen — never read as a cause', () => {
    const text = cutText(`${SPLIT_RECORD_MARKER}{"v":1,"state":`);
    const taken = takeSplitRecord(text);
    expect(taken.kind).toBe('unreadable');
    expect(taken.text).toBe(text);
  });

  it('text with no record is untouched', () => {
    expect(takeSplitRecord(refusalText(null))).toEqual({ kind: 'none', text: refusalText(null) });
  });
});

describe('Q-121: history after a relaunch says what the live notice said', () => {
  it('five refusals after the stop each render the split and why — never "No model is mounted", never Mount', async () => {
    const user = userEvent.setup();
    const userTurns = [0, 1, 2, 3, 4].map((i) => createUserMessage(`turn ${i}`));
    const refusals = [0, 1, 2, 3, 4].map((i) =>
      assistant(refusalText(recordLine({ turnStartedMs: HANG_MS + 30_000 + i })), HANG_MS + 31_000)
    );
    const history = userTurns.flatMap((u, i) => [u, refusals[i]]);
    for (const refusal of refusals) {
      const { container, unmount } = show(refusal, history);
      const notice = screen.getByTestId('no-node-split-stopped');
      expect(notice.textContent).toContain('The split across your Macs stopped');
      expect(notice.textContent).not.toContain('No model is mounted');
      expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
        'the Macs stopped making progress — nothing else could take this message.'
      );
      expect(screen.queryByRole('button', { name: /Mount/ })).toBeNull();
      expect(container.textContent).not.toContain(SPLIT_RECORD_MARKER);
      await user.click(screen.getByRole('button', { name: /Details/ }));
      expect(screen.getByTestId('no-node-split-raw').textContent).toBe(HANG_WORDS);
      assertStudioClean(container);
      unmount();
    }
  });

  it('the LIVE last refusal offers Retry and Open Engine — no Mount of this Mac while chat is on the Studio', async () => {
    await publishRoute('ready');
    const user = userEvent.setup();
    const append = vi.fn();
    const turn = createUserMessage('Two things to remember about how I work');
    const refusal = assistant(refusalText(recordLine()), HANG_MS + 31_000);
    show(refusal, [turn, refusal], append);
    expect(screen.getByTestId('no-node-split-stopped')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Mount/ })).toBeNull();
    expect(screen.getByTestId('no-node-open-engine')).toBeInTheDocument();
    await user.click(screen.getByTestId('no-node-retry'));
    expect(append).toHaveBeenCalledWith('Two things to remember about how I work');
  });

  it('a refusal with no record and no live events stays the plain notice (an older message)', () => {
    const turn = createUserMessage('hi');
    const older = assistant(refusalText(null), HANG_MS + 31_000);
    const later = createUserMessage('again');
    show(older, [turn, older, later]);
    expect(screen.getByTestId('no-node-notice').textContent).toContain('No model is mounted');
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
  });
});

describe('Q-121: a notice never offers Mount on this Mac while chat is served another way', () => {
  const rows = parseNoNodeError(refusalText(null))!;

  it('the Studio route serves chat: the row says so, no Mount, Retry is the action', async () => {
    await publishRoute('ready');
    const onRetry = vi.fn();
    const { container } = wrap(
      <NoNodeNotice rows={rows} live retryText="hello" onRetry={onRetry} />
    );
    const cell = await screen.findByTestId('no-node-served-elsewhere-mihai-mlx');
    expect(cell.textContent).toBe('Chat goes to Work’s Mac Studio now');
    expect(screen.queryByTestId('no-node-mount-mihai-mlx')).toBeNull();
    expect(screen.queryByRole('button', { name: /Mount/ })).toBeNull();
    await userEvent.setup().click(screen.getByTestId('no-node-retry'));
    expect(onRetry).toHaveBeenCalledWith('hello');
    assertStudioClean(container);
  });

  it('with the route off the same live row still offers Mount — the rule is the serving state', async () => {
    mockRead.mockResolvedValue({ devices: [DEVICE] });
    wrap(<NoNodeNotice rows={rows} live retryText="hello" onRetry={vi.fn()} />);
    expect(await screen.findByTestId('no-node-mount-mihai-mlx')).toBeInTheDocument();
    expect(screen.queryByTestId('no-node-served-elsewhere-mihai-mlx')).toBeNull();
  });
});

describe('Q-122: the answer the split stop cut says the split stopped, not "Network error"', () => {
  const turn = createUserMessage('Please turn this into a clean notes file at notes/kickoff.md');

  it('no answer written yet: the split-stop notice IS the error; the stream error only behind Details', async () => {
    const user = userEvent.setup();
    const cut = assistant(cutText(recordLine()), TURN_MS);
    const { container } = show(cut, [turn, cut]);
    const notice = screen.getByTestId('no-node-split-stopped');
    expect(notice.textContent).toContain('The split across your Macs stopped mid-answer');
    expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
      'the Macs stopped making progress — the answer was cut before any of it was written.'
    );
    expect(screen.getByTestId('no-node-split-error')).not.toBeVisible();
    expect(screen.queryByText(/Please resend your message/)).not.toBeVisible();
    await user.click(screen.getByRole('button', { name: /Details/ }));
    expect(screen.getByTestId('no-node-split-error').textContent).toBe(cutText(null));
    expect(container.textContent).not.toContain(SPLIT_RECORD_MARKER);
    assertStudioClean(container);
  });

  it('a partial answer is kept as written and the notice says the answer above stops there', () => {
    const cut = assistant(
      ['## Decisions\n\nStandard vs premium: Aoife wants', cutText(recordLine())],
      TURN_MS
    );
    show(cut, [turn, cut]);
    expect(screen.getByText(/Aoife wants/)).toBeInTheDocument();
    expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
      'the Macs stopped making progress — the answer above stops there.'
    );
  });

  it('a stop from BEFORE this call began did not cut it: the error is shown as goose wrote it', () => {
    const cut = assistant(
      cutText(recordLine({ turnStartedMs: HANG_MS + 60_000 })),
      HANG_MS + 90_000
    );
    show(cut, [turn, cut]);
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
    expect(screen.getByTestId('network-cut-raw').textContent).toContain(
      'no [DONE] after 9771 data frames'
    );
  });

  it('a network error with no record is left exactly as before', () => {
    const cut = assistant(cutText(null), TURN_MS);
    show(cut, [turn, cut]);
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
    expect(screen.queryByTestId('network-cut-raw')).toBeNull();
    expect(screen.getByText(/no \[DONE\] after 9771 data frames/)).toBeInTheDocument();
  });

  it('E2E #2: the rank died before the supervisor wrote it — the stop comes from the live events after the call began', async () => {
    const serving = recordLine({
      state: 'serving',
      turnStartedMs: SPLIT_STOPPED_E2E2.events[2].atMs + 60_000,
      events: [SPLIT_STOPPED_E2E2.events[2]],
    });
    const cut = assistant(cutText(serving), SPLIT_STOPPED_E2E2.events[5].atMs);
    const { unmount } = show(cut, [turn, cut]);
    // After a relaunch nothing says why: the error as written, no guessed cause.
    expect(screen.queryByTestId('no-node-split-stopped')).toBeNull();
    expect(screen.getByTestId('network-cut-raw')).toBeInTheDocument();
    unmount();
    await publishSplit(SPLIT_STOPPED_E2E2);
    show(cut, [turn, cut]);
    expect(screen.getByTestId('no-node-split-summary').textContent).toBe(
      'Work’s Mac Studio ran out of memory — the answer was cut before any of it was written.'
    );
  });
});
