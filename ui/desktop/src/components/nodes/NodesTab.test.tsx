import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, useLocation } from 'react-router-dom';
import type { ReactNode } from 'react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import type { Mac } from '../leanzero-swarm/macs';
import type { GlanceNodesState } from '../engineGlance/glanceStore';
import {
  CONFIG,
  MODEL_27B,
  MODEL_FLASH,
  NODE_CLOUD,
  NODE_FLASH,
  NODE_POOL,
  NODE_SPLIT,
  OPENROUTER,
  PLANS,
  SELF_MAC,
  TWO_MACS,
  WAY_SPLIT,
} from './nodeGlance.fixtures';
import type { NodesRead } from '../../acp/nodes';
import type { NodesConfig, ResolvedNodeDef } from './model';

const store = vi.hoisted(() => ({ state: { kind: 'unread' } as unknown }));
const mockRefresh = vi.fn();
vi.mock('../engineGlance/glanceStore', () => ({
  useGlanceNodes: () => store.state,
  useEngineGlance: () => null,
  refreshGlanceNodes: () => mockRefresh(),
}));
const macsNow = vi.hoisted(() => ({ macs: [] as unknown[] }));
vi.mock('../leanzero-swarm/useMacs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../leanzero-swarm/useMacs')>()),
  WithMacs: ({ children }: { children: ReactNode }) => <>{children}</>,
  useMacs: () => ({
    macs: macsNow.macs,
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
const mockEnsure = vi.fn();
const mockLoads = vi.fn();
const mockRemove = vi.fn();
const mockWrite = vi.fn();
const mockRead = vi.fn();
vi.mock('../../acp/nodes', () => ({
  nodesEnsureServing: (...a: unknown[]) => mockEnsure(...a),
  nodesLoadHistory: (...a: unknown[]) => mockLoads(...a),
  nodesRemoveNode: (...a: unknown[]) => mockRemove(...a),
  nodesWrite: (...a: unknown[]) => mockWrite(...a),
  nodesRead: (...a: unknown[]) => mockRead(...a),
}));
const mockProviders = vi.fn();
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: () => mockProviders(),
  acpListProviderLiveModels: vi.fn(async () => []),
}));
vi.mock('../../acp/mlx-engine', () => ({ mlxEngineUnmount: vi.fn() }));
vi.mock('../../acp/mlx-distributed', () => ({ mlxDistributedStop: vi.fn() }));
vi.mock('../leanzero-swarm/routeSwitch', () => ({ dropRoute: vi.fn() }));
vi.mock('../leanzero-swarm/cutGuard', () => ({
  useCutGuard: () => ({
    guard: (_e: unknown, _a: unknown, run: () => void) => run(),
    dialog: null,
  }),
}));

import { NodesTab } from './NodesTab';

function readOf(
  nodes: ResolvedNodeDef[],
  config: NodesConfig = CONFIG,
  extra: Partial<NodesRead> = {}
): NodesRead {
  return {
    config: { ...config, defs: nodes.map((n) => n.def) },
    nodes,
    stored: true,
    lmStudioHidden: 0,
    ...extra,
  };
}

