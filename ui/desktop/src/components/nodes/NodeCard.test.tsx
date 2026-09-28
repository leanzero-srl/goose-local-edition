import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import { NodeCard, type NodeCardAction } from './NodeCard';
import { nodeGlance, type NodeFacts, type NodeState } from './nodeGlance';
import {
  LOADS_FLASH,
  MODEL_27B,
  NODE_CLOUD,
  NODE_ENDPOINT,
  NODE_FLASH,
  NODE_LOCAL_27B,
  NODE_POOL,
  NODE_POOL_LEFT,
  NODE_SPLIT,
  NODE_STUDIO,
  OPENROUTER,
  SELF_MAC,
  STUDIO_KEY,
  STUDIO_MAC,
  WAY_LOADING,
  WAY_SPLIT,
  engineGlance,
  facts,
  provider,
} from './nodeGlance.fixtures';
import type { ResolvedNodeDef } from './model';

const read = <T,>(value: T) => ({ kind: 'read' as const, value });

function renderCard(
  node: ResolvedNodeDef,
  f: NodeFacts,
  extra: Partial<Parameters<typeof NodeCard>[0]> = {}
) {
  const onAction = vi.fn<(a: NodeCardAction) => void>();
  const view = render(
    <IntlTestWrapper>
      <NodeCard
        node={node}
        glance={nodeGlance(node, f)}
        usedBy={[]}
        onAction={onAction}
        {...extra}
      />
    </IntlTestWrapper>
  );
  return { ...view, onAction };
}

afterEach(cleanup);

