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

  it('Remove: each refusal offers its way through, and the second try carries the choices', async () => {
    mockRemove
      .mockResolvedValueOnce({
        written: false,
        refusals: [
          {
            code: 'nodeInUse',
            message:
              '"27B Atlassian · both Macs" is used by Everyday; remove it from those strategies too',
          },
          {
            code: 'liveSessionsNotAcknowledged',
            message:
              '3 chats are set to "27B Atlassian · both Macs"; their next message will say it was removed',
          },
        ],
        read: readOf([]),
      })
      .mockResolvedValueOnce({ written: true, refusals: [], read: readOf([]) });
    renderTab();
    const split = screen.getAllByTestId('node-card')[0];
    await userEvent.click(within(split).getByTestId('node-more'));
    await userEvent.click(await screen.findByTestId('node-remove'));
    const dialog = await screen.findByTestId('node-remove-dialog');
    expect(dialog).toHaveTextContent('Remove 27B Atlassian · both Macs?');
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_SPLIT.def.id, {
      alsoFromStrategies: false,
      andNewChatsAuto: false,
    });
    const refusals = await screen.findByTestId('node-remove-refusals');
    expect(refusals).toHaveTextContent('is used by Everyday');
    await userEvent.click(within(refusals).getByTestId('node-remove-from-strategies'));
    await userEvent.click(within(refusals).getByTestId('node-remove-acknowledge'));
    expect(within(refusals).getByTestId('node-remove-acknowledge')).toHaveTextContent(
      '3 chats are set to this node. Their next message will say it was removed.'
    );
    await userEvent.click(screen.getByTestId('node-remove-confirm'));
    expect(mockRemove).toHaveBeenLastCalledWith(NODE_SPLIT.def.id, {
      alsoFromStrategies: true,
      andNewChatsAuto: false,
      acknowledgedSessions: 3,
    });
    await waitFor(() => expect(screen.queryByTestId('node-remove-dialog')).toBeNull());
    expect(mockRefresh).toHaveBeenCalled();
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
});
