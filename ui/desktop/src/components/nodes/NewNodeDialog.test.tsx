import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import type { Mac } from '../leanzero-swarm/macs';
import type { PlacementPlan } from '../../acp/mlx-placement';
import {
  CONFIG,
  LOCAL_FLASH,
  MODEL_27B,
  MODEL_FLASH,
  NODE_CLOUD,
  NODE_FLASH,
  OPENROUTER,
  PLANS,
  SELF_MAC,
  STUDIO_KEY,
  TWO_MACS,
  provider,
} from './nodeGlance.fixtures';
import type { NodesConfig, ResolvedNodeDef } from './model';
import type { NodesServingWayDto } from '@aaif/goose-sdk';
import { TONE_TEXT } from '../lz';

const macsNow = vi.hoisted(() => ({ macs: [] as unknown[] }));
vi.mock('../leanzero-swarm/useMacs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../leanzero-swarm/useMacs')>()),
  WithMacs: ({ children }: { children: ReactNode }) => <>{children}</>,
  useMacs: () => ({
    macs: macsNow.macs,
    factsOf: (key: string) => ({
      models:
        key === STUDIO_KEY
          ? [{ id: MODEL_27B, sizeBytes: 31 * 1024 ** 3, complete: true, missingFiles: 0 }]
          : [
              { id: MODEL_27B, sizeBytes: 31 * 1024 ** 3, complete: true, missingFiles: 0 },
              { id: MODEL_FLASH, sizeBytes: 18 * 1024 ** 3, complete: true, missingFiles: 0 },
            ],
    }),
  }),
}));
const mockPlan = vi.fn();
vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlan(...a),
}));
const mockRead = vi.fn();
const mockWrite = vi.fn();
vi.mock('../../acp/nodes', () => ({
  nodesRead: (...a: unknown[]) => mockRead(...a),
  nodesWrite: (...a: unknown[]) => mockWrite(...a),
}));
const mockProviders = vi.fn();
const mockLive = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: () => mockProviders(),
  acpListProviderLiveModels: (...a: unknown[]) => mockLive(...a),
}));

import { NewNodeDialog, type NewNodeStart } from './NewNodeDialog';

const ENDPOINT = provider('custom_desk_vllm', 'Desk vLLM', { provider_type: 'Custom' });
const NOT_SET_UP = provider('anthropic', 'Anthropic', {
  is_configured: false,
  credentials_saved: false,
  connection_checked: false,
});

/** The 27B split across both Macs, serving this Mac's goose (the critic's 3.0.68 state). */
const SPLIT_27B_SERVING: NodesServingWayDto = {
  kind: 'split',
  macs: ['local', 'link:n-studio'],
  link: 'jaccl',
  modelId: MODEL_27B,
  servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  macNames: ['Mihai Macbook', 'Work’s Mac Studio'],
};

function renderDialog(
  start: NewNodeStart = { kind: 'new' },
  {
    serving = null,
    nodes = [NODE_CLOUD],
  }: { serving?: NodesServingWayDto | null; nodes?: ResolvedNodeDef[] } = {}
) {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  const onOpenCloudProviders = vi.fn();
  const onOpenModels = vi.fn();
  render(
    <IntlTestWrapper>
      <NewNodeDialog
        open
        start={start}
        nodes={nodes}
        serving={serving}
        onClose={onClose}
        onSaved={onSaved}
        onOpenCloudProviders={onOpenCloudProviders}
        onOpenModels={onOpenModels}
      />
    </IntlTestWrapper>
  );
  return { onSaved, onClose, onOpenCloudProviders, onOpenModels };
}

const next = () => userEvent.click(screen.getByTestId('new-node-next'));
const written = () => mockWrite.mock.calls[mockWrite.mock.calls.length - 1][0] as NodesConfig;
const lastDef = () => {
  const defs = written().defs!;
  return defs[defs.length - 1];
};

