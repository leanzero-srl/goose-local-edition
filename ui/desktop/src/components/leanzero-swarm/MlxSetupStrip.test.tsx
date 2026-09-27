import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { MlxSetupStrip, setupNodeFacts, setupSteps, type SetupFacts } from './MlxSetupStrip';
import { TONE_FILL } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

const ALL_DONE: SetupFacts = {
  linkAvailable: true,
  linkConnected: true,
  macsOnline: 2,
  models: 2,
  running: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  nodes: 3,
  runningHasNode: true,
};

const renderStrip = (facts: SetupFacts, onOpen = vi.fn()) => {
  const view = render(<MlxSetupStrip facts={facts} onOpen={onOpen} />, {
    wrapper: IntlTestWrapper,
  });
  return { ...view, onOpen };
};

/** The full strip (the ≥ sm row), step by step: its state and its words. */
const stepsRow = () =>
  within(screen.getByTestId('mlx-setup-steps'))
    .getAllByRole('button')
    .map((b) => [b.getAttribute('data-state'), b.textContent]);

afterEach(cleanup);

describe('setupSteps — each step done or not from the facts; the first open one is Next', () => {
  it('all four done', () => {
    expect(setupSteps(ALL_DONE).map((s) => s.state)).toEqual(['done', 'done', 'done', 'done']);
  });

  it.each<[string, Partial<SetupFacts>, string[]]>([
    ['signed out of Link', { linkConnected: false }, ['next', 'done', 'done', 'done']],
    ['no model on any Mac', { models: 0, running: null }, ['done', 'next', 'later', 'done']],
    ['nothing running', { running: null }, ['done', 'done', 'next', 'done']],
    ['no node yet', { nodes: 0 }, ['done', 'done', 'done', 'next']],
    ['the nodes unread', { nodes: null }, ['done', 'done', 'done', 'next']],
    ['the running way has no node', { runningHasNode: false }, ['done', 'done', 'done', 'next']],
    [
      'nothing runs: step 4 asks nothing of it',
      { running: null, runningHasNode: null },
      ['done', 'done', 'next', 'done'],
    ],
    [
      'a fresh install, signed out',
      { linkConnected: false, macsOnline: 1, models: 0, running: null, nodes: 0 },
      ['next', 'later', 'later', 'later'],
    ],
    [
      'no Link capability: one Mac is a done step 1',
      { linkAvailable: false, linkConnected: false },
      ['done', 'done', 'done', 'done'],
    ],
  ])('%s', (_, patch, states) => {
    expect(setupSteps({ ...ALL_DONE, ...patch }).map((s) => s.state)).toEqual(states);
  });

  it('each step opens where its work is done', () => {
    expect(setupSteps(ALL_DONE).map((s) => s.target)).toEqual([
      { kind: 'mlx', tab: 'macs' },
      { kind: 'mlx', tab: 'models' },
      { kind: 'mlx', tab: 'engine' },
      { kind: 'nodes' },
    ]);
    // Without Link there is no My Macs tab: step 1 opens the Engine tab.
    expect(setupSteps({ ...ALL_DONE, linkAvailable: false })[0].target).toEqual({
      kind: 'mlx',
      tab: 'engine',
    });
  });
});

