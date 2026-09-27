import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { MyMacs, nodesOnMac } from './MyMacs';
import type { Mac } from './macs';
import type { LinkState } from '../../acp/leanzero-link';
import type { NodesRead, Residency } from '../../acp/nodes';
import type { ResolvedNodeDef } from '../nodes/model';
import { resetEngineGlanceForTests } from '../engineGlance/glanceStore';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * MY MACS (S7, design §8.6): each Mac card names the nodes whose way runs on it — links to their
 * cards on the Nodes page, their live state beside them — and a lone Mac gets an "Add another Mac"
 * card. The Mac facts come from the view's MacsProvider (stubbed here with the two Macs); the nodes
 * come from goosed's nodes/read + nodes/residency through the glance store.
 */

const SELF: Mac = {
  key: 'self',
  isSelf: true,
  nodeId: 'mbp-1',
  name: 'Mihai Macbook',
  hostname: 'mihai-macbook',
  meshIp: '100.64.0.1',
  online: true,
  sessionsActive: 1,
  allows: null,
  pollError: null,
};
const STUDIO: Mac = {
  ...SELF,
  key: 'studio-7f3a',
  isSelf: false,
  nodeId: 'studio-7f3a',
  name: 'Work’s Mac Studio',
  hostname: 'works-mac-studio',
  meshIp: '100.64.0.2',
};

const macsMock = vi.hoisted(() => ({ macs: [] as Mac[] }));
vi.mock('./useMacs', async (importActual) => {
  const actual = await importActual<typeof import('./useMacs')>();
  return {
    ...actual,
    useMacs: () => ({
      macs: macsMock.macs,
      factsOf: () => ({ status: null, statusError: null, models: null, modelsError: null }),
      describeError: (_mac: Mac, reason: string) => reason,
      offText: () => '',
    }),
  };
});
vi.mock('./useMacSummary', () => ({
  useMacSummary: () => ({
    phase: 'idle',
    state: 'idle',
    modelId: null,
    decodeTps: null,
    detail: null,
  }),
}));
vi.mock('./useMlxDistributedStatus', () => ({ useMlxDistributedStatus: () => undefined }));
vi.mock('../../contexts/FeaturesContext', () => ({
  useFeatures: () => ({ leanzeroLink: true, mlxDistributed: true, mlxEngine: true }),
}));
vi.mock('../../acp/config', () => ({ acpUpsertConfig: vi.fn() }));

const acp = vi.hoisted(() => ({ read: vi.fn(), residency: vi.fn() }));
vi.mock('../../acp/nodes', () => ({
  nodesRead: () => acp.read(),
  nodesResidency: () => acp.residency(),
  nodesServedLast: vi.fn(async () => ({})),
}));

function def(
  id: string,
  name: string,
  placement: ResolvedNodeDef['def']['placement'],
  kind: 'mlx' | 'cloud' = 'mlx'
): ResolvedNodeDef {
  return { def: { id, name, kind, placement, origin: 'user' }, modelFrom: { kind: 'own' } };
}

const SPLIT = def('27b-split', '27B · both Macs', {
  kind: 'pipeline',
  macs: ['local', 'link:studio-7f3a'],
  link: 'jaccl',
});
const FLASH_HERE = def('flash-here', 'Flash · this Mac', { kind: 'single', macs: ['local'] });
const FLASH_STUDIO = def('flash-studio', 'Flash · Studio', {
  kind: 'single',
  macs: ['link:studio-7f3a'],
});
const POOL = def('mbp-engine', 'Mihai Macbook engine', { kind: 'follows' });
const ALIAS = def('ssh-box', 'On an ssh alias', { kind: 'single', macs: ['workhorse'] });
const CLOUD = def('sonnet', 'Claude Sonnet · OpenRouter', null, 'cloud');
const ALL = [SPLIT, FLASH_HERE, FLASH_STUDIO, POOL, ALIAS, CLOUD];

function readOf(nodes: ResolvedNodeDef[]): NodesRead {
  return { config: { version: 1 }, nodes, stored: true, lmStudioHidden: 0 };
}

const RESIDENCY: Residency = {
  loaderInstalled: false,
  serving: {
    kind: 'split',
    macs: [],
    link: 'jaccl',
    modelId: 'Mihai-LeanZero/Qwen3.8-27B',
    servedModelId: 'qwen-27b',
    macNames: ['Mihai Macbook', 'Work’s Mac Studio'],
  },
  nodes: [
    { node: '27b-split', residency: { kind: 'serving' } },
    { node: 'mbp-engine', residency: { kind: 'serving' } },
    { node: 'flash-here', residency: { kind: 'notRunning', otherWay: 'Qwen3.8-27B split' } },
    { node: 'flash-studio', residency: { kind: 'loading', phase: 'loading' } },
    { node: 'sonnet', residency: { kind: 'alwaysReady' } },
  ],
};

const LINK: LinkState = {
  auth: { state: 'connected', email: 'user@example.com', meshIp: '100.64.0.1' },
  nodeCount: 2,
};

function Where() {
  const location = useLocation();
  return <div data-testid="where">{location.pathname + location.search}</div>;
}

