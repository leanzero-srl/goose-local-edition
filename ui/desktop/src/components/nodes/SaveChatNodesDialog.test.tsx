import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { NodesConfig } from './model';
import { CONFIG, NODE_CLOUD } from './nodeGlance.fixtures';

const mockWrite = vi.fn();
vi.mock('../../acp/nodes', () => ({ nodesWrite: (...a: unknown[]) => mockWrite(...a) }));
vi.mock('../engineGlance/glanceStore', () => ({ refreshGlanceNodes: vi.fn() }));

import { SaveChatNodesDialog } from './SaveChatNodesDialog';

const WITH_SET: NodesConfig = {
  ...CONFIG,
  strategies: [
    ...(CONFIG.strategies ?? []),
    {
      id: 'chat-7',
      name: 'This chat’s nodes (7)',
      chat: '7',
      roles: { chat: { chain: [{ node: NODE_CLOUD.def.id, weight: 1 }], when: 'failover' } },
    },
  ],
};

afterEach(() => {
  cleanup();
  mockWrite.mockReset();
});

function show() {
  const onClose = vi.fn();
  render(
    <IntlTestWrapper>
      <SaveChatNodesDialog config={WITH_SET} strategyId="chat-7" onClose={onClose} />
    </IntlTestWrapper>
  );
  return { onClose };
}

describe('Save this chat’s nodes as a strategy (Q-359)', () => {
  it('names the set and makes it no chat’s own, through the one write door', async () => {
    mockWrite.mockResolvedValueOnce({ written: true, refusals: [] });
    const { onClose } = show();
    const save = screen.getByTestId('save-chat-nodes-save');
    expect(save).toBeDisabled();
    await userEvent.type(screen.getByTestId('save-chat-nodes-name'), 'Pair');
    await userEvent.click(save);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    const written = mockWrite.mock.calls[0][0] as NodesConfig;
    const saved = written.strategies?.find((s) => s.id === 'chat-7');
    expect(saved?.name).toBe('Pair');
    expect(saved?.chat).toBeUndefined();
    expect(written.strategies?.map((s) => s.id)).toEqual(['everyday', 'chat-7']);
  });

  it('a refusal is shown in goosed’s words and the dialog stays', async () => {
    mockWrite.mockResolvedValueOnce({
      written: false,
      refusals: [{ code: 'duplicateName', message: 'two strategies are named "Everyday"' }],
    });
    const { onClose } = show();
    await userEvent.type(screen.getByTestId('save-chat-nodes-name'), 'Everyday');
    await userEvent.click(screen.getByTestId('save-chat-nodes-save'));
    expect(await screen.findByTestId('save-chat-nodes-refusals')).toHaveTextContent(
      'two strategies are named "Everyday"'
    );
    expect(onClose).not.toHaveBeenCalled();
  });
});
