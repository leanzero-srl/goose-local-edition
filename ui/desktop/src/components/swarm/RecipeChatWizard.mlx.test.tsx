import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';

// jsdom has no Element.scrollTo; the chat wizard follows its own scroll after every message.
if (!window.Element.prototype.scrollTo) {
  window.Element.prototype.scrollTo = () => {};
}

/**
 * The recipe wizard talks to whichever fleet engine SERVES a model. Measured 2026-09-05 on an MLX-only
 * machine: LM Studio discovery is off by default and found nothing, so the wizard read "no fleet model is
 * loaded — start LM Studio" while the LeanZero MLX sidecar served `workhorse-qwen3.5-9b-4bit-mlx`.
 */
const fleetMock = { online: false, models: [] as string[], loading: false, endpoint: 'http://127.0.0.1:1234' };
vi.mock('./useFleet', () => ({ useFleet: () => fleetMock }));
vi.mock('../../hooks/useLmStudioFleetVisible', () => ({ useLmStudioFleetVisible: () => false }));
let mlxStatus: MlxEngineStatus | null = null;
vi.mock('../leanzero-swarm/useMlxEngineStatus', () => ({
  useMlxEngineStatusPoll: () => ({ status: mlxStatus, error: null }),
}));
vi.mock('../../recipe/recipe_management', () => ({ saveRecipe: async () => undefined }));

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
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), { target: { value: 'summarise my inbox' } });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    await waitFor(() => expect(fleetChat).toHaveBeenCalledTimes(1));
    const [endpoint, body] = fleetChat.mock.calls[0] as [string, { model: string }];
    expect(endpoint).toBe('http://127.0.0.1:9600/v1');
    expect(body.model).toBe('workhorse-qwen3.5-9b-4bit-mlx');
    await screen.findByText('What inputs does the agent take?');
  });

  it('mixed: LM Studio models keep the LM Studio endpoint; picking the MLX alias switches the host', async () => {
    fleetMock.online = true;
    fleetMock.models = ['gabee-coder-27b'];
    mlxStatus = SERVING;
    mount();
    await screen.findByText(/What's the task\?/);
    // Auto-pick prefers a coder model — the LM Studio one here — at LM Studio's endpoint.
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    await waitFor(() => expect(fleetChat).toHaveBeenCalledTimes(1));
    expect((fleetChat.mock.calls[0] as [string, { model: string }])[0]).toBe('http://127.0.0.1:1234');
    expect((fleetChat.mock.calls[0] as [string, { model: string }])[1].model).toBe('gabee-coder-27b');
  });

  it('nothing served anywhere: offline, and the failure names BOTH engines', async () => {
    mlxStatus = { ...SERVING, state: 'stopped', servedModelId: undefined, modelId: undefined };
    mount();
    await screen.findByText(/What's the task\?/);
    expect(screen.getByText('offline')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText(/Answer the fleet/), { target: { value: 'hi' } });
    fireEvent.click(screen.getByRole('button', { name: /^Send$/ }));
    await screen.findByText(/load one in LM Studio or mount one in the LeanZero MLX engine/);
    expect(fleetChat).not.toHaveBeenCalled();
  });
});