function readState(read: NodesRead, residency?: unknown): GlanceNodesState {
  return {
    kind: 'read',
    read,
    residency: (residency as never) ?? {
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

function Where() {
  const l = useLocation();
  return <span data-testid="where">{l.pathname + l.search}</span>;
}

function renderTab(path = '/nodes?tab=nodes') {
  const onEditInPool = vi.fn();
  const view = render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[path]}>
        <NodesTab onEditInPool={onEditInPool} />
        <Where />
      </MemoryRouter>
    </IntlTestWrapper>
  );
  return { ...view, onEditInPool };
}

beforeEach(() => {
  macsNow.macs = [...TWO_MACS] as Mac[];
  store.state = readState(readOf([NODE_SPLIT, NODE_FLASH, NODE_POOL, NODE_CLOUD]));
  mockPlan.mockResolvedValue({ plans: PLANS, nodes: [], storeErrors: [], probeMs: 1 });
  mockLoads.mockResolvedValue({ groups: [], path: '/x' });
  mockProviders.mockResolvedValue([OPENROUTER]);
  mockEnsure.mockResolvedValue({
    kind: 'refused',
    code: 'loaderAbsent',
    reason:
      'loading nodes is not available in this goose process; start Flash · this Mac in Run it',
  });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('NodesTab', () => {
  it('draws two groups of cards, each with its manage link, and nothing else reads on a clock', async () => {
    const { container } = renderTab();
    const macs = screen.getByTestId('nodes-group-macs');
    const cloud = screen.getByTestId('nodes-group-cloud');
    expect(
      within(macs)
        .getAllByTestId('node-card')
        .map((c) => c.getAttribute('data-node'))
    ).toEqual([NODE_SPLIT.def.id, NODE_FLASH.def.id, NODE_POOL.def.id]);
    expect(
      within(cloud)
        .getAllByTestId('node-card')
        .map((c) => c.getAttribute('data-node'))
    ).toEqual([NODE_CLOUD.def.id]);
    await waitFor(() =>
      expect(within(cloud).getByTestId('node-state')).toHaveAttribute('data-state', 'cloudReady')
    );
    // One plan read per goal in use (only chat here), one load-history read per pinned node.
    await waitFor(() => expect(mockPlan).toHaveBeenCalledTimes(1));
    expect(mockPlan).toHaveBeenCalledWith('chat', undefined);
    expect(mockLoads.mock.calls.map((c) => c[0]).sort()).toEqual(
      [NODE_FLASH.def.id, NODE_SPLIT.def.id].sort()
    );
    // Used by, from the config's strategies.
    const split = within(macs).getAllByTestId('node-card')[0];
    expect(within(split).getByTestId('node-used-by')).toHaveTextContent('Chat 1st · Everyday');
    assertStudioClean(container);
  });

  it('an empty config: No nodes yet, with Set up your Macs and New node', async () => {
    store.state = readState(readOf([]));
    renderTab();
    const empty = screen.getByTestId('nodes-empty');
    expect(empty).toHaveTextContent('No nodes yet');
    expect(empty).toHaveTextContent('Connect your Macs and run a model, or add a cloud model.');
    await userEvent.click(within(empty).getByRole('button', { name: 'Set up your Macs' }));
    expect(screen.getByTestId('where')).toHaveTextContent('/leanzero-swarm?tab=mlx&mlx=macs');
  });

  it('one Mac: the hint to add another, leading to My Macs', () => {
    macsNow.macs = [SELF_MAC];
    renderTab();
    expect(screen.getByTestId('nodes-one-mac')).toHaveTextContent(
      'Add another Mac to run models too big for this one'
    );
  });

  it('LM Studio devices in the swarm config are counted, not shown', () => {
    store.state = readState(readOf([NODE_FLASH], CONFIG, { lmStudioHidden: 2 }));
    renderTab();
    expect(screen.getByTestId('nodes-lmstudio-hidden')).toHaveTextContent(
      '2 LM Studio devices in your swarm config are not shown here'
    );
  });

  it('a failed read is its words with Read again; unread says it is reading', async () => {
    store.state = { kind: 'failed', error: 'goosed is not answering' };
    const view = renderTab();
    expect(screen.getByTestId('nodes-read-failed')).toHaveTextContent('goosed is not answering');
    await userEvent.click(screen.getByRole('button', { name: 'Read again' }));
    expect(mockRefresh).toHaveBeenCalled();
    view.unmount();
    store.state = { kind: 'unread' };
    renderTab();
    expect(screen.getByTestId('nodes-reading')).toBeInTheDocument();
  });

  it('Start asks goosed to serve the node and shows its answer verbatim on the card', async () => {
    renderTab();
    const flash = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === NODE_FLASH.def.id)!;
    await userEvent.click(within(flash).getByTestId('node-start'));
    expect(mockEnsure).toHaveBeenCalledWith(NODE_FLASH.def.id);
    expect(await within(flash).findByTestId('node-notice')).toHaveTextContent(
      'loading nodes is not available in this goose process; start Flash · this Mac in Run it'
    );
    expect(mockRefresh).toHaveBeenCalled();
  });

  const openRemove = async (nodeId: string) => {
    const nodeCard = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === nodeId)!;
    await userEvent.click(within(nodeCard).getByTestId('node-more'));
    await userEvent.click(await screen.findByTestId('node-remove'));
    return screen.findByTestId('node-remove-dialog');
  };
  const panelText = () => screen.getByTestId('node-remove-dialog').parentElement!.textContent ?? '';

  it('Remove (Q-259): on open, no red and nothing twice; the strategies box says what it does and Remove says why it waits', async () => {
    mockRemove.mockResolvedValueOnce({ written: true, refusals: [], read: readOf([]) });
    renderTab();
    const dialog = await openRemove(NODE_SPLIT.def.id);
    expect(dialog).toHaveTextContent('Remove 27B Atlassian · both Macs?');
    expect(screen.queryByTestId('node-remove-refusals')).toBeNull();
    expect(panelText()).not.toContain('Not removed');
    // Everyday's chat and build chains name the split: one box, named by the strategy, with its why.
    const box = screen.getByTestId('node-remove-from-strategies');
    expect(box).toHaveTextContent('Also remove it from Everyday');
    expect(box).toHaveTextContent('A strategy uses this node, so it can’t be removed on its own.');
    // CONFIG's new chats start on the Everyday strategy, not this node: no Auto box.
    expect(screen.queryByTestId('node-remove-and-auto')).toBeNull();
    const confirm = screen.getByTestId('node-remove-confirm');
    expect(confirm).toBeDisabled();
    expect(screen.getByTestId('node-remove-blocked')).toHaveTextContent(
      'Tick the box above to remove it'
    );
    await userEvent.click(confirm);
    expect(mockRemove).not.toHaveBeenCalled();
    await userEvent.click(box);
    expect(confirm).toBeEnabled();
    expect(screen.queryByTestId('node-remove-blocked')).toBeNull();
    await userEvent.click(confirm);
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_SPLIT.def.id, {
      alsoFromStrategies: true,
      andNewChatsAuto: false,
    });
    await waitFor(() => expect(screen.queryByTestId('node-remove-dialog')).toBeNull());
    expect(mockRefresh).toHaveBeenCalled();
  });

  it('Remove (Q-259, the live 3.0.65 case): the engine\'s chat count becomes one box — not a red "Not removed", not the sentence twice', async () => {
    mockRemove
      .mockResolvedValueOnce({
        written: false,
        refusals: [
          {
            code: 'liveSessionsNotAcknowledged',
            message:
              '1 chat is set to "Flash · this Mac"; their next message will say it was removed',
            liveSessions: 1,
          },
        ],
        read: readOf([]),
      })
      .mockResolvedValueOnce({ written: true, refusals: [], read: readOf([]) });
    renderTab();
    await openRemove(NODE_FLASH.def.id);
    // Flash is in no strategy: nothing to confirm until the engine has counted the chats.
    expect(screen.queryByTestId('node-remove-confirmations')).toBeNull();
    expect(screen.getByTestId('node-remove-confirm')).toBeEnabled();
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_FLASH.def.id, {
      alsoFromStrategies: false,
      andNewChatsAuto: false,
    });
    const box = await screen.findByTestId('node-remove-acknowledge');
    expect(box).toHaveTextContent('Remove it anyway: 1 chat is set to this node');
    expect(box).toHaveTextContent(
      'Its next message will say the node was removed and ask you to pick another.'
    );
    expect(screen.queryByTestId('node-remove-refusals')).toBeNull();
    const text = panelText();
    expect(text).not.toContain('Not removed');
    // The engine's sentence is not printed beside the box's: the count is said once.
    expect(text).not.toContain('their next message will say it was removed');
    expect(text.split('set to this node').length - 1).toBe(1);
    assertStudioClean(screen.getByTestId('node-remove-dialog').parentElement!);
    // Clicking Remove again would re-send the refused call: it is disabled, and says why.
    expect(screen.getByTestId('node-remove-confirm')).toBeDisabled();
    expect(screen.getByTestId('node-remove-blocked')).toHaveTextContent(
      'Tick the box above to remove it'
    );
    await userEvent.click(box);
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_FLASH.def.id, {
      alsoFromStrategies: false,
      andNewChatsAuto: false,
      acknowledgedSessions: 1,
    });
    await waitFor(() => expect(screen.queryByTestId('node-remove-dialog')).toBeNull());
  });

  it('Remove: a node new chats start on offers the Auto box on open; two boxes are counted in the reason', async () => {
    store.state = readState(
      readOf([NODE_SPLIT, NODE_FLASH, NODE_POOL, NODE_CLOUD], {
        ...CONFIG,
        forNewChats: { kind: 'node', id: NODE_SPLIT.def.id },
      })
    );
    renderTab();
    await openRemove(NODE_SPLIT.def.id);
    const auto = screen.getByTestId('node-remove-and-auto');
    expect(auto).toHaveTextContent('Start new chats on Any node (Auto) instead');
    expect(auto).toHaveTextContent(
      'New chats start on this node now, so it can’t be removed on its own.'
    );
    expect(screen.getByTestId('node-remove-blocked')).toHaveTextContent(
      'Tick the 2 boxes above to remove it'
    );
    await userEvent.click(auto);
    expect(screen.getByTestId('node-remove-blocked')).toHaveTextContent(
      'Tick the box above to remove it'
    );
    expect(screen.getByTestId('node-remove-confirm')).toBeDisabled();
  });

  it("Remove: a refusal no box answers is red, after the attempt, in the engine's words", async () => {
    mockRemove.mockResolvedValueOnce({
      written: false,
      refusals: [{ code: 'unknownNode', message: "there is no node 'flash'" }],
      read: readOf([]),
    });
    renderTab();
    await openRemove(NODE_FLASH.def.id);
    expect(screen.queryByTestId('node-remove-refusals')).toBeNull();
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    const refusals = await screen.findByTestId('node-remove-refusals');
    expect(refusals).toHaveTextContent('Not removed');
    expect(refusals).toHaveTextContent("there is no node 'flash'");
    expect(screen.getByTestId('node-remove-confirm')).toBeEnabled();
  });

  it("the live-chat count is the refusal's number, never read out of its words", async () => {
    mockRemove.mockResolvedValueOnce({
      written: false,
      refusals: [
        {
          code: 'liveSessionsNotAcknowledged',
          // Words with a different leading number: only the field decides what is acknowledged.
          message: '9 of them — see the field',
          liveSessions: 2,
        },
      ],
      read: readOf([]),
    });
    renderTab();
    await openRemove(NODE_FLASH.def.id);
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    const box = await screen.findByTestId('node-remove-acknowledge');
    expect(box).toHaveTextContent('Remove it anyway: 2 chats are set to this node');
    expect(panelText()).not.toContain('9 of them');
  });

  it('a live-chat refusal without its count offers nothing to acknowledge (no guessed count) and stays red', async () => {
    mockRemove.mockResolvedValueOnce({
      written: false,
      refusals: [
        {
          code: 'liveSessionsNotAcknowledged',
          message: '3 chats are set to "Flash · this Mac"',
        },
      ],
      read: readOf([]),
    });
    renderTab();
    await openRemove(NODE_FLASH.def.id);
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    const refusals = await screen.findByTestId('node-remove-refusals');
    expect(refusals).toHaveTextContent('Not removed');
    expect(refusals).toHaveTextContent('3 chats are set');
    expect(screen.queryByTestId('node-remove-acknowledge')).toBeNull();
  });

  it('Keep loaded writes the def through nodes/write with every other def kept', async () => {
    mockWrite.mockImplementation(async (config: NodesConfig) => ({
      written: true,
      refusals: [],
      read: readOf([]),
      config,
    }));
    renderTab();
    const flash = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === NODE_FLASH.def.id)!;
    await userEvent.click(within(flash).getByTestId('node-more'));
    await userEvent.click(await screen.findByTestId('node-keep-loaded'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    expect(written.defs?.map((d) => d.id)).toEqual([
      NODE_SPLIT.def.id,
      NODE_FLASH.def.id,
      NODE_POOL.def.id,
      NODE_CLOUD.def.id,
    ]);
    expect(written.defs?.find((d) => d.id === NODE_FLASH.def.id)?.keepLoaded).toBe(true);
    expect(written.strategies).toEqual(CONFIG.strategies);
    expect(await within(flash).findByTestId('node-notice')).toHaveTextContent('Saved.');
  });

  it('Stop on the serving split stops the split', async () => {
    const { mlxDistributedStop } = await import('../../acp/mlx-distributed');
    const read = readOf([NODE_SPLIT, NODE_FLASH]);
    store.state = readState(read, {
      nodes: [
        { node: NODE_SPLIT.def.id, residency: { kind: 'serving' } },
        { node: NODE_FLASH.def.id, residency: { kind: 'notRunning', otherWay: 'the split' } },
      ],
      serving: WAY_SPLIT,
      loaderInstalled: false,
    } as never);
    renderTab();
    const split = screen.getAllByTestId('node-card')[0];
    await userEvent.click(within(split).getByTestId('node-stop'));
    await waitFor(() => expect(mlxDistributedStop).toHaveBeenCalledTimes(1));
    // Flash names what its start would stop.
    const flash = screen.getAllByTestId('node-card')[1];
    expect(within(flash).getByTestId('node-displaces')).toHaveTextContent(
      'Starting it stops 27B Atlassian · both Macs'
    );
  });

  it('#/nodes?tab=nodes&node=<id> rings that card', () => {
    renderTab(`/nodes?tab=nodes&node=${NODE_FLASH.def.id}`);
    const flash = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === NODE_FLASH.def.id)!;
    expect(flash.className).toContain('ring-2');
    const split = screen.getAllByTestId('node-card')[0];
    expect(split.className).not.toContain('ring-2');
  });

  it('Edit in your swarm pool hands over to the host', async () => {
    const { onEditInPool } = renderTab();
    const pool = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === NODE_POOL.def.id)!;
    await userEvent.click(within(pool).getByTestId('node-edit-in-pool'));
    expect(onEditInPool).toHaveBeenCalledTimes(1);
  });

  it('New node opens the dialog', async () => {
    renderTab();
    await userEvent.click(screen.getByTestId('nodes-new'));
    expect(await screen.findByTestId('new-node-dialog')).toHaveTextContent('New node');
  });

  it('Q-310: removing a pool node says what chats run on afterwards and how to get the card back — no “device”', async () => {
    renderTab();
    await openRemove(NODE_POOL.def.id);
    const text = panelText();
    expect(text).toContain(
      'Only this card goes: its model stays in your swarm pool, so chats on Any node (Auto) keep running on it, and so do swarm builds that use the pool. To bring the card back, choose “Show removed pool nodes” on this page.'
    );
    expect(text).not.toContain('device');
  });

  it('Q-310: a removed pool node can be shown again — the write clears the removed list and keeps every def', async () => {
    mockWrite.mockResolvedValueOnce({ written: true, refusals: [], read: readOf([]) });
    const nodes = [NODE_SPLIT, NODE_FLASH, NODE_CLOUD];
    store.state = readState(readOf(nodes, { ...CONFIG, declined: ['mlx-local'] }));
    renderTab();
    expect(screen.getByTestId('nodes-removed-pool')).toHaveTextContent(
      '1 node you removed from your swarm pool is not shown'
    );
    await userEvent.click(screen.getByTestId('nodes-show-removed-pool'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    expect(written.declined).toEqual([]);
    expect(written.defs!.map((d) => d.id)).toEqual(nodes.map((n) => n.def.id));
    expect(mockRefresh).toHaveBeenCalled();
  });

  it('Q-310: nothing removed, nothing offered', () => {
    renderTab();
    expect(screen.queryByTestId('nodes-removed-pool')).toBeNull();
  });
});

