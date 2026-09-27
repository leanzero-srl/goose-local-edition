import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import type { ReactNode } from 'react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import type { GlanceNodesState } from '../engineGlance/glanceStore';
import type { BuildEligibility, NodesRead } from '../../acp/nodes';
import {
  CONFIG,
  LOADS_FLASH,
  MODEL_27B,
  MODEL_FLASH,
  NODE_CLOUD,
  NODE_FLASH,
  NODE_SPLIT,
  OPENROUTER,
  PLANS,
  TWO_MACS,
} from './nodeGlance.fixtures';
import type { NodeStrategy, NodesConfig, ResolvedNodeDef } from './model';
import type { Read } from './nodeGlance';

const store = vi.hoisted(() => ({ state: { kind: 'unread' } as unknown }));
const mockRefresh = vi.fn();
vi.mock('../engineGlance/glanceStore', () => ({
  useGlanceNodes: () => store.state,
  useEngineGlance: () => null,
  refreshGlanceNodes: () => mockRefresh(),
}));
vi.mock('../leanzero-swarm/useMacs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../leanzero-swarm/useMacs')>()),
  WithMacs: ({ children }: { children: ReactNode }) => <>{children}</>,
  useMacs: () => ({
    macs: TWO_MACS,
    factsOf: () => ({
      models: [
        { id: MODEL_27B, sizeBytes: 1, complete: true, missingFiles: 0 },
        { id: MODEL_FLASH, sizeBytes: 1, complete: true, missingFiles: 0 },
      ],
    }),
  }),
}));
const mockPlan = vi.fn();
vi.mock('../../acp/mlx-placement', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-placement')>()),
  mlxPlacementPlan: (...a: unknown[]) => mockPlan(...a),
}));
const mockLoads = vi.fn();
const mockWrite = vi.fn();
const mockRemove = vi.fn();
vi.mock('../../acp/nodes', () => ({
  nodesLoadHistory: (...a: unknown[]) => mockLoads(...a),
  nodesWrite: (...a: unknown[]) => mockWrite(...a),
  nodesRemoveStrategy: (...a: unknown[]) => mockRemove(...a),
}));
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: async () => [OPENROUTER],
}));

import { StrategiesTab } from './StrategiesTab';

const NODES = [NODE_SPLIT, NODE_FLASH, NODE_CLOUD];

/** "Quick": Chat on Flash · this Mac, Build on the 27B split — the strategy that swaps. */
const QUICK: NodeStrategy = {
  id: 'quick',
  name: 'Quick',
  roles: {
    chat: { chain: [{ node: NODE_FLASH.def.id, weight: 1 }], when: 'failover' },
    build: { chain: [{ node: NODE_SPLIT.def.id, weight: 1 }], when: 'failover' },
  },
};
/** "Local": Flash only — the strategy swarm builds can use. */
const LOCAL: NodeStrategy = {
  id: 'local',
  name: 'Local',
  roles: { chat: { chain: [{ node: NODE_FLASH.def.id, weight: 1 }], when: 'failover' } },
};

function readOf(config: NodesConfig, nodes: ResolvedNodeDef[] = NODES): NodesRead {
  return {
    config: { ...config, defs: nodes.map((n) => n.def) },
    nodes,
    stored: true,
    lmStudioHidden: 0,
  };
}

function readState(read: NodesRead): GlanceNodesState {
  return {
    kind: 'read',
    read,
    residency: {
      nodes: read.nodes.map((n) => ({
        node: n.def.id,
        residency:
          n.def.kind === 'mlx' ? { kind: 'notRunning', otherWay: null } : { kind: 'alwaysReady' },
      })),
      serving: null,
      loaderInstalled: false,
    },
    servedNode: null,
  };
}

const SPLIT_REFUSED: BuildEligibility = {
  eligible: false,
  reasons: [
    {
      reason: { kind: 'split', node: NODE_SPLIT.def.id },
      message: 'goosed words for the split',
    },
  ],
  notes: [],
};
const LOCAL_OK: BuildEligibility = {
  eligible: true,
  reasons: [],
  notes: ['Testing, Frontend and Backend take effect when the engine learns roles'],
};

