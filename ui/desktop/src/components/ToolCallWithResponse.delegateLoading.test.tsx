import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { NodeResidency } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../i18n/test-utils';
import type { NotificationEvent, ToolRequestMessageContent } from '../types/message';
import type { GlanceNodesState } from './engineGlance/glanceStore';

/**
 * Q-382 (DESIGN-Q359-CHAT-NODES.md, the delegate card: "Loading 27B · Work's Mac Studio for this
 * delegate: Loading weights"): while a delegate's first model call waits on the node loader, its
 * card says the node is loading FOR it — the delegate named by its start notification before any
 * tool call, the load named by the loader's own fact (`loading.demandedBy`). Through the real card.
 */

let residency: { node: string; residency: NodeResidency }[] = [];
function glance(): GlanceNodesState {
  return {
    kind: 'read',
    read: {
      config: { defs: [], strategies: [], forNewChats: { kind: 'auto' } },
      nodes: [
        { def: { id: 'studio', name: '27B · Work’s Mac Studio' } },
        { def: { id: 'flash', name: 'Flash · this Mac' } },
      ],
      stored: true,
      lmStudioHidden: 0,
    },
    residency: { nodes: residency, loaderInstalled: true, displaced: [] },
    servedNode: null,
  } as unknown as GlanceNodesState;
}
vi.mock('./engineGlance/glanceStore', () => ({
  useGlanceNodes: () => glance(),
  useGlanceNodesWhen: (armed: boolean) => (armed ? glance() : { kind: 'unread' }),
}));
vi.mock('../acp/nodes', () => ({ nodesServedLast: async () => ({}) }));

import ToolCallWithResponse from './ToolCallWithResponse';

const request: ToolRequestMessageContent = {
  type: 'toolRequest',
  id: 'call_1',
  toolCall: {
    status: 'success',
    value: { name: 'summon__delegate', arguments: { instructions: 'Review the parser' } },
  },
};

const started: NotificationEvent = {
  type: 'Notification',
  request_id: 'call_1',
  message: {
    method: 'notifications/message',
    params: {
      level: 'info',
      logger: 'subagent:sub-1',
      data: { type: 'subagent_started', subagent_id: 'sub-1' },
    },
  },
};

function renderRunning(notifications: NotificationEvent[]) {
  render(
    <ToolCallWithResponse
      isCancelledMessage={false}
      toolRequest={request}
      notifications={notifications}
      isStreamingMessage
      isPendingApproval={false}
    />,
    { wrapper: IntlTestWrapper }
  );
}

beforeEach(() => {
  residency = [];
});

describe('the delegate card while its node loads for it (Q-382)', () => {
  it('says which node loads for this delegate and in which phase, the card open', () => {
    residency = [
      { node: 'studio', residency: { kind: 'loading', phase: 'loading', demandedBy: ['sub-1'] } },
    ];
    renderRunning([started]);
    expect(screen.getByTestId('delegate-loading-line')).toHaveTextContent(
      'Loading 27B · Work’s Mac Studio for this delegate: Loading weights'
    );
    // The start names the session only: it is never a log line.
    expect(screen.queryByText(/subagent_started/)).toBeNull();
  });

  it('says nothing for a load that is another session’s', () => {
    residency = [
      { node: 'studio', residency: { kind: 'loading', phase: 'loading', demandedBy: ['sub-9'] } },
    ];
    renderRunning([started]);
    expect(screen.queryByTestId('delegate-loading-line')).toBeNull();
  });

  it('a delegate not yet named by its start has nothing to say', () => {
    residency = [
      { node: 'studio', residency: { kind: 'loading', phase: 'loading', demandedBy: ['sub-1'] } },
    ];
    renderRunning([]);
    expect(screen.queryByTestId('delegate-loading-line')).toBeNull();
  });
});

/**
 * Q-434: two delegates run "at the same time" as background tasks; the parent waits on each with
 * summon's `load(source: <task id>)` — the Working card the person watches. Its source is the
 * delegate's session, so that card says the node loads for this delegate.
 */
describe('the Working load card of a background delegate (Q-434)', () => {
  const load = (source: string): ToolRequestMessageContent => ({
    type: 'toolRequest',
    id: 'call_load',
    toolCall: { status: 'success', value: { name: 'summon__load', arguments: { source } } },
  });
  const renderLoad = (source: string) =>
    render(
      <ToolCallWithResponse
        isCancelledMessage={false}
        toolRequest={load(source)}
        notifications={[]}
        isStreamingMessage
        isPendingApproval={false}
      />,
      { wrapper: IntlTestWrapper }
    );

  it('says the node loads for this delegate while the loader loads it for that task', () => {
    residency = [
      {
        node: 'studio',
        residency: { kind: 'loading', phase: 'loading', demandedBy: ['20260928_42'] },
      },
    ];
    renderLoad('20260928_42');
    expect(screen.getByTestId('delegate-loading-line')).toHaveTextContent(
      'Loading 27B · Work’s Mac Studio for this delegate: Loading weights'
    );
  });

  it('a load of a skill or recipe names no delegate', () => {
    residency = [
      {
        node: 'studio',
        residency: { kind: 'loading', phase: 'loading', demandedBy: ['my-skill'] },
      },
    ];
    renderLoad('my-skill');
    expect(screen.queryByTestId('delegate-loading-line')).toBeNull();
  });
});