describe('Remove — chats’ own node sets (Q-359)', () => {
  it('a node in chats’ sets gets its own box, named by the count, and Remove sends it', async () => {
    const withSets: NodesConfig = {
      ...CONFIG,
      strategies: [
        ...(CONFIG.strategies ?? []),
        ...['7', '8'].map((chat) => ({
          id: `chat-${chat}`,
          name: `This chat’s nodes (${chat})`,
          chat,
          roles: {
            chat: { chain: [{ node: NODE_FLASH.def.id, weight: 1 }], when: 'failover' as const },
            build: {
              chain: [
                { node: NODE_FLASH.def.id, weight: 1 },
                { node: NODE_CLOUD.def.id, weight: 1 },
              ],
              when: 'share' as const,
            },
          },
        })),
      ],
    };
    store.state = readState(readOf([NODE_SPLIT, NODE_FLASH, NODE_POOL, NODE_CLOUD], withSets));
    mockRemove.mockResolvedValueOnce({ written: true, refusals: [], read: readOf([]) });
    renderTab();
    const nodeCard = screen
      .getAllByTestId('node-card')
      .find((c) => c.getAttribute('data-node') === NODE_FLASH.def.id)!;
    // The generated set names never reach the "Used by" list.
    expect(nodeCard.textContent).not.toContain('This chat’s nodes');
    await userEvent.click(within(nodeCard).getByTestId('node-more'));
    await userEvent.click(await screen.findByTestId('node-remove'));
    const box = await screen.findByTestId('node-remove-from-chat-node-sets');
    expect(box).toHaveTextContent('Also take it out of 2 chats’ node sets');
    expect(box).toHaveTextContent('2 chats run on it with other nodes.');
    expect(screen.queryByTestId('node-remove-from-strategies')).toBeNull();
    const confirm = screen.getByTestId('node-remove-confirm');
    expect(confirm).toBeDisabled();
    await userEvent.click(box);
    await userEvent.click(confirm);
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_FLASH.def.id, {
      alsoFromStrategies: false,
      alsoFromChatNodeSets: true,
      andNewChatsAuto: false,
    });
  });
});