function renderMacs() {
  return render(
    <MemoryRouter initialEntries={['/leanzero-swarm?tab=mlx&mlx=macs']}>
      <IntlTestWrapper>
        <MyMacs
          email="user@example.com"
          linkState={LINK}
          stale={false}
          disconnecting={false}
          reconnecting={false}
          onDisconnect={vi.fn()}
          onLogout={vi.fn()}
          onReconnect={vi.fn()}
          onLinkChanged={vi.fn(async () => undefined)}
        />
        <Where />
      </IntlTestWrapper>
    </MemoryRouter>
  );
}

beforeEach(() => {
  macsMock.macs = [SELF, STUDIO];
  acp.read.mockResolvedValue(readOf(ALL));
  acp.residency.mockResolvedValue(RESIDENCY);
});
afterEach(() => {
  resetEngineGlanceForTests(null);
  vi.clearAllMocks();
});

describe('nodesOnMac — which nodes a Mac card names', () => {
  it('a split is on every Mac it spans; a single on its one Mac; a follows node is this Mac’s', () => {
    const read = readOf(ALL);
    const ids = (mac: Mac) => nodesOnMac(read, [SELF, STUDIO], mac).map((n) => n.def.id);
    expect(ids(SELF)).toEqual(['27b-split', 'flash-here', 'mbp-engine']);
    expect(ids(STUDIO)).toEqual(['27b-split', 'flash-studio']);
  });

  it('a Mac key no card answers to (an ssh alias, a Mac signed out) and cloud nodes are on no card', () => {
    const read = readOf(ALL);
    const all = [SELF, STUDIO].flatMap((mac) =>
      nodesOnMac(read, [SELF, STUDIO], mac).map((n) => n.def.id)
    );
    expect(all).not.toContain('ssh-box');
    expect(all).not.toContain('sonnet');
  });
});

describe('MyMacs — Nodes on this Mac, and Add another Mac', () => {
  it('each card names its nodes with their live state, and each opens its card on the Nodes page', async () => {
    renderMacs();
    const self = await screen.findByTestId('my-mac-nodes-self');
    expect(self).toHaveTextContent('Nodes on this Mac');
    const selfNodes = within(self).getAllByTestId('my-mac-node');
    expect(selfNodes.map((n) => n.textContent)).toEqual([
      '27B · both Macs',
      'Flash · this Mac',
      'Mihai Macbook engine',
    ]);
    const splitItem = selfNodes[0].closest('li') as HTMLElement;
    expect(within(splitItem).getByTestId('lz-chip')).toHaveTextContent('Serving');
    expect(within(selfNodes[1].closest('li') as HTMLElement).queryByTestId('lz-chip')).toBeNull();

    const studio = screen.getByTestId('my-mac-nodes-studio-7f3a');
    expect(studio).toHaveTextContent('Nodes on Work’s Mac Studio');
    const studioNodes = within(studio).getAllByTestId('my-mac-node');
    expect(studioNodes.map((n) => n.textContent)).toEqual(['27B · both Macs', 'Flash · Studio']);
    expect(
      within(studioNodes[1].closest('li') as HTMLElement).getByTestId('lz-chip')
    ).toHaveTextContent('Loading');

    fireEvent.click(studioNodes[1]);
    expect(screen.getByTestId('where').textContent).toBe('/nodes?tab=nodes&node=flash-studio');
  });

  it('a Mac with no node says nothing about nodes; with one Mac an Add another Mac card says how', async () => {
    macsMock.macs = [SELF];
    acp.read.mockResolvedValue(readOf([FLASH_STUDIO, CLOUD]));
    renderMacs();
    const add = await screen.findByTestId('my-macs-add-another');
    expect(add).toHaveTextContent('Add another Mac');
    expect(add).toHaveTextContent(
      'Sign in to LeanZero Link with the same account on your other Mac. It appears here, and you can run models too big for one Mac across both.'
    );
    await waitFor(() => expect(acp.residency).toHaveBeenCalled());
    expect(screen.queryByTestId('my-mac-nodes-self')).toBeNull();
  });

  it('two Macs: no Add another Mac card', async () => {
    renderMacs();
    await screen.findByTestId('my-mac-nodes-self');
    expect(screen.queryByTestId('my-macs-add-another')).toBeNull();
  });

  it('a failed nodes read is said once, in its words — never an empty list', async () => {
    acp.read.mockRejectedValue(Object.assign(new Error('Invalid params'), { data: 'no goosed' }));
    renderMacs();
    expect(await screen.findByTestId('my-macs-nodes-failed')).toHaveTextContent(
      'The nodes on your Macs could not be read: no goosed'
    );
    expect(screen.queryByTestId('my-mac-nodes-self')).toBeNull();
  });

  it('carries no banned pattern, and every class it emits compiles against main.css', async () => {
    macsMock.macs = [SELF];
    const { container } = renderMacs();
    await screen.findByTestId('my-mac-nodes-self');
    assertStudioClean(container);
    const added = [
      ...allClasses(screen.getByTestId('my-mac-nodes-self')),
      ...allClasses(screen.getByTestId('my-macs-add-another')),
    ].filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(added)).toEqual([]);
  }, 30_000);
});
