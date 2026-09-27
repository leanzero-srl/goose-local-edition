import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';

// jsdom has no Element.scrollTo; the chat wizard follows its own scroll after every message.
if (!window.Element.prototype.scrollTo) {
  window.Element.prototype.scrollTo = () => {};
}

/**
 * The recipe wizard talks to whichever fleet engine SERVES a model. Measured 2026-09-05 on an MLX-only
 * machine: LM Studio discovery is off by default and found nothing, so the wizard read "no fleet model is
 * loaded — start LM Studio" while the LeanZero MLX sidecar served `workhorse-qwen3.5-9b-4bit-mlx`.
 */
const fleetMock = {
  online: false,
  models: [] as string[],
  loading: false,
  endpoint: 'http://127.0.0.1:1234',
};
vi.mock('./useFleet', () => ({ useFleet: () => fleetMock }));
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({ useLmStudioFleetVisible: () => false }));
let mlxStatus: MlxEngineStatus | null = null;
vi.mock('../leanzero-swarm/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: () => ({ status: mlxStatus, error: null }),
}));
vi.mock('../../recipe/recipe_management', () => ({ saveRecipe: async () => undefined }));
// The split and the route, as the shared stores hold them (Q-7).
let distStatus: MlxDistributedStatus | null = null;
let routeStatus: MlxRemoteSingleStatus | null = null;
vi.mock('../noNodeNotice/mlxMount', async (original) => ({
  ...(await original<typeof import('../noNodeNotice/mlxMount')>()),
  useLatestMlxDistributedStatus: () => distStatus,
}));
vi.mock('../../acp/mlx-distributed', async (original) => ({
  ...(await original<typeof import('../../acp/mlx-distributed')>()),
  mlxDistributedStatus: async () => distStatus,
}));
vi.mock('../../acp/mlx-remote-single', async (original) => ({
  ...(await original<typeof import('../../acp/mlx-remote-single')>()),
  mlxRemoteSingleStatus: async () => routeStatus,
  latestMlxRemoteSingleStatus: () => routeStatus,
  subscribeMlxRemoteSingleStatus: () => () => {},
}));

import RecipeChatWizard from './RecipeChatWizard';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const SERVING: MlxEngineStatus = {
  state: 'running',
  modelId: 'mlx-community/Qwen3.5-9B-4bit',
  servedModelId: 'workhorse-qwen3.5-9b-4bit-mlx',
  baseUrl: 'http://127.0.0.1:9600/v1',
  restartRequired: false,
  availableMemoryGb: 30,
  totalMemoryGb: 64,
};

const mount = () => render(<RecipeChatWizard isOpen onClose={() => {}} onSaved={() => {}} />);