beforeEach(() => {
  macsNow.macs = [...TWO_MACS] as Mac[];
  mockPlan.mockImplementation(async (goal: string, modelId?: string) => ({
    plans: PLANS.filter((p) => !modelId || p.modelId === modelId).map((p) => ({ ...p, goal })),
    nodes: [],
    storeErrors: [],
    probeMs: 1,
  }));
  mockRead.mockResolvedValue({
    config: { ...CONFIG, defs: [NODE_CLOUD.def] },
    nodes: [NODE_CLOUD],
    stored: true,
    lmStudioHidden: 0,
  });
  mockWrite.mockImplementation(async (config: NodesConfig) => ({
    written: true,
    refusals: [],
    read: { config },
  }));
  mockProviders.mockResolvedValue([OPENROUTER, ENDPOINT, NOT_SET_UP]);
  mockLive.mockResolvedValue(['anthropic/claude-sonnet-4.5', 'openai/gpt-5']);
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('New node · on your Macs', () => {
  it('Q-259: a disabled Next or Create says why beside it, and the reason leaves once it is met', async () => {
    renderDialog();
    // Kind has a default (On your Macs): Next is live, nothing to say.
    expect(screen.getByTestId('new-node-next')).toBeEnabled();
    expect(screen.queryByTestId('new-node-blocked')).toBeNull();
    await next();
    const rows = await screen.findAllByTestId('new-node-model-row');
    expect(screen.getByTestId('new-node-next')).toBeDisabled();
    expect(screen.getByTestId('new-node-blocked')).toHaveTextContent('Pick a model to go on');
    expect(screen.getByTestId('new-node-next')).toHaveAttribute(
      'aria-describedby',
      screen.getByTestId('new-node-blocked').id
    );
    await userEvent.click(rows[0]);
    expect(screen.queryByTestId('new-node-blocked')).toBeNull();
    await next();
    await screen.findByTestId('placement-pick-split');
    expect(screen.getByTestId('new-node-blocked')).toHaveTextContent(
      'Pick a way to run it to go on'
    );
    await userEvent.click(screen.getByTestId('placement-pick-split'));
    expect(screen.queryByTestId('new-node-blocked')).toBeNull();
    await next();
    const name = screen.getByTestId('new-node-name-input');
    await userEvent.clear(name);
    expect(screen.getByTestId('new-node-create')).toBeDisabled();
    expect(screen.getByTestId('new-node-blocked')).toHaveTextContent('Give it a name to save it');
    // Nothing was tried: no "Not saved".
    expect(screen.queryByTestId('new-node-refusals')).toBeNull();
    await userEvent.type(name, 'Mine');
    expect(screen.queryByTestId('new-node-blocked')).toBeNull();
    expect(screen.getByTestId('new-node-create')).toBeEnabled();
  });

  it('two Macs: model with its badge, the split way, the default name, one def written', async () => {
    const { onSaved, onClose } = renderDialog();
    expect(screen.getByTestId('new-node-kind-mlx')).toHaveAttribute('aria-checked', 'true');
    await next();
    const rows = await screen.findAllByTestId('new-node-model-row');
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining(MODEL_27B),
      expect.stringContaining(MODEL_FLASH),
    ]);
    expect(rows[0]).toHaveTextContent('on Mihai Macbook and Work’s Mac Studio');
    await waitFor(() => expect(rows[0]).toHaveTextContent('Needs both Macs'));
    await userEvent.click(rows[0]);
    await next();

    // The ways are Run it's: the split and the Studio can be picked; this Mac (too big) cannot.
    await screen.findByTestId('placement-pick-split');
    expect(screen.getByTestId('placement-pick-peer')).toBeInTheDocument();
    expect(screen.queryByTestId('placement-pick-local')).toBeNull();
    expect(screen.getByTestId('placement-way-local')).toHaveTextContent(
      'Does not fit: short 1.6 GB'
    );
    // Q-306: the need first, then what is free — never "38.6 of 61.8 GB", which read backwards
    // when the need was the larger number.
    expect(screen.getByTestId('placement-way-split')).toHaveTextContent(
      'Mihai Macbook: needs 38.6 GB, 61.8 GB free · Work’s Mac Studio: needs 38.6 GB, 66.2 GB free'
    );
    await userEvent.click(screen.getByTestId('placement-pick-split'));
    expect(screen.getByTestId('placement-pick-split')).toHaveAttribute('aria-checked', 'true');
    await next();

    const name = screen.getByTestId('new-node-name-input') as HTMLInputElement;
    expect(name.value).toBe('Qwen3.8-27B-Atlassian-Q8-mlx · both Macs');
    await userEvent.click(screen.getByTestId('new-node-keep-loaded'));
    await userEvent.click(screen.getByTestId('new-node-create'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const def = lastDef();
    expect(def).toEqual({
      id: 'qwen3-8-27b-atlassian-q8-mlx-both-macs',
      name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
      kind: 'mlx',
      model: MODEL_27B,
      placement: { kind: 'pipeline', macs: ['local', 'link:n-studio'], link: 'jaccl' },
      goal: 'chat',
      keepLoaded: true,
      origin: 'user',
    });
    // Every other def and strategy goes back as goosed holds them.
    expect(written().defs!.map((d) => d.id)).toEqual([NODE_CLOUD.def.id, def.id]);
    expect(written().strategies).toEqual(CONFIG.strategies);
    expect(onSaved).toHaveBeenCalledWith(def, false);
    expect(onClose).toHaveBeenCalled();
  });

  it('one Mac: only the ways on this Mac are there to pick', async () => {
    macsNow.macs = [SELF_MAC];
    mockPlan.mockImplementation(async (goal: PlacementPlan['goal']) => ({
      plans: [
        {
          modelId: MODEL_FLASH,
          goal,
          candidates: [LOCAL_FLASH],
          best: LOCAL_FLASH.id,
          bestAvailable: LOCAL_FLASH.id,
          notes: [],
        } satisfies PlacementPlan,
      ],
      nodes: [],
      storeErrors: [],
      probeMs: 1,
    }));
    renderDialog({ kind: 'pin', model: MODEL_FLASH });
    await screen.findByTestId('placement-pick-local');
    expect(screen.queryByTestId('placement-way-split')).toBeNull();
    expect(screen.queryByTestId('placement-way-peer')).toBeNull();
    await userEvent.click(screen.getByTestId('placement-pick-local'));
    await next();
    expect((screen.getByTestId('new-node-name-input') as HTMLInputElement).value).toBe(
      'Qwen3.8-Flash-Next-4bit · this Mac'
    );
  });

  it('a plan that measured nothing shows its notes; the goal plans again', async () => {
    mockPlan.mockImplementation(async (goal: string) => ({
      plans: [
        {
          modelId: MODEL_FLASH,
          goal,
          candidates: [],
          notes: ['the Studio’s memory could not be read: no answer over LeanZero Link'],
        },
      ],
      nodes: [],
      storeErrors: [],
      probeMs: 1,
    }));
    renderDialog({ kind: 'pin', model: MODEL_FLASH });
    expect(await screen.findByTestId('new-node-plan-note')).toHaveTextContent(
      'the Studio’s memory could not be read: no answer over LeanZero Link'
    );
    expect(screen.getByTestId('new-node-way')).toHaveTextContent(
      'goose found no way to run this model on your Macs.'
    );
    await userEvent.click(screen.getByRole('radio', { name: 'Long documents' }));
    await waitFor(() => expect(mockPlan).toHaveBeenLastCalledWith('longDocuments', MODEL_FLASH));
  });

  it('a plan read that failed is its words, with Plan again — never a guessed row', async () => {
    mockPlan.mockRejectedValueOnce(
      Object.assign(new Error('Invalid params'), { data: 'Link is down' })
    );
    renderDialog({ kind: 'pin', model: MODEL_27B });
    const failed = await screen.findByTestId('new-node-plan-failed');
    expect(failed).toHaveTextContent('Could not plan: Link is down');
    expect(screen.queryByTestId('placement-ways')).toBeNull();
    await userEvent.click(within(failed).getByRole('button', { name: 'Plan again' }));
    await screen.findByTestId('placement-pick-split');
  });

  it('no models on any Mac: says so and leads to Models', async () => {
    macsNow.macs = [];
    const { onOpenModels } = renderDialog();
    await next();
    expect(screen.getByTestId('new-node-no-models')).toHaveTextContent(
      'No models on your Macs yet.'
    );
    await userEvent.click(screen.getByRole('button', { name: 'Get one in Models' }));
    expect(onOpenModels).toHaveBeenCalled();
  });

  it('a refusal from nodes/write is shown verbatim and nothing closes', async () => {
    mockWrite.mockResolvedValue({
      written: false,
      refusals: [{ code: 'duplicateName', message: 'two nodes are named "Flash"' }],
      read: {},
    });
    const { onClose, onSaved } = renderDialog({ kind: 'pin', model: MODEL_27B });
    await userEvent.click(await screen.findByTestId('placement-pick-split'));
    await next();
    await userEvent.click(screen.getByTestId('new-node-create-start'));
    expect(await screen.findByTestId('new-node-refusals')).toHaveTextContent(
      'two nodes are named "Flash"'
    );
    expect(onClose).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it('Q-306: a way goose refuses says so in the error colour; a merely slower way stays plain', async () => {
    renderDialog({ kind: 'pin', model: MODEL_27B });
    await screen.findByTestId('placement-pick-split');
    const refused = within(screen.getByTestId('placement-way-local')).getByTestId(
      'placement-way-why'
    );
    expect(refused).toHaveTextContent('Does not fit: short 1.6 GB');
    expect(refused).toHaveAttribute('data-refused', 'true');
    for (const token of TONE_TEXT.err.split(' ')) expect(refused.className).toContain(token);
    for (const why of screen.getAllByTestId('placement-way-why')) {
      if (why === refused) continue;
      expect(why).not.toHaveAttribute('data-refused');
    }
  });

  it('Q-299: Create and start says beside it what starting stops, and Create node leads', async () => {
    const { onSaved } = renderDialog(
      { kind: 'pin', model: MODEL_27B },
      { serving: SPLIT_27B_SERVING }
    );
    await userEvent.click(await screen.findByTestId('placement-pick-peer'));
    await next();
    const stops = screen.getByTestId('new-node-start-stops');
    expect(stops).toHaveTextContent(
      'Starting it stops Qwen3.8-27B-Atlassian-Q8-mlx on Mihai Macbook and Work’s Mac Studio.'
    );
    const start = screen.getByTestId('new-node-create-start');
    expect(start).toHaveAttribute('aria-describedby', stops.id);
    // The safe door is the primary one while starting would stop what serves.
    expect(screen.getByTestId('new-node-create').className).not.toBe(start.className);
    await userEvent.click(start);
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(expect.anything(), true));
  });

  it('Q-299: nothing is claimed when the node IS the way serving now, or when nothing serves', async () => {
    renderDialog({ kind: 'pin', model: MODEL_27B }, { serving: SPLIT_27B_SERVING });
    await userEvent.click(await screen.findByTestId('placement-pick-split'));
    await next();
    expect(screen.queryByTestId('new-node-start-stops')).toBeNull();
    cleanup();
    renderDialog({ kind: 'pin', model: MODEL_27B });
    await userEvent.click(await screen.findByTestId('placement-pick-split'));
    await next();
    expect(screen.queryByTestId('new-node-start-stops')).toBeNull();
  });

  it('Q-309: a name another node carries disables Create with the reason, before anything is sent', async () => {
    renderDialog({ kind: 'pin', model: MODEL_27B }, { nodes: [NODE_CLOUD, NODE_FLASH] });
    await userEvent.click(await screen.findByTestId('placement-pick-split'));
    await next();
    const name = screen.getByTestId('new-node-name-input');
    await userEvent.clear(name);
    await userEvent.type(name, NODE_FLASH.def.name);
    expect(screen.getByTestId('new-node-blocked')).toHaveTextContent(
      `Another node is already named “${NODE_FLASH.def.name}” — give this one another name`
    );
    expect(screen.getByTestId('new-node-create')).toBeDisabled();
    expect(screen.getByTestId('new-node-create-start')).toBeDisabled();
    await userEvent.type(name, ' 2');
    expect(screen.queryByTestId('new-node-blocked')).toBeNull();
    expect(screen.getByTestId('new-node-create')).toBeEnabled();
    expect(mockWrite).not.toHaveBeenCalled();
  });

  it('Create and start hands the def back to start it', async () => {
    const { onSaved } = renderDialog({ kind: 'pin', model: MODEL_27B });
    await userEvent.click(await screen.findByTestId('placement-pick-split'));
    await next();
    await userEvent.click(screen.getByTestId('new-node-create-start'));
    await waitFor(() =>
      expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ kind: 'mlx' }), true)
    );
  });
});