describe('MlxSetupStrip renders each step state in its words and its solid colour', () => {
  it('all done: four green chips with their facts, no Next, no compact line', () => {
    renderStrip(ALL_DONE);
    expect(stepsRow()).toEqual([
      ['done', '1Your Macs·2 connected'],
      ['done', '2Models·2 on your Macs'],
      ['done', '3Run it·Qwen3.8-27B-Atlassian-Q8-mlx running'],
      ['done', '4Nodes·3 nodes'],
    ]);
    expect(screen.queryByTestId('mlx-setup-compact')).not.toBeInTheDocument();
    for (const [, text] of stepsRow()) expect(text).not.toContain('Next');
    for (const b of within(screen.getByTestId('mlx-setup-steps')).getAllByRole('button')) {
      for (const c of TONE_FILL.ok.split(' ')) expect(b.className).toContain(c);
    }
  });

  it('signed out: step 1 is the blue Next "Connect your other Macs"; one connected Mac reads "1 Mac"', () => {
    renderStrip({ ...ALL_DONE, linkConnected: false });
    const [first] = within(screen.getByTestId('mlx-setup-steps')).getAllByRole('button');
    expect(first.getAttribute('data-state')).toBe('next');
    expect(first.textContent).toBe('1Your Macs·Connect your other MacsNext');
    for (const c of TONE_FILL.accent.split(' ')) expect(first.className).toContain(c);
    cleanup();
    renderStrip({ ...ALL_DONE, macsOnline: 1 });
    expect(stepsRow()[0]).toEqual(['done', '1Your Macs·1 Mac']);
  });

  it('no model: step 2 is Next "Get a model", and Run it after it is a slate later step', () => {
    renderStrip({ ...ALL_DONE, models: 0, running: null });
    const buttons = within(screen.getByTestId('mlx-setup-steps')).getAllByRole('button');
    expect(stepsRow()[1]).toEqual(['next', '2Models·Get a modelNext']);
    expect(stepsRow()[2]).toEqual(['later', '3Run it·Run a model']);
    for (const c of TONE_FILL.stopped.split(' ')) expect(buttons[2].className).toContain(c);
  });

  it('nothing running: step 3 is Next "Run a model"', () => {
    renderStrip({ ...ALL_DONE, running: null });
    expect(stepsRow()[2]).toEqual(['next', '3Run it·Run a modelNext']);
  });

  it('no node: step 4 is Next "Add a node"; one node is singular', () => {
    renderStrip({ ...ALL_DONE, nodes: 0 });
    expect(stepsRow()[3]).toEqual(['next', '4Nodes·Add a nodeNext']);
    cleanup();
    renderStrip({ ...ALL_DONE, nodes: 1 });
    expect(stepsRow()[3]).toEqual(['done', '4Nodes·1 node']);
  });

  it('something runs and no node is that way: step 4 is Next "Save as a node" and saves it', async () => {
    const { onOpen } = renderStrip({ ...ALL_DONE, runningHasNode: false });
    expect(stepsRow()[3]).toEqual(['next', '4Nodes·Save as a nodeNext']);
    await userEvent.click(within(screen.getByTestId('mlx-setup-steps')).getAllByRole('button')[3]);
    expect(onOpen).toHaveBeenCalledWith({ kind: 'saveNode' });
    cleanup();
    // Nothing defined yet and the split running: the same — save what runs.
    renderStrip({ ...ALL_DONE, nodes: 0, runningHasNode: false });
    expect(stepsRow()[3]).toEqual(['next', '4Nodes·Save as a nodeNext']);
  });

  it('an unread pool claims nothing about nodes: the step carries its name only', () => {
    renderStrip({ ...ALL_DONE, nodes: null });
    expect(stepsRow()[3]).toEqual(['next', '4NodesNext']);
  });

  it('narrow windows get "Step n of 4 · <step>" with the Next chip; the full row is ≥ sm only', () => {
    renderStrip({ ...ALL_DONE, running: null });
    const compact = screen.getByTestId('mlx-setup-compact');
    expect(compact.className).toContain('sm:hidden');
    expect(compact.textContent).toBe('Step 3 of 4 · Run it3Run it·Run a modelNext');
    expect(screen.getByTestId('mlx-setup-steps').className).toMatch(/(^|\s)hidden(\s|$)/);
    expect(screen.getByTestId('mlx-setup-steps').className).toContain('sm:flex');
  });

  it('a click opens that step’s place', async () => {
    const { onOpen } = renderStrip({ ...ALL_DONE, models: 0, running: null });
    const buttons = within(screen.getByTestId('mlx-setup-steps')).getAllByRole('button');
    await userEvent.click(buttons[1]);
    expect(onOpen).toHaveBeenLastCalledWith({ kind: 'mlx', tab: 'models' });
    await userEvent.click(buttons[3]);
    expect(onOpen).toHaveBeenLastCalledWith({ kind: 'nodes' });
  });

  it('is a labelled navigation landmark, no step button is named plain "Run" (split-start.mjs keys on it)', () => {
    renderStrip({ ...ALL_DONE, running: null });
    expect(screen.getByRole('navigation', { name: 'Setting up LeanZero MLX' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Run$/ })).toBeNull();
  });

  it('is Studio-clean in every state and every class compiles', async () => {
    const { container } = renderStrip({
      ...ALL_DONE,
      linkConnected: false,
      models: 0,
      running: null,
    });
    assertStudioClean(container);
    const done = renderStrip(ALL_DONE);
    assertStudioClean(done.container);
    const classes = [...allClasses(container), ...allClasses(done.container)].filter(
      (c) => !c.startsWith('lucide')
    );
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('setupNodeFacts — the strip reads the glance store, claims nothing before it lands', () => {
  const residency = (serving: boolean, servingNodes: string[]) => ({
    nodes: servingNodes.map((node) => ({ node, residency: { kind: 'serving' as const } })),
    serving: serving
      ? { kind: 'split' as const, modelId: 'm', servedModelId: 'm', macNames: [] }
      : null,
    loaderInstalled: false,
  });
  const read = (count: number) => ({
    config: { version: 1 },
    nodes: Array.from({ length: count }, (_, i) => ({
      def: { id: `n${i}`, name: `n${i}`, kind: 'mlx' as const, origin: 'user' as const },
      modelFrom: { kind: 'own' as const },
    })),
    stored: true,
    lmStudioHidden: 0,
  });

  it('unread or failed: nothing claimed', () => {
    expect(setupNodeFacts({ kind: 'unread' }, true)).toEqual({ nodes: null, runningHasNode: null });
    expect(setupNodeFacts({ kind: 'failed', error: 'x' }, true)).toEqual({
      nodes: null,
      runningHasNode: null,
    });
  });

  it('a node serves the running way; none does; nothing answers', () => {
    const state = (servingNodes: string[], serving = true) => ({
      kind: 'read' as const,
      read: read(2),
      residency: residency(serving, servingNodes),
      servedNode: null,
    });
    expect(setupNodeFacts(state(['n0']), true)).toEqual({ nodes: 2, runningHasNode: true });
    expect(setupNodeFacts(state([]), true)).toEqual({ nodes: 2, runningHasNode: false });
    expect(setupNodeFacts(state([], false), true)).toEqual({ nodes: 2, runningHasNode: null });
    expect(setupNodeFacts(state([]), false)).toEqual({ nodes: 2, runningHasNode: null });
  });
});