const ELIGIBILITY: Record<string, Read<BuildEligibility>> = {
  everyday: { kind: 'read', value: SPLIT_REFUSED },
  quick: { kind: 'read', value: SPLIT_REFUSED },
  local: { kind: 'read', value: LOCAL_OK },
};

function Where() {
  const l = useLocation();
  return <span data-testid="where">{l.pathname + l.search}</span>;
}

function renderTab(path = '/nodes?tab=strategies') {
  return render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[path]}>
        <StrategiesTab eligibility={ELIGIBILITY} />
        <Where />
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

const card = (id: string) =>
  screen.getAllByTestId('strategy-card').find((c) => c.getAttribute('data-strategy') === id)!;
const roleRow = (role: string) =>
  within(screen.getByTestId('strategy-editor'))
    .getAllByTestId('strategy-role')
    .find((r) => r.getAttribute('data-role') === role)!;
const where = () => screen.getByTestId('where').textContent;

beforeEach(() => {
  store.state = readState(readOf({ ...CONFIG, strategies: [...CONFIG.strategies!, QUICK, LOCAL] }));
  mockPlan.mockResolvedValue({ plans: PLANS, nodes: [], storeErrors: [], probeMs: 1 });
  mockLoads.mockImplementation(async (id: string) => ({
    groups: id === NODE_FLASH.def.id ? LOADS_FLASH : [],
    path: '/x',
  }));
  mockWrite.mockImplementation(async (config: NodesConfig) => ({
    written: true,
    refusals: [],
    read: readOf(config),
  }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('the Strategies tab — cards', () => {
  it('Everyday: its chains, the roles that follow, one way with no swaps, and why builds refuse it', () => {
    const { container } = renderTab();
    const everyday = card('everyday');
    const roles = within(everyday).getByTestId('strategy-roles');
    expect(roles).toHaveTextContent('Chat27B Atlassian · both Macs → Claude Sonnet · OpenRouter');
    expect(roles).toHaveTextContent(
      'Build27B Atlassian · both Macs → Claude Sonnet · OpenRouter (share 2:1)'
    );
    expect(
      within(roles)
        .getAllByTestId('strategy-same-as')
        .map((s) => `${s.getAttribute('data-role')}: ${s.textContent}`)
    ).toEqual([
      'planning: PlanningSame as Chat',
      'testing: TestingSame as Build',
      'frontend: FrontendSame as Build',
      'backend: BackendSame as Build',
    ]);
    expect(within(everyday).getByTestId('strategy-fit')).toHaveTextContent(
      'On your Macs: one way (27B Atlassian · both Macs), no swaps'
    );
    expect(within(everyday).queryByTestId('strategy-swap')).toBeNull();
    expect(within(everyday).queryByTestId('strategy-delegate-warning')).toBeNull();
    expect(within(everyday).getByTestId('strategy-badge-chats')).toHaveTextContent(
      'New chats start here'
    );
    // The split reason in the person's words (the design's string), not a guessed eligibility.
    const builds = within(everyday).getByTestId('strategy-builds');
    expect(builds).toHaveAttribute('data-builds', 'refused');
    expect(builds).toHaveTextContent(
      'Swarm builds can’t use this strategy:27B Atlassian · both Macs is a split; swarm builds reach LeanZero MLX only through this Mac’s single engine'
    );
    // "Use for swarm builds" is not offered where builds cannot use it; the badge's own strategy
    // does not offer "Use for new chats" again.
    expect(within(everyday).queryByTestId('strategy-use-builds')).toBeNull();
    expect(within(everyday).queryByTestId('strategy-use-chats')).toBeNull();
    assertStudioClean(container);
  });

  it('Quick swaps: both ways with their measured (or unmeasured) loads, and the delegate warning', async () => {
    renderTab();
    const quick = card('quick');
    expect(within(quick).getByTestId('strategy-fit')).toHaveTextContent(
      'On your Macs: 2 ways, one at a time: each switch stops one and loads the other'
    );
    await waitFor(() =>
      expect(within(quick).getByTestId('strategy-swap')).toHaveTextContent(
        'Flash · this Mac (about 48s, 3 loads measured) ⇄ 27B Atlassian · both Macs (load not measured yet)'
      )
    );
    expect(within(quick).getByTestId('strategy-delegate-warning')).toHaveTextContent(
      'Each delegate call swaps twice: to 27B Atlassian · both Macs and back to Flash · this Mac'
    );
  });

  it('an eligible strategy offers Use for swarm builds, which writes forBuilds through nodes/write', async () => {
    renderTab();
    const local = card('local');
    expect(within(local).getByTestId('strategy-builds')).toHaveAttribute('data-builds', 'eligible');
    expect(within(local).getByTestId('strategy-builds')).toHaveTextContent(
      'LM Studio models loaded on your fleet also join this build'
    );
    await userEvent.click(within(local).getByTestId('strategy-use-builds'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    expect(written.forBuilds).toEqual({ kind: 'strategy', id: 'local' });
    expect(written.forNewChats).toEqual(CONFIG.forNewChats);
    expect(written.strategies?.map((s) => s.id)).toEqual(['everyday', 'quick', 'local']);
    expect(await within(local).findByTestId('strategy-notice')).toHaveTextContent('Saved.');
  });

  it('Use for new chats: a refusal is shown on the card verbatim', async () => {
    mockWrite.mockResolvedValueOnce({
      written: false,
      refusals: [{ code: 'unknownStrategy', message: 'there is no strategy quick' }],
      read: readOf(CONFIG),
    });
    renderTab();
    await userEvent.click(within(card('quick')).getByTestId('strategy-use-chats'));
    expect(await within(card('quick')).findByTestId('strategy-notice')).toHaveTextContent(
      'there is no strategy quick'
    );
  });

  it('no strategies: an empty state that makes one', async () => {
    store.state = readState(readOf({ ...CONFIG, strategies: [], forNewChats: { kind: 'auto' } }));
    renderTab();
    const empty = screen.getByTestId('strategies-empty');
    expect(empty).toHaveTextContent('No strategies yet');
    await userEvent.click(within(empty).getByRole('button', { name: 'New strategy' }));
    expect(screen.getByTestId('strategy-editor')).toBeInTheDocument();
  });

  it('a strategy= link to an id no strategy carries is said, and Close drops it from the URL', async () => {
    renderTab('/nodes?tab=strategies&strategy=gone');
    expect(screen.getByTestId('strategies-missing-link')).toHaveTextContent(
      'There is no strategy “gone”. It may have been removed.'
    );
    expect(screen.queryByTestId('strategy-editor')).toBeNull();
    await userEvent.click(
      within(screen.getByTestId('strategies-missing-link')).getByRole('button', { name: 'Close' })
    );
    expect(where()).toBe('/nodes?tab=strategies');
  });

  it('Remove: a refusal offers its way through and the second try carries it', async () => {
    mockRemove
      .mockResolvedValueOnce({
        written: false,
        refusals: [{ code: 'strategyIsForNewChats', message: 'new chats start on "Everyday"' }],
        read: readOf(CONFIG),
      })
      .mockResolvedValueOnce({ written: true, refusals: [], read: readOf(CONFIG) });
    renderTab();
    await userEvent.click(within(card('everyday')).getByTestId('strategy-more'));
    await userEvent.click(await screen.findByTestId('strategy-remove'));
    await userEvent.click(await screen.findByTestId('strategy-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith('everyday', {
      andNewChatsAuto: false,
      andBuildsPool: false,
    });
    const refusals = await screen.findByTestId('strategy-remove-refusals');
    expect(refusals).toHaveTextContent('new chats start on "Everyday"');
    expect(within(refusals).queryByTestId('strategy-remove-and-pool')).toBeNull();
    await userEvent.click(within(refusals).getByTestId('strategy-remove-and-auto'));
    await userEvent.click(screen.getByTestId('strategy-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith('everyday', {
      andNewChatsAuto: true,
      andBuildsPool: false,
    });
    await waitFor(() => expect(screen.queryByTestId('strategy-remove-dialog')).toBeNull());
    expect(mockRefresh).toHaveBeenCalled();
  });
});

describe('the strategy editor', () => {
  it('strategy= opens that editor: each role in words, the rule read back, inheritance shown', async () => {
    renderTab('/nodes?tab=strategies&strategy=everyday');
    const editor = screen.getByTestId('strategy-editor');
    expect(editor).toHaveTextContent('Edit Everyday');
    const chat = roleRow('chat');
    expect(chat).toHaveAttribute('data-set', 'true');
    expect(chat).toHaveTextContent('Your turns in a chat: answers, edits, tool calls');
    expect(within(chat).queryByText('Used by swarm builds.')).toBeNull();
    expect(within(chat).getByTestId('strategy-sentence')).toHaveTextContent(
      'Chat runs on 27B Atlassian · both Macs. If it can’t run, on Claude Sonnet · OpenRouter. If 27B Atlassian · both Macs isn’t loaded, it loads and your turn waits; its first load is not measured yet.'
    );
    expect(within(roleRow('build')).getByTestId('strategy-sentence')).toHaveTextContent(
      'Build is shared: 27B Atlassian · both Macs 2 parts, Claude Sonnet · OpenRouter 1 part.'
    );
    const testing = roleRow('testing');
    expect(testing).toHaveAttribute('data-set', 'false');
    expect(testing).toHaveTextContent('Same as Build');
    expect(testing).toHaveTextContent('Used by swarm builds.');
    // Each picker wears its node's state chip — the cards' own derivation.
    const picker = within(chat).getAllByRole('combobox')[0];
    expect(within(picker).getByTestId('node-state')).toHaveAttribute('data-state', 'ready');
    // The stored strategy's build answer is shown until something changes.
    const panel = screen.getByTestId('strategy-fit-panel');
    expect(within(panel).getByTestId('strategy-fit-one')).toHaveTextContent(
      'One way at a time serves your chats: 27B Atlassian · both Macs'
    );
    expect(within(panel).getByTestId('strategy-builds')).toHaveAttribute('data-builds', 'refused');
    assertStudioClean(editor);
  });

  it('a change makes the build answer wait for the save; Cancel drops strategy= in place', async () => {
    renderTab('/nodes?tab=strategies&strategy=everyday');
    await userEvent.type(screen.getByTestId('strategy-name'), ' days');
    const panel = screen.getByTestId('strategy-fit-panel');
    expect(within(panel).getByTestId('strategy-builds-after-save')).toHaveTextContent(
      'Whether swarm builds can use it is checked when you save.'
    );
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByTestId('strategy-editor')).toBeNull();
    expect(where()).toBe('/nodes?tab=strategies');
    expect(mockWrite).not.toHaveBeenCalled();
  });

  it('a new strategy: Chat is what new chats start on, Build reads Same as Chat, Save appends it', async () => {
    renderTab();
    await userEvent.click(screen.getByTestId('strategies-new'));
    expect(screen.getByTestId('strategy-editor')).toHaveTextContent('New strategy');
    // New chats start on Everyday, so the new Chat row is Everyday's Chat chain.
    const chat = roleRow('chat');
    expect(
      within(chat)
        .getAllByTestId('strategy-chain-entry')
        .map((e) => within(e).getByRole('combobox').textContent)
    ).toEqual(['27B Atlassian · both MacsNot loaded', 'Claude Sonnet · OpenRouterReady']);
    expect(roleRow('build')).toHaveAttribute('data-set', 'false');
    expect(roleRow('build')).toHaveTextContent('Same as Chat');
    // A new strategy never swaps by default.
    expect(screen.queryByTestId('strategy-fit-swap')).toBeNull();
    expect(screen.queryByTestId('strategy-fit-delegate')).toBeNull();
    await userEvent.click(screen.getByTestId('strategy-save'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    const added = written.strategies?.[3];
    expect(added?.id).toBe('new-strategy');
    expect(added?.name).toBe('New strategy');
    expect(added?.roles?.build ?? null).toBeNull();
    expect(added?.roles?.chat?.chain.map((l) => l.node)).toEqual([
      NODE_SPLIT.def.id,
      NODE_CLOUD.def.id,
    ]);
    await waitFor(() => expect(screen.queryByTestId('strategy-editor')).toBeNull());
  });

  it('Set its own nodes, then share with two MLX ways: flagged before save, refused verbatim on save', async () => {
    mockWrite.mockResolvedValueOnce({
      written: false,
      refusals: [
        {
          code: 'sharesTwoWays',
          message:
            'Quick: sharing Build between 27B Atlassian · both Macs and Flash · this Mac would stop one to load the other on every turn',
        },
      ],
      read: readOf(CONFIG),
    });
    renderTab('/nodes?tab=strategies&strategy=quick');
    // Quick already swaps: Chat on Flash, Build on the split.
    expect(screen.getByTestId('strategy-fit-swap')).toHaveTextContent(
      'Flash · this Mac ⇄ 27B Atlassian · both Macs: each switch between them stops one and loads the other.'
    );
    expect(screen.getByTestId('strategy-fit-delegate')).toHaveTextContent(
      'Each delegate call swaps twice: to 27B Atlassian · both Macs and back to Flash · this Mac'
    );
    const build = roleRow('build');
    await userEvent.click(within(build).getByTestId('strategy-add-node'));
    // The next free node is Flash · this Mac: two MLX ways in one Build chain.
    await userEvent.click(within(build).getByRole('radio', { name: 'share' }));
    // Two ways named once each, never a role paired with itself.
    expect(screen.getAllByTestId('strategy-fit-swap').map((p) => p.textContent)).toEqual([
      'Flash · this Mac ⇄ 27B Atlassian · both Macs: each switch between them stops one and loads the other.',
    ]);
    expect(screen.getByTestId('strategy-fit-refusal')).toHaveTextContent(
      'Build: sharing between 27B Atlassian · both Macs and Flash · this Mac would stop one to load the other on every turn'
    );
    expect(within(build).getAllByRole('button', { name: /More work/ })).toHaveLength(2);
    await userEvent.click(screen.getByTestId('strategy-save'));
    const refusals = await screen.findByTestId('strategy-refusals');
    expect(refusals).toHaveTextContent(
      'Quick: sharing Build between 27B Atlassian · both Macs and Flash · this Mac would stop one to load the other on every turn'
    );
    // Refused: nothing closes, the draft is kept.
    expect(screen.getByTestId('strategy-editor')).toBeInTheDocument();
    expect(within(roleRow('build')).getAllByTestId('strategy-chain-entry')).toHaveLength(2);
  });

  it('Same as Chat takes a role back to inheriting; Chat cannot follow an unset Build', async () => {
    renderTab('/nodes?tab=strategies&strategy=local');
    // Local sets Chat only: Chat has no "Same as" (Build would follow Chat back — a cycle).
    expect(within(roleRow('chat')).queryByTestId('strategy-unset')).toBeNull();
    await userEvent.click(within(roleRow('planning')).getByTestId('strategy-set-own'));
    expect(roleRow('planning')).toHaveAttribute('data-set', 'true');
    await userEvent.click(within(roleRow('planning')).getByTestId('strategy-unset'));
    expect(roleRow('planning')).toHaveAttribute('data-set', 'false');
    expect(roleRow('planning')).toHaveTextContent('Same as Chat');
  });
});
