import type React from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import type { NodesReadResponse_unstable, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import { NodesChipMenu, type ChatNodesControl } from './NodesChipMenu';
import { chatNodesNow } from '../../../nodes/chatNodeAvailability';
import {
  CONFIG,
  NODE_CLOUD,
  NODE_FLASH,
  NODE_SPLIT,
} from '../../../nodes/nodeGlance.fixtures';
import type { NodesConfig } from '../../../nodes/model';

// The chip's menu, always open: its rows are plain elements.
vi.mock('../../../ui/dropdown-menu', () => ({
  DropdownMenuLabel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuSeparator: () => <hr />,
  DropdownMenuItem: ({
    children,
    onClick,
    ...rest
  }: {
    children: React.ReactNode;
    onClick?: () => void;
    [key: string]: unknown;
  }) => (
    <div role="menuitem" {...rest} onClick={onClick}>
      {children}
    </div>
  ),
}));

const SET: NodesConfig = {
  ...CONFIG,
  strategies: [
    ...(CONFIG.strategies ?? []),
    {
      id: 'chat-7',
      name: 'This chat’s nodes (7)',
      chat: '7',
      roles: {
        chat: { chain: [{ node: NODE_SPLIT.def.id, weight: 1 }], when: 'failover' },
        build: {
          chain: [
            { node: NODE_SPLIT.def.id, weight: 1 },
            { node: NODE_CLOUD.def.id, weight: 1 },
          ],
          when: 'share',
        },
      },
    },
  ],
};

const READ: NodesReadResponse_unstable = {
  config: SET,
  nodes: [NODE_SPLIT, NODE_FLASH, NODE_CLOUD],
  stored: true,
  lmStudioHidden: 0,
};
const RESIDENCY: NodesResidencyResponse_unstable = {
  nodes: [
    { node: NODE_SPLIT.def.id, residency: { kind: 'serving' } },
    { node: NODE_FLASH.def.id, residency: { kind: 'notRunning' } },
    { node: NODE_CLOUD.def.id, residency: { kind: 'alwaysReady' } },
  ],
  loaderInstalled: true,
  displaced: [],
};

function show(model: string, control: Partial<ChatNodesControl> = {}) {
  const chatNodes: ChatNodesControl = {
    now: chatNodesNow(SET, '7', model),
    onSetNodes: vi.fn(),
    onAddNode: vi.fn(),
    onSaveAsStrategy: vi.fn(),
    ...control,
  };
  const onPick = vi.fn();
  render(
    <IntlTestWrapper>
      <NodesChipMenu
        read={READ}
        residency={RESIDENCY}
        currentModel={model}
        onPick={onPick}
        onManageNodes={vi.fn()}
        onEngine={vi.fn()}
        onOtherModels={vi.fn()}
        chatNodes={chatNodes}
      />
    </IntlTestWrapper>
  );
  return { chatNodes, onPick };
}

afterEach(cleanup);

describe('the chip’s "This chat’s nodes" (Q-359)', () => {
  it('shows the set as chips — the one that answers marked — with the delegates’ line', () => {
    show('strategy:chat-7');
    const section = screen.getByTestId('chat-nodes-section');
    expect(section).toHaveTextContent('This chat’s nodes');
    const lead = screen.getByTestId(`chat-node-${NODE_SPLIT.def.id}`);
    expect(lead).toHaveAttribute('data-lead', 'yes');
    expect(lead).toHaveTextContent('27B Atlassian · both Macs');
    expect(lead).toHaveTextContent('Serving');
    expect(within(lead).getByTestId('chat-node-answers')).toHaveTextContent('answers');
    expect(screen.getByTestId(`chat-node-${NODE_CLOUD.def.id}`)).toHaveTextContent('Ready');
    expect(screen.getByTestId('chat-nodes-line')).toHaveTextContent(
      'Delegates share these nodes. The chat answers on 27B Atlassian · both Macs.'
    );
    // The lead has no ×; the chat's set is listed under no strategy.
    expect(screen.queryByTestId(`chat-node-remove-${NODE_SPLIT.def.id}`)).toBeNull();
    expect(screen.queryByTestId('nodes-chip-strategy-chat-7')).toBeNull();
    expect(screen.getByTestId('nodes-chip-strategy-everyday')).toBeTruthy();
  });

  it('× takes a node out; the failover switch is off by default and turns on', () => {
    const { chatNodes } = show('strategy:chat-7');
    fireEvent.click(screen.getByTestId(`chat-node-remove-${NODE_CLOUD.def.id}`));
    expect(chatNodes.onSetNodes).toHaveBeenLastCalledWith([NODE_SPLIT.def.id], false);
    const box = screen.getByTestId('chat-nodes-answer-on-next');
    expect(box).toHaveAttribute('aria-checked', 'false');
    expect(box).toHaveTextContent(
      'If 27B Atlassian · both Macs can’t run, answer on the next node'
    );
    fireEvent.click(box);
    expect(chatNodes.onSetNodes).toHaveBeenLastCalledWith(
      [NODE_SPLIT.def.id, NODE_CLOUD.def.id],
      true
    );
  });

  it('opens Add and Save as a strategy', () => {
    const { chatNodes } = show('strategy:chat-7');
    fireEvent.click(screen.getByTestId('chat-nodes-add'));
    expect(chatNodes.onAddNode).toHaveBeenCalled();
    fireEvent.click(screen.getByTestId('chat-nodes-save'));
    expect(chatNodes.onSaveAsStrategy).toHaveBeenCalledWith('chat-7');
  });

  it('one node: its chip, no switch, no Save; a named strategy says adding makes a copy', () => {
    show(`node:${NODE_FLASH.def.id}`);
    expect(screen.getByTestId('chat-nodes-line')).toHaveTextContent(
      'The chat and its delegates run on Flash · this Mac.'
    );
    expect(screen.queryByTestId('chat-nodes-answer-on-next')).toBeNull();
    expect(screen.queryByTestId('chat-nodes-save')).toBeNull();
    cleanup();
    show('strategy:everyday');
    expect(screen.getByTestId('chat-nodes-line')).toHaveTextContent(
      'On Everyday: the chat answers on 27B Atlassian · both Macs. Adding a node gives this chat its own nodes; Everyday stays as it is.'
    );
    expect(screen.queryByTestId(`chat-node-remove-${NODE_CLOUD.def.id}`)).toBeNull();
  });

  it('Auto: no chips, and the first node added answers', () => {
    show('swarm');
    expect(screen.queryByTestId('chat-nodes-chips')).toBeNull();
    expect(screen.getByTestId('chat-nodes-line')).toHaveTextContent(
      'No node answers this chat by name. The first node you add answers it.'
    );
    expect(screen.getByTestId('chat-nodes-add')).toBeTruthy();
  });
});