/** Every §4.3 state: its chip words, its line, its one action. */
const CASES: {
  name: string;
  node: ResolvedNodeDef;
  facts: NodeFacts;
  state: NodeState;
  chip: string;
  line: string;
  action: string | null;
}[] = [
  {
    name: 'serving',
    node: NODE_SPLIT,
    facts: facts({
      residency: read({ kind: 'serving' }),
      serving: WAY_SPLIT,
      glance: engineGlance(),
    }),
    state: 'serving',
    chip: 'Serving',
    line: 'Writing · Kickoff notes',
    action: 'node-stop',
  },
  {
    name: 'loading',
    node: NODE_FLASH,
    facts: facts({
      residency: read({ kind: 'loading', phase: 'loading' }),
      serving: WAY_LOADING,
      glance: engineGlance({
        stage: 'loading',
        progress: { done: 12.4 * 1024 ** 3, total: 31 * 1024 ** 3, unit: 'bytes' },
      }),
    }),
    state: 'loading',
    chip: 'Loading',
    line: 'Loading weights · 12.4 of 31.0 GB',
    action: null,
  },
  {
    name: 'waiting',
    node: NODE_FLASH,
    facts: facts({ residency: read({ kind: 'waiting', reason: '27B is answering 1' }) }),
    state: 'waiting',
    chip: 'Waiting to load',
    line: '27B is answering 1',
    action: null,
  },
  {
    name: 'ready, measured',
    node: NODE_FLASH,
    facts: facts({ loads: read(LOADS_FLASH) }),
    state: 'ready',
    chip: 'Not loaded',
    line: 'Starts in about 48s · median of 3 loads',
    action: 'node-start',
  },
  {
    name: 'ready, not measured',
    node: NODE_FLASH,
    facts: facts(),
    state: 'ready',
    chip: 'Not loaded',
    line: 'First start not measured yet',
    action: 'node-start',
  },
  {
    name: 'displaced',
    node: NODE_STUDIO,
    facts: facts({
      residency: read({ kind: 'notRunning', otherWay: 'Qwen split across both' }),
      serving: WAY_SPLIT,
      servingNodeName: '27B Atlassian · both Macs',
    }),
    state: 'displaced',
    chip: 'Not loaded',
    line: 'First start not measured yet',
    action: 'node-start',
  },
  {
    name: 'needs a step',
    node: NODE_STUDIO,
    facts: facts({ modelsOn: (key) => (key === STUDIO_KEY ? [] : null) }),
    state: 'needsStep',
    chip: 'Needs a step',
    line: 'Copy the model to Work’s Mac Studio first',
    action: 'node-open-run-it',
  },
  {
    name: 'needs a step: permission',
    node: NODE_SPLIT,
    facts: facts({
      macs: [
        SELF_MAC,
        { ...STUDIO_MAC, allows: { manage_models: true, answer_chat: true, run_split: false } },
      ],
    }),
    state: 'needsStep',
    chip: 'Needs a step',
    line: 'Turn on “Run part of a split model” on Work’s Mac Studio first (LeanZero MLX › My Macs)',
    action: 'node-open-run-it',
  },
  {
    name: 'can’t run: too big',
    node: NODE_LOCAL_27B,
    facts: facts(),
    state: 'cantRun',
    chip: 'Can’t run',
    line: 'Does not fit: short 1.6 GB on Mihai Macbook',
    action: null,
  },
  {
    name: 'can’t run: not connected',
    node: NODE_SPLIT,
    facts: facts({ macs: [SELF_MAC, { ...STUDIO_MAC, online: false }] }),
    state: 'cantRun',
    chip: 'Can’t run',
    line: 'Work’s Mac Studio is not connected to LeanZero Link',
    action: null,
  },
  {
    name: 'held by a build',
    node: NODE_SPLIT,
    facts: facts({ buildHolder: { node: NODE_FLASH.def.id, way: 'Flash · this Mac' } }),
    state: 'heldByBuild',
    chip: 'Held by a build',
    line: 'A swarm build is using Flash · this Mac; it frees when the build ends',
    action: null,
  },
  {
    name: 'follows',
    node: NODE_POOL,
    facts: facts({
      residency: read({ kind: 'serving' }),
      serving: WAY_SPLIT,
      glance: engineGlance(),
    }),
    state: 'follows',
    chip: 'Follows this Mac',
    line: 'Serves whatever this Mac’s engine runs: Qwen3.8-27B-Atlassian-Q8-mlx · split across 2 Macs',
    action: 'node-pin-way',
  },
  {
    name: 'left the pool',
    node: NODE_POOL_LEFT,
    facts: facts(),
    state: 'cantRun',
    chip: 'Can’t run',
    line: 'No longer in your swarm pool',
    action: null,
  },
  {
    name: 'unknown',
    node: NODE_SPLIT,
    facts: facts({
      residency: read({ kind: 'unknown', reason: 'the route record is unreadable' }),
    }),
    state: 'unknown',
    chip: 'State unknown',
    line: 'the route record is unreadable',
    action: null,
  },
  {
    name: 'cloud ready',
    node: NODE_CLOUD,
    facts: facts(),
    state: 'cloudReady',
    chip: 'Ready',
    line: 'Always available · billed by OpenRouter',
    action: null,
  },
  {
    name: 'cloud key missing',
    node: NODE_CLOUD,
    facts: facts({ provider: read(null) }),
    state: 'keyMissing',
    chip: 'Key missing',
    line: 'Set up openrouter under Providers › Cloud Providers',
    action: 'node-set-up',
  },
  {
    name: 'cloud failing',
    node: NODE_CLOUD,
    facts: facts({ provider: read({ ...OPENROUTER, connection_error: '401: invalid key' }) }),
    state: 'failing',
    chip: 'Last call failed',
    line: '401: invalid key',
    action: 'node-details',
  },
  {
    name: 'endpoint ready',
    node: NODE_ENDPOINT,
    facts: facts({
      provider: read(provider('custom_desk_vllm', 'Desk vLLM', { provider_type: 'Custom' })),
    }),
    state: 'cloudReady',
    chip: 'Ready',
    line: 'Always available · served by Desk vLLM',
    action: null,
  },
];

