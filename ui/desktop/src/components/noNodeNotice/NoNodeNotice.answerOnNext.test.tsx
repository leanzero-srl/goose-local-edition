import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { IntlProvider } from 'react-intl';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { NodesReadResponse_unstable, NodesResidencyResponse_unstable } from '@aaif/goose-sdk';
import { createUserMessage, type Message } from '../../types/message';
import GooseMessage from '../GooseMessage';
import type { GlanceNodesState } from '../engineGlance/glanceStore';
import { answerOnNextOf } from './answerOnNext';
import { parseNoNodeError } from './parseNoNodeError';
import { CONFIG, NODE_CLOUD, NODE_FLASH, NODE_SPLIT } from '../nodes/nodeGlance.fixtures';
import type { NodesConfig } from '../nodes/model';

/**
 * Q-381 (DESIGN-Q359-CHAT-NODES.md "Failures": "lead can't run, switch off → turn ends offering
 * [Answer on {next} for now]"): the refusal of a chat on its own nodes whose lead could not run
 * offers the set's next node for THIS turn; the click sends the refused text again marked for that
 * node (`append(text, { answerOn })`), the set unchanged. Through the real GooseMessage.
 */

let glance: GlanceNodesState = { kind: 'unread' };
vi.mock('../engineGlance/glanceStore', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../engineGlance/glanceStore')>();
  return {
    ...actual,
    useGlanceNodesWhen: (armed: boolean) => (armed ? glance : { kind: 'unread' }),
  };
});
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineStatus: vi.fn(),
  mlxEngineMount: vi.fn(),
  mlxEngineSettingsRead: vi.fn(),
}));
vi.mock('../ConfigContext', () => ({ useConfig: () => ({ read: vi.fn(), upsert: vi.fn() }) }));

const CHAT = '20260928_4';

function setConfig(answerOnNext: boolean): NodesConfig {
  const nodes = [NODE_CLOUD.def.id, NODE_FLASH.def.id, NODE_SPLIT.def.id];
  return {
    ...CONFIG,
    strategies: [
      ...(CONFIG.strategies ?? []),
      {
        id: `chat-${CHAT}`,
        name: `This chat’s nodes (${CHAT})`,
        chat: CHAT,
        roles: {
          chat: {
            chain: (answerOnNext ? nodes : nodes.slice(0, 1)).map((node) => ({ node, weight: 1 })),
            when: 'failover',
          },
          build: { chain: nodes.map((node) => ({ node, weight: 1 })), when: 'share' },
        },
      },
    ],
  };
}

function glanceOf(config: NodesConfig): GlanceNodesState {
  const read: NodesReadResponse_unstable = {
    config,
    nodes: [NODE_SPLIT, NODE_FLASH, NODE_CLOUD],
    stored: true,
    lmStudioHidden: 0,
  };
  const residency: NodesResidencyResponse_unstable = {
    nodes: [],
    loaderInstalled: true,
    displaced: [],
  };
  return { kind: 'read', read, residency, servedNode: null };
}

/** The router's refusal as the agent loop wraps it: this chat's nodes' Chat chain ran out. */
function refusalText(rows: string): string {
  return `Ran into this error: Execution error: swarm chat: this chat's nodes (chat): no node can serve this turn — ${rows}.\n\nPlease retry if you think this is a transient or recoverable error.`;
}

const LEAD_DOWN = refusalText(`${NODE_CLOUD.def.id}: failed to load: not connected`);

function assistant(text: string): Message {
  return {
    id: 'a1',
    role: 'assistant',
    created: 2,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

function show(text: string, append = vi.fn()) {
  const userTurn = createUserMessage('Summarise the release notes');
  const refusal = assistant(text);
  render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter>
        <GooseMessage
          sessionId={CHAT}
          message={refusal}
          messages={[userTurn, refusal]}
          toolCallNotifications={new Map()}
          append={append}
          isStreaming={false}
        />
      </MemoryRouter>
    </IntlProvider>
  );
  return append;
}

beforeEach(() => {
  glance = glanceOf(setConfig(false));
});

describe('Answer on {next} for now (Q-381)', () => {
  it('offers the set’s next node, and sends the refused text again marked for it', async () => {
    const user = userEvent.setup();
    const append = show(LEAD_DOWN);
    const offer = screen.getByTestId('no-node-answer-on-next');
    expect(offer).toHaveTextContent('Answer on Flash · this Mac for now');
    expect(screen.getByTestId('no-node-answer-on-next-hint')).toHaveTextContent(
      'Sends your message again to Flash · this Mac for this turn only. This chat’s nodes stay as they are, and the next message goes to Claude Sonnet · OpenRouter again.'
    );
    await user.click(offer);
    expect(append).toHaveBeenCalledWith('Summarise the release notes', {
      answerOn: NODE_FLASH.def.id,
    });
    // Retry, which goes back to the lead that just refused, is not the primary move.
    await user.click(screen.getByTestId('no-node-retry'));
    expect(append).toHaveBeenLastCalledWith('Summarise the release notes');
  });

  it('offers nothing when the switch is on, or the refusal is not this chat’s nodes', () => {
    glance = glanceOf(setConfig(true));
    show(LEAD_DOWN);
    expect(screen.queryByTestId('no-node-answer-on-next')).toBeNull();
    cleanup();

    glance = glanceOf(setConfig(false));
    show(
      `Ran into this error: Execution error: swarm chat: the strategy "Everyday" (chat): no node can serve this turn — ${NODE_CLOUD.def.id}: failed to load: not connected.`
    );
    expect(screen.queryByTestId('no-node-answer-on-next')).toBeNull();
  });
});

describe('answerOnNextOf', () => {
  const rowsOf = (text: string) => parseNoNodeError(text) ?? [];

  it('names the first node after the lead that the refusal does not name', () => {
    expect(answerOnNextOf(setConfig(false), CHAT, rowsOf(LEAD_DOWN))).toEqual({
      next: NODE_FLASH.def.id,
      lead: NODE_CLOUD.def.id,
    });
    // An asked turn that also failed on Flash: the same dead end is never offered twice.
    const both = refusalText(
      `${NODE_CLOUD.def.id}: passed over for this turn: you asked to answer on Flash · this Mac for now; ${NODE_FLASH.def.id}: failed to load: memory gate BLOCK`
    );
    expect(answerOnNextOf(setConfig(false), CHAT, rowsOf(both))?.next).toBe(NODE_SPLIT.def.id);
  });

  it('is null for another chat, the switch on, a refusal that is not the lead’s, or every node refused', () => {
    expect(answerOnNextOf(setConfig(false), 'another-chat', rowsOf(LEAD_DOWN))).toBeNull();
    expect(answerOnNextOf(setConfig(true), CHAT, rowsOf(LEAD_DOWN))).toBeNull();
    const notLead = refusalText(`${NODE_FLASH.def.id}: failed to load: x`);
    expect(answerOnNextOf(setConfig(false), CHAT, rowsOf(notLead))).toBeNull();
    const all = refusalText(
      [NODE_CLOUD, NODE_FLASH, NODE_SPLIT].map((n) => `${n.def.id}: down`).join('; ')
    );
    expect(answerOnNextOf(setConfig(false), CHAT, rowsOf(all))).toBeNull();
  });
});
