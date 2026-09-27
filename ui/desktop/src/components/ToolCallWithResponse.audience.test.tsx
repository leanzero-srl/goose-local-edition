import { fireEvent, render, screen } from '@testing-library/react';
import type { SessionNotification } from '@agentclientprotocol/sdk';
import { describe, expect, it } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import ToolCallWithResponse from './ToolCallWithResponse';
import { createAcpSessionNotificationAdapter } from '../acp/sessionNotificationAdapter';
import type {
  Message,
  ToolRequestMessageContent,
  ToolResponseMessageContent,
} from '../types/message';

// Q-212: an MCP server may mark a result item for the assistant alone (context the model reads) or
// the user alone (what the person should see). The engine now carries each item's annotations on
// the ACP update (`content[i].content.annotations`, acp/server.rs `build_tool_call_content`); the
// adapter must keep them and the card must show only what is meant for the person.

const SESSION_ID = 'session-q212';

function update(u: SessionNotification['update']): SessionNotification {
  return { sessionId: SESSION_ID, update: u };
}

function contentOf(messages: Message[]) {
  const all = messages.flatMap((m) => m.content);
  const request = all.find((c) => c.type === 'toolRequest');
  const response = all.find((c) => c.type === 'toolResponse');
  if (request?.type !== 'toolRequest' || response?.type !== 'toolResponse') {
    throw new Error('no tool pair');
  }
  return { request, response };
}

/** The engine's wire for one MCP call whose result carries every audience. */
function mcpCall(
  status: 'completed' | 'failed' = 'completed'
): { request: ToolRequestMessageContent; response: ToolResponseMessageContent } {
  const adapter = createAcpSessionNotificationAdapter();
  adapter.apply(
    update({
      sessionUpdate: 'tool_call',
      toolCallId: 'w1',
      title: 'weather: forecast',
      status: 'pending',
      rawInput: { city: 'Cluj' },
      _meta: { goose: { toolCall: { toolName: 'weather__forecast', extensionName: 'weather' } } },
    })
  );
  const changes = adapter.apply(
    update({
      sessionUpdate: 'tool_call_update',
      toolCallId: 'w1',
      status,
      content: [
        {
          type: 'content',
          content: {
            type: 'text',
            text: 'MODEL-ONLY raw forecast json',
            annotations: { audience: ['assistant'] },
          },
        },
        {
          type: 'content',
          content: {
            type: 'text',
            text: 'PERSON-ONLY Sunny, 21°C',
            annotations: { audience: ['user'] },
          },
        },
        { type: 'content', content: { type: 'text', text: 'EVERYONE forecast source' } },
        {
          type: 'content',
          content: {
            type: 'text',
            text: 'BOTH updated hourly',
            annotations: { audience: ['user', 'assistant'] },
          },
        },
      ],
    })
  );
  const change = changes.find((c) => c.type === 'messages');
  if (!change || change.type !== 'messages') throw new Error('no messages change');
  return contentOf(change.messages);
}

function renderCard(
  toolRequest: ToolRequestMessageContent,
  toolResponse: ToolResponseMessageContent
) {
  render(
    <ToolCallWithResponse
      isCancelledMessage={false}
      toolRequest={toolRequest}
      toolResponse={toolResponse}
      isPendingApproval={false}
    />,
    { wrapper: IntlTestWrapper }
  );
}

/** Opens the card and every output it shows, as a person reading it would. */
function openOutputs(cardLabel: RegExp) {
  fireEvent.click(screen.getByRole('button', { name: cardLabel }));
  for (const output of screen.queryAllByRole('button', { name: 'Output' })) {
    fireEvent.click(output);
  }
}

describe('tool card audience (Q-212)', () => {
  it('the adapter keeps each item’s audience from the ACP update', () => {
    const { response } = mcpCall();
    expect(response.toolResult.status).toBe('success');
    const value = (response.toolResult as { value: { content: { annotations?: unknown }[] } })
      .value;
    expect(value.content.map((c) => c.annotations)).toEqual([
      { audience: ['assistant'] },
      { audience: ['user'] },
      undefined,
      { audience: ['user', 'assistant'] },
    ]);
  });

  it('shows the person only what is meant for them — assistant-only content is hidden', async () => {
    const { request, response } = mcpCall();
    renderCard(request, response);
    openOutputs(/Forecast/);
    expect(screen.getAllByRole('button', { name: 'Output' })).toHaveLength(3);
    expect(await screen.findByText('PERSON-ONLY Sunny, 21°C')).toBeInTheDocument();
    expect(screen.getByText('EVERYONE forecast source')).toBeInTheDocument();
    expect(screen.getByText('BOTH updated hourly')).toBeInTheDocument();
    expect(screen.queryByText(/MODEL-ONLY/)).not.toBeInTheDocument();
  });

  it('a failed call’s error line never quotes assistant-only content', () => {
    const { request, response } = mcpCall('failed');
    expect(response.toolResult.status).toBe('error');
    renderCard(request, response);
    const failure = screen.getByTestId('tool-call-failure');
    expect(failure).toHaveTextContent('PERSON-ONLY Sunny');
    expect(failure).not.toHaveTextContent('MODEL-ONLY');
  });

  it('a result with no annotations shows every item, as before', async () => {
    const adapter = createAcpSessionNotificationAdapter();
    adapter.apply(
      update({
        sessionUpdate: 'tool_call',
        toolCallId: 's1',
        title: 'developer: shell',
        status: 'pending',
        rawInput: { command: 'ls' },
      })
    );
    const changes = adapter.apply(
      update({
        sessionUpdate: 'tool_call_update',
        toolCallId: 's1',
        status: 'completed',
        content: [
          { type: 'content', content: { type: 'text', text: 'a.md' } },
          { type: 'content', content: { type: 'text', text: 'b.md' } },
        ],
      })
    );
    const change = changes.find((c) => c.type === 'messages');
    if (!change || change.type !== 'messages') throw new Error('no messages change');
    const { request, response } = contentOf(change.messages);
    const value = (response.toolResult as { value: { content: object[] } }).value;
    expect(value.content).toEqual([
      { type: 'text', text: 'a.md' },
      { type: 'text', text: 'b.md' },
    ]);
    renderCard(request, response);
    openOutputs(/shell · ls/);
    expect(screen.getAllByRole('button', { name: 'Output' })).toHaveLength(2);
    expect(await screen.findByText('a.md')).toBeInTheDocument();
    expect(screen.getByText('b.md')).toBeInTheDocument();
  });
});
