import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import type { NodeGlance } from './nodeGlance';
import type { ResolvedNodeDef } from './model';
import { NODE_CLOUD, NODE_FLASH, NODE_STUDIO } from './nodeGlance.fixtures';

const GLANCES: Record<string, NodeGlance> = {
  [NODE_STUDIO.def.id]: {
    state: 'serving',
    line: { kind: 'live', stage: 'running', hero: null, chat: null },
    action: 'stop',
    where: { kind: 'mac', name: 'Work’s Mac Studio' },
    figures: null,
    memory: [],
    detail: null,
    displaces: null,
  },
  [NODE_FLASH.def.id]: {
    state: 'ready',
    line: { kind: 'startsIn', medianMs: 48_000, count: 3 },
    action: 'start',
    where: { kind: 'thisMac' },
    figures: null,
    memory: [],
    detail: null,
    displaces: null,
  },
  [NODE_CLOUD.def.id]: {
    state: 'cloudReady',
    line: { kind: 'cloudAlways', provider: 'OpenRouter', endpoint: false },
    action: null,
    where: { kind: 'provider', name: 'OpenRouter' },
    figures: null,
    memory: [],
    detail: null,
    displaces: null,
  },
};
const NODES: ResolvedNodeDef[] = [NODE_STUDIO, NODE_FLASH, NODE_CLOUD];

vi.mock('../leanzero-swarm/useMacs', () => ({
  WithMacs: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock('./useNodeFacts', () => ({
  useNodeFacts: () => ({
    store: {
      kind: 'read',
      read: { config: { version: 1 }, nodes: NODES, stored: true, lmStudioHidden: 0 },
      residency: { nodes: [], loaderInstalled: true, displaced: [] },
      servedNode: null,
    },
    nodes: NODES,
    servingNode: NODE_STUDIO,
    glanceOf: (node: ResolvedNodeDef) => GLANCES[node.def.id],
  }),
}));
vi.mock('../engineGlance/glanceStore', () => ({ useEngineGlance: () => null }));

import { AddChatNodeDialog, type AddChatNodeDialogProps } from './AddChatNodeDialog';

afterEach(cleanup);

function show(
  setChatNodes: AddChatNodeDialogProps['setChatNodes'] = vi.fn(async () => ({
    applied: true as const,
    model: 'strategy:x',
  }))
) {
  const onClose = vi.fn();
  render(
    <IntlTestWrapper>
      <AddChatNodeDialog
        sessionId="7"
        now={{ kind: 'node', node: NODE_STUDIO.def.id }}
        setChatNodes={setChatNodes}
        onClose={onClose}
      />
    </IntlTestWrapper>
  );
  return { setChatNodes, onClose };
}

describe('Add a node to this chat (Q-359)', () => {
  it('lists every node the chat lacks; one that can’t join says why at full strength, with no Add', () => {
    show();
    expect(screen.getByTestId('add-chat-node-context')).toHaveTextContent(
      '27B · Work’s Mac Studio answers this chat. Its delegates share every node you add.'
    );
    expect(screen.queryByTestId(`add-chat-node-${NODE_STUDIO.def.id}`)).toBeNull();
    const flash = screen.getByTestId(`add-chat-node-${NODE_FLASH.def.id}`);
    expect(flash).toHaveAttribute('data-addable', 'no');
    expect(within(flash).getByTestId('add-chat-node-line')).toHaveTextContent(
      'Can’t run beside 27B · Work’s Mac Studio yet: your Macs serve goose one model at a time'
    );
    expect(within(flash).queryByTestId('add-chat-node-add')).toBeNull();
    // Never greyed: no opacity, nothing disabled on the row.
    expect(flash.className).not.toMatch(/opacity/);
    const cloud = screen.getByTestId(`add-chat-node-${NODE_CLOUD.def.id}`);
    expect(cloud).toHaveAttribute('data-addable', 'yes');
    expect(within(cloud).getByTestId('add-chat-node-line')).toHaveTextContent(
      'Always available · billed by OpenRouter'
    );
    assertStudioClean(screen.getByTestId('add-chat-node-list'));
  });

  it('Add stores the set through the one door, lead first, and closes', async () => {
    const { setChatNodes, onClose } = show();
    const cloud = screen.getByTestId(`add-chat-node-${NODE_CLOUD.def.id}`);
    await userEvent.click(within(cloud).getByTestId('add-chat-node-add'));
    expect(setChatNodes).toHaveBeenCalledWith([NODE_STUDIO.def.id, NODE_CLOUD.def.id], false);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it('a refusal is shown in goosed’s words and the dialog stays', async () => {
    const refused = vi.fn(async () => ({
      applied: false as const,
      refusals: ['"This chat’s nodes (7)": sharing build between A and B would stop one'],
    }));
    const { onClose } = show(refused);
    const cloud = screen.getByTestId(`add-chat-node-${NODE_CLOUD.def.id}`);
    await userEvent.click(within(cloud).getByTestId('add-chat-node-add'));
    const alert = await screen.findByTestId('add-chat-node-refusals');
    expect(alert).toHaveTextContent('Not added');
    expect(alert).toHaveTextContent('sharing build between A and B would stop one');
    expect(onClose).not.toHaveBeenCalled();
  });
});