describe('NodeCard — every state says its chip, its line and its one action', () => {
  it.each(CASES)('$name', ({ node, facts: f, state, chip, line, action }) => {
    const { container } = renderCard(node, f);
    const stateChip = screen.getByTestId('node-state');
    expect(stateChip).toHaveAttribute('data-state', state);
    expect(stateChip).toHaveTextContent(chip);
    expect(screen.getByTestId('node-line')).toHaveTextContent(line);
    for (const id of [
      'node-start',
      'node-stop',
      'node-open-run-it',
      'node-set-up',
      'node-pin-way',
      'node-details',
    ]) {
      if (id === action) expect(screen.getByTestId(id)).toBeInTheDocument();
      // `node-details` is also the MLX Details disclosure's test id; only buttons count here.
      else if (id !== 'node-details') expect(screen.queryByTestId(id)).toBeNull();
    }
    assertStudioClean(container);
    // Never greyed: no opacity on the card, whatever the state.
    expect(container.innerHTML).not.toMatch(/opacity/);
  });
});

describe('NodeCard — the parts of a card', () => {
  it('the kind chip is solid ink with its word; the where chip names the Macs', () => {
    renderCard(NODE_SPLIT, facts());
    expect(screen.getByTestId('node-kind')).toHaveTextContent('MLX');
    expect(screen.getByTestId('node-kind').className).toContain('bg-[#111827]');
    expect(screen.getByTestId('node-where')).toHaveTextContent('Split · 2 Macs');
  });

  it('displaced: the slate outline chip and what starting it stops', () => {
    renderCard(
      NODE_STUDIO,
      facts({
        residency: read({ kind: 'notRunning', otherWay: 'x' }),
        serving: WAY_SPLIT,
        servingNodeName: '27B Atlassian · both Macs',
      })
    );
    expect(screen.getByTestId('node-other-way')).toHaveTextContent('Another way is running');
    expect(screen.getByTestId('node-displaces')).toHaveTextContent(
      'Starting it stops 27B Atlassian · both Macs'
    );
  });

  it('a pool node reads its model through the pool and is edited there', async () => {
    const { onAction } = renderCard(NODE_POOL, facts());
    // Q-308: the pool entry's own model name is not a model spelling — it lives in Details; the
    // card's line names what this Mac's engine runs.
    expect(screen.getByTestId('node-model')).toHaveTextContent('from your swarm pool');
    expect(screen.getByTestId('node-model')).not.toHaveTextContent('qwen3.8-27b');
    await userEvent.click(screen.getByRole('button', { name: 'Details' }));
    expect(screen.getByTestId('node-details-line')).toHaveTextContent(
      'Swarm pool entry mlx-local, model name there: qwen3.8-27b'
    );
    expect(screen.queryByTestId('node-edit')).toBeNull();
    await userEvent.click(screen.getByTestId('node-edit-in-pool'));
    expect(onAction).toHaveBeenCalledWith({ kind: 'editInPool' });
  });

  it('Q-308: a node names its model by the short name; the repo id is in Details only', async () => {
    renderCard(NODE_SPLIT, facts());
    const model = screen.getByTestId('node-model');
    expect(model).toHaveTextContent('Qwen3.8-27B-Atlassian-Q8-mlx');
    expect(model.textContent).not.toContain('/');
    await userEvent.click(screen.getByRole('button', { name: 'Details' }));
    expect(screen.getByTestId('node-details-line')).toHaveTextContent(`Model: ${MODEL_27B}`);
  });

  it('Q-303: a follows node shows where it runs NOW — the split’s Macs, not “This Mac”', () => {
    renderCard(NODE_POOL, facts({ residency: read({ kind: 'serving' }), serving: WAY_SPLIT }));
    expect(screen.getByTestId('node-where')).toHaveTextContent('Split · 2 Macs');
    cleanup();
    renderCard(NODE_POOL, facts({ residency: read({ kind: 'notRunning', otherWay: null }) }));
    expect(screen.getByTestId('node-where')).toHaveTextContent('This Mac');
  });

  it('Q-317: the follows card offers a new node for the model it runs, in plain words', () => {
    renderCard(NODE_POOL, facts({ residency: read({ kind: 'serving' }), serving: WAY_SPLIT }));
    expect(screen.getByTestId('node-pin-way')).toHaveTextContent('New node for this model');
    expect(screen.getByTestId('node-pin-way')).not.toHaveTextContent('Pin');
  });

  it('serving: the live figures and memory peak against budget per Mac', () => {
    renderCard(
      NODE_SPLIT,
      facts({ residency: read({ kind: 'serving' }), serving: WAY_SPLIT, glance: engineGlance() })
    );
    expect(screen.getByTestId('node-figures')).toHaveTextContent('11.2tok/s writing');
    const rows = screen.getAllByTestId('node-memory-row');
    expect(rows.map((r) => r.textContent)).toEqual([
      'Mihai Macbook38.6 of 61.8 GB',
      'Work’s Mac Studio38.6 of 66.2 GB',
    ]);
  });

  it('not serving: the planner’s need against budget, red when over', () => {
    renderCard(NODE_LOCAL_27B, facts());
    const row = screen.getByTestId('node-memory-row');
    // Q-306: the need, then what is free — "63.4 of 61.8 GB" read backwards.
    expect(row).toHaveTextContent('needs 63.4 GB, 61.8 GB free');
    expect(within(row).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100');
  });

  it('used by: role chips in their hues, each opening its strategy', async () => {
    const onAction = vi.fn();
    render(
      <IntlTestWrapper>
        <NodeCard
          node={NODE_SPLIT}
          glance={nodeGlance(NODE_SPLIT, facts())}
          usedBy={[
            { role: 'chat', rank: 1, strategyId: 'everyday', strategyName: 'Everyday' },
            { role: 'build', rank: 2, strategyId: 'quick', strategyName: 'Quick' },
          ]}
          onAction={onAction}
        />
      </IntlTestWrapper>
    );
    const chips = within(screen.getByTestId('node-used-by')).getAllByTestId('node-role');
    expect(chips.map((c) => c.textContent)).toEqual(['Chat 1st · Everyday', 'Build 2nd · Quick']);
    expect(chips[0].className).toContain('bg-[#DB2777]');
    await userEvent.click(chips[1]);
    expect(onAction).toHaveBeenCalledWith({ kind: 'openStrategy', id: 'quick' });
  });

  it('the highlighted card carries a solid ring, never a tint', () => {
    renderCard(NODE_FLASH, facts(), { highlighted: true });
    expect(screen.getByTestId('node-card').className).toContain('ring-2');
  });

  it('a notice from the last action is shown in its tone', () => {
    renderCard(NODE_FLASH, facts(), {
      notice: { tone: 'err', text: 'loading nodes is not available in this goose process' },
    });
    expect(screen.getByTestId('node-notice')).toHaveTextContent(
      'loading nodes is not available in this goose process'
    );
  });

  it('Start and Edit call back', async () => {
    const { onAction } = renderCard(NODE_FLASH, facts());
    await userEvent.click(screen.getByTestId('node-start'));
    await userEvent.click(screen.getByTestId('node-edit'));
    expect(onAction.mock.calls.map((c) => c[0])).toEqual([{ kind: 'start' }, { kind: 'edit' }]);
  });

  it('the ⋯ menu: Keep loaded, Duplicate, Show in Run it, Remove', async () => {
    const { onAction } = renderCard(NODE_FLASH, facts());
    await userEvent.click(screen.getByTestId('node-more'));
    const menu = await screen.findByTestId('node-more-menu');
    expect(within(menu).getByText('Keep loaded')).toBeInTheDocument();
    expect(within(menu).getByText('Duplicate')).toBeInTheDocument();
    expect(within(menu).getByText('Show in Run it')).toBeInTheDocument();
    await userEvent.click(within(menu).getByTestId('node-keep-loaded'));
    expect(onAction).toHaveBeenCalledWith({ kind: 'keepLoaded', value: true });
    await userEvent.click(screen.getByTestId('node-more'));
    await userEvent.click(await screen.findByTestId('node-remove'));
    expect(onAction).toHaveBeenCalledWith({ kind: 'remove' });
  });

  it('a pool node’s menu has no Keep loaded or Duplicate (they belong to the pool)', async () => {
    renderCard(NODE_POOL, facts());
    await userEvent.click(screen.getByTestId('node-more'));
    const menu = await screen.findByTestId('node-more-menu');
    expect(within(menu).queryByText('Keep loaded')).toBeNull();
    expect(within(menu).queryByText('Duplicate')).toBeNull();
    expect(within(menu).getByText('Remove')).toBeInTheDocument();
  });
});