describe('New node · a cloud model or an endpoint', () => {
  it('one provider configured: provider, its own model list, a default name, one def', async () => {
    const { onSaved } = renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('1 set up')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-cloud'));
    await next();
    const providers = screen.getAllByTestId('new-node-provider-row');
    // Configured cloud providers only: not the endpoint, not the one never set up.
    expect(providers.map((p) => p.textContent)).toEqual(['OpenRouter']);
    await userEvent.click(providers[0]);
    await next();
    expect(mockLive).toHaveBeenCalledWith('openrouter');
    await userEvent.click(await screen.findByRole('combobox', { name: 'Model' }));
    await userEvent.click(await screen.findByText('openai/gpt-5'));
    await next();
    // Q-439: named by the model id as the provider lists it, not its short name.
    expect((screen.getByTestId('new-node-name-input') as HTMLInputElement).value).toBe(
      'openai/gpt-5 · OpenRouter'
    );
    expect(screen.queryByTestId('new-node-create-start')).toBeNull();
    await userEvent.click(screen.getByTestId('new-node-create'));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(lastDef()).toEqual({
      id: 'openai-gpt-5-openrouter',
      name: 'openai/gpt-5 · OpenRouter',
      kind: 'cloud',
      model: 'openai/gpt-5',
      provider: 'openrouter',
      origin: 'user',
    });
  });

  it('Q-429: the cloud tile counts and lists cloud providers only — never a local engine', async () => {
    // The demo's 3.0.73 list: oMLX and Goose Swarm read configured (they need no key) while the
    // Cloud Providers tab said 0 of 14 — the tile said "2 set up".
    const OMLX = provider('omlx', 'oMLX');
    const SWARM = provider('swarm', 'Goose Swarm');
    const LMSTUDIO = provider('lmstudio', 'LM Studio');
    mockProviders.mockResolvedValue([OMLX, SWARM, LMSTUDIO, NOT_SET_UP, ENDPOINT]);
    renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('None set up')
    );
    expect(screen.getByTestId('new-node-kind-endpoint')).toHaveTextContent('1 added');
    cleanup();
    mockProviders.mockResolvedValue([OMLX, SWARM, LMSTUDIO, OPENROUTER, ENDPOINT]);
    renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('1 set up')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-cloud'));
    await next();
    expect(screen.getAllByTestId('new-node-provider-row').map((p) => p.textContent)).toEqual([
      'OpenRouter',
    ]);
  });

  it('Q-437: the provider’s default model leads the model list, marked, and is the first pick', async () => {
    mockProviders.mockResolvedValue([
      { ...OPENROUTER, default_model: 'deepseek/deepseek-v4.1-flash' },
    ]);
    mockLive.mockResolvedValue([
      'aion-labs/aion-2.0',
      'aion-labs/aion-3.0',
      'deepseek/deepseek-v4.1-flash',
      'openai/gpt-5',
    ]);
    renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('1 set up')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-cloud'));
    await next();
    await userEvent.click(screen.getAllByTestId('new-node-provider-row')[0]);
    await next();
    const box = await screen.findByRole('combobox', { name: 'Model' });
    expect(box).toHaveValue('deepseek/deepseek-v4.1-flash');
    expect(screen.getByTestId('new-node-next')).toBeEnabled();
    await userEvent.clear(box);
    const options = await screen.findAllByRole('option');
    expect(options.map((o) => o.textContent)).toEqual([
      'deepseek/deepseek-v4.1-flashDefault',
      'aion-labs/aion-2.0',
      'aion-labs/aion-3.0',
      'openai/gpt-5',
    ]);
  });

  it('none configured: the tile says so and leads to Cloud Providers', async () => {
    mockProviders.mockResolvedValue([NOT_SET_UP]);
    const { onOpenCloudProviders } = renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('None set up')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-cloud'));
    expect(screen.getByTestId('new-node-next')).toBeDisabled();
    await userEvent.click(within(screen.getByTestId('new-node-none-set-up')).getByRole('button'));
    expect(onOpenCloudProviders).toHaveBeenCalled();
  });

  it('a provider with no model listing offers goose’s catalog for it, and says so', async () => {
    mockLive.mockResolvedValue([]);
    renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-cloud')).toHaveTextContent('1 set up')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-cloud'));
    await next();
    await userEvent.click(screen.getAllByTestId('new-node-provider-row')[0]);
    await next();
    expect(await screen.findByTestId('new-node-provider-model')).toHaveTextContent(
      'OpenRouter has no model listing; these are the models goose knows for it.'
    );
  });

  it('an endpoint node names its endpoint', async () => {
    renderDialog();
    await waitFor(() =>
      expect(screen.getByTestId('new-node-kind-endpoint')).toHaveTextContent('1 added')
    );
    await userEvent.click(screen.getByTestId('new-node-kind-endpoint'));
    await next();
    expect(screen.getAllByTestId('new-node-provider-row').map((p) => p.textContent)).toEqual([
      'Desk vLLM',
    ]);
  });

  it('a provider list that could not be read says why', async () => {
    mockProviders.mockRejectedValue(new Error('goosed is not answering'));
    renderDialog();
    expect(
      await screen.findByText('Your providers could not be read: goosed is not answering')
    ).toBeInTheDocument();
  });
});

describe('New node · edit', () => {
  it('opens on Name, keeps the id, and saves over the same def', async () => {
    const { onSaved } = renderDialog({ kind: 'edit', node: NODE_FLASH });
    const name = screen.getByTestId('new-node-name-input') as HTMLInputElement;
    expect(name.value).toBe('Flash · this Mac');
    await userEvent.clear(name);
    await userEvent.type(name, 'Flash');
    await userEvent.click(screen.getByTestId('new-node-create'));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(onSaved.mock.calls[0][0]).toMatchObject({
      id: NODE_FLASH.def.id,
      name: 'Flash',
      model: MODEL_FLASH,
    });
  });
});

describe('New node · Studio-clean', () => {
  it('no native select, no rail, no tint on any step', async () => {
    renderDialog({ kind: 'pin', model: MODEL_27B });
    await screen.findByTestId('placement-pick-split');
    assertStudioClean(screen.getByTestId('new-node-way').closest('[role="dialog"]') as HTMLElement);
  });
});