describe('RecipeChatWizard × the LeanZero MLX engine', () => {
  let fleetChat: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    fleetMock.online = false;
    fleetMock.models = [];
    mlxStatus = null;
    distStatus = null;
    routeStatus = null;
    fleetChat = vi.fn(async () => ({
      ok: true,
      status: 200,
      url: 'x',
      body: { choices: [{ message: { content: 'What inputs does the agent take?' } }] },
    }));
    electron().fleetChat = fleetChat;
  });
  afterEach(() => {
    delete electron().fleetChat;
  });

  it('MLX-only: the served alias is the model, the chip reads online, and the chat POSTs to the sidecar base URL', async () => {
    mlxStatus = SERVING;
    mount();
    await screen.findByText(/What's the task\?/);
    expect(screen.getByRole('img', { name: 'fleet online' })).toBeInTheDocument();
    expect(screen.queryByText('offline')).toBeNull();
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), {
      target: { value: 'summarise my inbox' },
    });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    await waitFor(() => expect(fleetChat).toHaveBeenCalledTimes(1));
    const [endpoint, body] = fleetChat.mock.calls[0] as [string, { model: string }];
    expect(endpoint).toBe('http://127.0.0.1:9600/v1');
    expect(body.model).toBe('workhorse-qwen3.5-9b-4bit-mlx');
    await screen.findByText('What inputs does the agent take?');
  });

  const sendHi = async () => {
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    await waitFor(() => expect(fleetChat).toHaveBeenCalledTimes(1));
    return fleetChat.mock.calls[0] as [string, { model: string }];
  };

  it('mixed: the model chat is served by is the default; picking an LM Studio model switches the host', async () => {
    fleetMock.online = true;
    fleetMock.models = ['gabee-coder-27b'];
    mlxStatus = SERVING;
    mount();
    await screen.findByText(/What's the task\?/);
    // Q-7: the interview goes to the engine goose serves chat with — not LM Studio's coder model.
    const trigger = screen.getByTitle(/workhorse-qwen3.5-9b-4bit-mlx — click to switch node/);
    fireEvent.pointerDown(trigger, new PointerEvent('pointerdown', { bubbles: true, button: 0 }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'gabee-coder-27b' }));
    const [endpoint, body] = await sendHi();
    expect(endpoint).toBe('http://127.0.0.1:1234');
    expect(body.model).toBe('gabee-coder-27b');
  });

  it('mixed, untouched: the interview goes to the chat engine, not the LM Studio coder', async () => {
    fleetMock.online = true;
    fleetMock.models = ['gabee-coder-27b'];
    mlxStatus = SERVING;
    mount();
    await screen.findByText(/What's the task\?/);
    const [endpoint, body] = await sendHi();
    expect(endpoint).toBe('http://127.0.0.1:9600/v1');
    expect(body.model).toBe('workhorse-qwen3.5-9b-4bit-mlx');
  });

  // Q-7, round live-1: chat was on the split across the Macs, this Mac's single engine stopped —
  // the wizard read "no fleet model is served" while the split answered every chat.
  it('the split serves chat: the interview goes to rank 0 under the id the ranks serve', async () => {
    mlxStatus = { ...SERVING, state: 'stopped', servedModelId: undefined, modelId: undefined };
    distStatus = {
      mode: 'distributed',
      state: 'ready',
      modelId: 'mlx-community/Qwen3.6-27B-8bit',
      servedModelId: 'qwen3.6-27b-split',
      baseUrl: 'http://127.0.0.1:8090/v1',
      contextLimit: 262144,
      admissionOpen: true,
      nodes: [],
    } as unknown as MlxDistributedStatus;
    mount();
    await screen.findByText(/What's the task\?/);
    expect(screen.queryByText('offline')).toBeNull();
    const [endpoint, body] = await sendHi();
    expect(endpoint).toBe('http://127.0.0.1:8090/v1');
    expect(body.model).toBe('qwen3.6-27b-split');
  });

  it('a linked Mac serves chat through the route: the interview goes through the relay', async () => {
    routeStatus = {
      state: 'ready',
      peer: 'node-work',
      peerHostname: 'works-mac-studio',
      baseUrl: 'http://127.0.0.1:8095/relay-cap',
      modelId: 'mlx-community/Qwen3.6-27B-8bit',
      servedModelId: 'work-qwen3.6-27b',
    } as unknown as MlxRemoteSingleStatus;
    mount();
    await screen.findByText(/What's the task\?/);
    const [endpoint, body] = await sendHi();
    expect(endpoint).toBe('http://127.0.0.1:8095/relay-cap');
    expect(body.model).toBe('work-qwen3.6-27b');
  });

  it('a route whose Mac is not ready is not a target (no guess, the wizard is offline)', async () => {
    routeStatus = {
      state: 'reconnecting',
      peer: 'node-work',
      baseUrl: 'http://127.0.0.1:8095/relay-cap',
      servedModelId: 'work-qwen3.6-27b',
    } as unknown as MlxRemoteSingleStatus;
    mount();
    await screen.findByText(/What's the task\?/);
    expect(screen.getByText('offline')).toBeInTheDocument();
  });

  it('nothing served anywhere: offline, and the failure says where to start a model — never LM Studio', async () => {
    mlxStatus = { ...SERVING, state: 'stopped', servedModelId: undefined, modelId: undefined };
    mount();
    await screen.findByText(/What's the task\?/);
    expect(screen.getByText('offline')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    const failure = await screen.findByText(/start one in Providers › LeanZero MLX › Run it/);
    expect(failure.textContent).not.toMatch(/LM Studio/);
    expect(fleetChat).not.toHaveBeenCalled();
  });
});
