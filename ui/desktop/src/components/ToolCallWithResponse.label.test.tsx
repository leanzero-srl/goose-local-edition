import { render, screen } from '@testing-library/react';
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

// Q-99, measured on E2E #1 / #2b turn 0 (Harbourline brief): the cards read "Ledger Append kind,
// text" and "Shell · cd /Users/…/work 2>/dev/null && pwd && echo "---" && ls -la …", while the
// engine had written a label for each call ("Summarize this tool call …", 28 of them in E2E #1's
// calls.csv) that the desktop dropped. A failed Write showed only the repeat line; the error
// ("missing field `path`") sat in the collapsed output.

const SESSION_ID = 'session-q99';
const LEDGER_ARGS = {
  kind: 'decision',
  text: 'Harbourline: Jira-only scope, 24-month cutoff, project leads never dropped',
};
const SHELL_COMMAND =
  'cd /Users/mihaiperdum/goose-builds/quality/E2E-1-split-tensor/work 2>/dev/null && pwd && echo "---" && ls -la';

function update(u: SessionNotification['update']): SessionNotification {
  return { sessionId: SESSION_ID, update: u };
}

function toolCall(id: string, toolName: string, rawInput: Record<string, unknown>, title: string) {
  return update({
    sessionUpdate: 'tool_call',
    toolCallId: id,
    title,
    status: 'pending',
    rawInput,
    _meta: { goose: { toolCall: { toolName, extensionName: toolName.split('__')[0] } } },
  });
}

function labelUpdate(id: string, toolName: string, title: string, fromModel: boolean) {
  return update({
    sessionUpdate: 'tool_call_update',
    toolCallId: id,
    title,
    _meta: {
      goose: {
        toolCall: { toolName, extensionName: toolName.split('__')[0] },
        ...(fromModel ? { toolTitleFromModel: true } : {}),
      },
    },
  });
}

function lastMessages(
  changes: ReturnType<ReturnType<typeof createAcpSessionNotificationAdapter>['apply']>
) {
  const change = changes.find((c) => c.type === 'messages');
  return change && change.type === 'messages' ? change.messages : undefined;
}

function requestOf(messages: Message[] | undefined, id: string): ToolRequestMessageContent {
  const content = messages
    ?.flatMap((m) => m.content)
    .find((c) => c.type === 'toolRequest' && c.id === id);
  if (!content || content.type !== 'toolRequest') throw new Error(`no request ${id}`);
  return content;
}

function renderCard(
  toolRequest: ToolRequestMessageContent,
  toolResponse?: ToolResponseMessageContent
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

describe('tool card labels (Q-99)', () => {
  it("lands the model's label on the call and the card shows it instead of argument names", () => {
    const adapter = createAcpSessionNotificationAdapter();
    adapter.apply(toolCall('t1', 'ledger__append', LEDGER_ARGS, 'ledger: append · decision'));
    const messages = lastMessages(
      adapter.apply(labelUpdate('t1', 'ledger__append', 'logging the kickoff decision', true))
    );
    const request = requestOf(messages, 't1');
    expect(request.metadata).toMatchObject({
      title: 'logging the kickoff decision',
      titleFromModel: true,
    });

    renderCard(request);
    expect(screen.getByText('Append · logging the kickoff decision')).toBeInTheDocument();
    expect(screen.queryByText(/kind, text/)).not.toBeInTheDocument();
  });

  it("never lets the engine's fallback title replace anything", () => {
    const adapter = createAcpSessionNotificationAdapter();
    adapter.apply(toolCall('t2', 'developer__shell', { command: SHELL_COMMAND }, 'x'));
    const changes = adapter.apply(
      labelUpdate('t2', 'developer__shell', 'developer: shell · cd /Users', false)
    );
    expect(changes.some((c) => c.type === 'messages')).toBe(false);
  });

  it('a label that arrives after the result still lands', () => {
    const adapter = createAcpSessionNotificationAdapter();
    adapter.apply(toolCall('t3', 'developer__shell', { command: SHELL_COMMAND }, 'x'));
    adapter.apply(
      update({
        sessionUpdate: 'tool_call_update',
        toolCallId: 't3',
        status: 'completed',
        content: [{ type: 'content', content: { type: 'text', text: 'ok' } }],
      })
    );
    const messages = lastMessages(
      adapter.apply(labelUpdate('t3', 'developer__shell', 'checking the work directory', true))
    );
    renderCard(requestOf(messages, 't3'));
    expect(screen.getByText('Shell · checking the work directory')).toBeInTheDocument();
  });

  it('with no label yet, an unknown tool shows a value, never its argument names', () => {
    renderCard({
      type: 'toolRequest',
      id: 't4',
      toolCall: { status: 'success', value: { name: 'ledger__append', arguments: LEDGER_ARGS } },
    });
    expect(screen.getByText('Append · decision')).toBeInTheDocument();
    expect(screen.queryByText(/kind, text/)).not.toBeInTheDocument();
  });

  it('a Failed card says the error on its face, beside the repeat line', () => {
    renderCard(
      {
        type: 'toolRequest',
        id: 't5',
        toolCall: {
          status: 'success',
          value: { name: 'developer__write', arguments: { content: '# Kickoff' } },
        },
      },
      {
        type: 'toolResponse',
        id: 't5',
        toolResult: {
          status: 'error',
          error: 'Invalid parameters: missing field `path`',
        },
        metadata: { repeat: 'same_output' },
      }
    );
    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(screen.getByTestId('tool-call-failure')).toHaveTextContent(
      'Invalid parameters: missing field `path`'
    );
    expect(
      screen.getByText(
        'Same call and same output as an earlier call this turn — the model was told'
      )
    ).toBeInTheDocument();
  });

  it('a completed card has no failure line', () => {
    renderCard(
      {
        type: 'toolRequest',
        id: 't7',
        toolCall: {
          status: 'success',
          value: { name: 'developer__shell', arguments: { command: 'ls' } },
        },
      },
      {
        type: 'toolResponse',
        id: 't7',
        toolResult: {
          status: 'success',
          value: { content: [{ type: 'text', text: 'notes' }], isError: false },
        },
      }
    );
    expect(screen.getByText('Completed')).toBeInTheDocument();
    expect(screen.queryByTestId('tool-call-failure')).not.toBeInTheDocument();
  });

  it('an error result (isError) says its text too', () => {
    renderCard(
      {
        type: 'toolRequest',
        id: 't6',
        toolCall: {
          status: 'success',
          value: { name: 'developer__shell', arguments: { command: 'ls data/' } },
        },
      },
      {
        type: 'toolResponse',
        id: 't6',
        toolResult: {
          status: 'success',
          value: {
            content: [{ type: 'text', text: 'ls: data/: No such file or directory' }],
            isError: true,
          },
        },
      }
    );
    expect(screen.getByTestId('tool-call-failure')).toHaveTextContent(
      'ls: data/: No such file or directory'
    );
  });
});
