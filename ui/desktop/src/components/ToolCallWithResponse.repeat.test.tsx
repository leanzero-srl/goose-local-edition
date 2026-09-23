import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import ToolCallWithResponse from './ToolCallWithResponse';
import type { ToolRequestMessageContent, ToolResponseMessageContent } from '../types/message';

const request: ToolRequestMessageContent = {
  type: 'toolRequest',
  id: 'call_1',
  toolCall: {
    status: 'success',
    value: { name: 'developer__shell', arguments: { command: 'ls -la' } },
  },
};

function response(repeat?: string, failed = false): ToolResponseMessageContent {
  return {
    type: 'toolResponse',
    id: 'call_1',
    toolResult: failed
      ? { status: 'error', error: 'Not run' }
      : {
          status: 'success',
          value: { content: [{ type: 'text', text: 'total 8' }], isError: false },
        },
    ...(repeat ? { metadata: { repeat } } : {}),
  };
}

function renderCall(toolResponse: ToolResponseMessageContent) {
  render(
    <ToolCallWithResponse
      isCancelledMessage={false}
      toolRequest={request}
      toolResponse={toolResponse}
      isPendingApproval={false}
    />,
    { wrapper: IntlTestWrapper }
  );
}

describe('ToolCallWithResponse repeat guard line', () => {
  it('shows a declined repeat as Skipped with the plain reason, not as Failed', () => {
    renderCall(response('skipped', true));
    expect(
      screen.getByText('Skipped — identical to the previous call, same output')
    ).toBeInTheDocument();
    expect(screen.getByText('Skipped')).toBeInTheDocument();
    expect(screen.queryByText('Failed')).not.toBeInTheDocument();
  });

  it('shows a noted repeat without hiding its result', () => {
    renderCall(response('same_output'));
    expect(
      screen.getByText('Same call and same output as the previous one — the model was told')
    ).toBeInTheDocument();
    expect(screen.getByText('Completed')).toBeInTheDocument();
  });

  it('renders no repeat line for an ordinary call', () => {
    renderCall(response());
    expect(screen.queryByText(/identical to the previous call/)).not.toBeInTheDocument();
    expect(screen.queryByText(/same output as the previous one/)).not.toBeInTheDocument();
  });
});
