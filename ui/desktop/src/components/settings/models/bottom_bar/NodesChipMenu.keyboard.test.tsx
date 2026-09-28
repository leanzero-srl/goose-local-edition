import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { NodesReadResponse_unstable, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from '../../../ui/dropdown-menu';
import { NodesChipMenu, type ChatNodesControl } from './NodesChipMenu';
import { chatNodesNow } from '../../../nodes/chatNodeAvailability';
import { CONFIG, NODE_CLOUD, NODE_FLASH, NODE_SPLIT } from '../../../nodes/nodeGlance.fixtures';
import type { NodesConfig } from '../../../nodes/model';

/**
 * Q-380: the chip menu driven by the KEYBOARD alone, in the real Radix menu (no mock): the arrow
 * keys reach each chip's × and the failover switch, and Enter / Space act on them. A plain button
 * inside the menu is not one of the menu's items, so the arrows pass it by.
 */

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

function openMenu() {
  const chatNodes: ChatNodesControl = {
    now: chatNodesNow(SET, '7', 'strategy:chat-7'),
    onSetNodes: vi.fn(),
    onAddNode: vi.fn(),
    onSaveAsStrategy: vi.fn(),
  };
  render(
    <IntlTestWrapper>
      <DropdownMenu>
        <DropdownMenuTrigger>chip</DropdownMenuTrigger>
        <DropdownMenuContent>
          <NodesChipMenu
            read={READ}
            residency={RESIDENCY}
            currentModel="strategy:chat-7"
            onPick={vi.fn()}
            onManageNodes={vi.fn()}
            onEngine={vi.fn()}
            onOtherModels={vi.fn()}
            chatNodes={chatNodes}
          />
        </DropdownMenuContent>
      </DropdownMenu>
    </IntlTestWrapper>
  );
  fireEvent.keyDown(screen.getByRole('button', { name: 'chip' }), { key: 'ArrowDown' });
  return chatNodes;
}

/**
 * Arrow down through the menu, as a keyboard-only person does, until `target` has the focus. Radix's
 * roving focus moves on the next task (react-roving-focus `setTimeout(() => focusFirst(…))`), so
 * each press is let through before the focus is read. One press per item is enough to pass them all.
 */
async function arrowTo(target: HTMLElement) {
  const stops = document.querySelectorAll('[role^="menuitem"]').length;
  for (let i = 0; i <= stops && document.activeElement !== target; i++) {
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'ArrowDown' });
    await act(() => new Promise<void>((next) => setTimeout(next, 0)));
  }
  expect(document.activeElement, 'the arrow keys never reached it').toBe(target);
}

afterEach(cleanup);

describe('the chip menu by keyboard (Q-380)', () => {
  it('reaches a chip’s × with the arrows and Enter takes the node out', async () => {
    const chatNodes = openMenu();
    const remove = screen.getByTestId(`chat-node-remove-${NODE_CLOUD.def.id}`);
    expect(remove).toHaveAttribute('role', 'menuitem');
    expect(remove).toHaveAccessibleName('Take Claude Sonnet · OpenRouter out of this chat');
    await arrowTo(remove);
    fireEvent.keyDown(remove, { key: 'Enter' });
    expect(chatNodes.onSetNodes).toHaveBeenCalledWith([NODE_SPLIT.def.id], false);
    // The menu stays open, so the person sees the chip go.
    expect(screen.getByTestId('nodes-chip-menu')).toBeInTheDocument();
  });

  it('reaches the failover switch with the arrows and Space turns it on', async () => {
    const chatNodes = openMenu();
    const box = screen.getByTestId('chat-nodes-answer-on-next');
    expect(box).toHaveAttribute('role', 'menuitemcheckbox');
    expect(box).toHaveAttribute('aria-checked', 'false');
    await arrowTo(box);
    fireEvent.keyDown(box, { key: ' ' });
    expect(chatNodes.onSetNodes).toHaveBeenCalledWith([NODE_SPLIT.def.id, NODE_CLOUD.def.id], true);
    expect(screen.getByTestId('nodes-chip-menu')).toBeInTheDocument();
  });
});
