import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import type { BuildEligibility } from '../../acp/nodes';
import { CONFIG, NODE_CLOUD, NODE_FLASH, NODE_SPLIT } from './nodeGlance.fixtures';
import type { NodesConfig } from './model';
import type { Read } from './nodeGlance';

const mockWrite = vi.fn();
vi.mock('../../acp/nodes', () => ({ nodesWrite: (...a: unknown[]) => mockWrite(...a) }));
const mockRefresh = vi.fn();
vi.mock('../engineGlance/glanceStore', () => ({ refreshGlanceNodes: () => mockRefresh() }));

import { UseSelectors } from './UseSelectors';

const NODES = [NODE_SPLIT, NODE_FLASH, NODE_CLOUD];
const LOCAL = {
  id: 'local',
  name: 'Local',
  roles: { chat: { chain: [{ node: NODE_FLASH.def.id, weight: 1 }] } },
};
const WITH_LOCAL: NodesConfig = { ...CONFIG, strategies: [...CONFIG.strategies!, LOCAL] };
const ELIGIBILITY: Record<string, Read<BuildEligibility>> = {
  everyday: {
    kind: 'read',
    value: {
      eligible: false,
      reasons: [
        {
          reason: { kind: 'split', node: NODE_SPLIT.def.id },
          message: '27B Atlassian · both Macs is a split',
        },
      ],
    },
  },
  local: { kind: 'read', value: { eligible: true, reasons: [], notes: [] } },
};

function renderIt(config: NodesConfig = WITH_LOCAL) {
  return render(
    <IntlTestWrapper>
      <UseSelectors config={config} nodes={NODES} eligibility={ELIGIBILITY} />
    </IntlTestWrapper>
  );
}

const chats = () => screen.getByRole('combobox', { name: 'New chats start on:' });
const builds = () => screen.getByRole('combobox', { name: 'Swarm builds use:' });

beforeEach(() => {
  mockWrite.mockImplementation(async () => ({ written: true, refusals: [] }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('UseSelectors — New chats start on / Swarm builds use', () => {
  it('shows what is stored: a strategy for new chats, the pool for builds', () => {
    const { container } = renderIt();
    expect(chats()).toHaveTextContent('Everyday (strategy)');
    expect(builds()).toHaveTextContent('Your swarm pool');
    assertStudioClean(container);
  });

  it('lists Auto, every strategy and every node; picking a node writes forNewChats through nodes/write', async () => {
    renderIt();
    await userEvent.click(chats());
    const list = screen.getByRole('listbox', { name: 'New chats start on:' });
    expect(within(list).getAllByRole('option').map((o) => o.textContent)).toEqual([
      'Any node (Auto)',
      'Everyday (strategy)',
      'Local (strategy)',
      '27B Atlassian · both Macs (node)',
      'Flash · this Mac (node)',
      'Claude Sonnet · OpenRouter (node)',
    ]);
    await userEvent.click(screen.getByTestId(`use-chats-node:${NODE_FLASH.def.id}`));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    expect(written.forNewChats).toEqual({ kind: 'node', id: NODE_FLASH.def.id });
    // Everything else rides the same write untouched.
    expect(written.forBuilds).toEqual(CONFIG.forBuilds);
    expect(written.strategies).toEqual(WITH_LOCAL.strategies);
    expect(mockRefresh).toHaveBeenCalled();
  });

  it('a refusal is shown verbatim and nothing else is claimed', async () => {
    mockWrite.mockResolvedValueOnce({
      written: false,
      refusals: [{ code: 'unknownNode', message: 'there is no node flash-here' }],
    });
    renderIt();
    await userEvent.click(chats());
    await userEvent.click(screen.getByTestId('use-chats-auto'));
    expect(await screen.findByTestId('use-refused')).toHaveTextContent(
      'there is no node flash-here'
    );
  });

  it('a strategy swarm builds cannot use stays visible with its reason and cannot be picked', async () => {
    renderIt();
    await userEvent.click(builds());
    const everyday = screen.getByTestId('use-builds-strategy:everyday');
    expect(everyday).toBeDisabled();
    expect(everyday).toHaveTextContent('27B Atlassian · both Macs is a split');
    await userEvent.click(everyday);
    expect(mockWrite).not.toHaveBeenCalled();
    await userEvent.click(screen.getByTestId('use-builds-strategy:local'));
    await waitFor(() => expect(mockWrite).toHaveBeenCalledTimes(1));
    expect((mockWrite.mock.calls[0][0] as NodesConfig).forBuilds).toEqual({
      kind: 'strategy',
      id: 'local',
    });
  });

  it('a stored choice whose target is gone is named, never shown as Auto', () => {
    renderIt({ ...WITH_LOCAL, forNewChats: { kind: 'node', id: 'gone' } });
    expect(chats()).toHaveTextContent('gone (no longer defined)');
  });
});
