import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import ToolCallWithResponse from './ToolCallWithResponse';
import { REVEAL_TOOL_CALL_EVENT } from './changes/fileDiff';
import { KICKOFF_EDIT, diffResponse, editRequest, failedResponse } from './changes/fixtures';
import type { ToolResponseMessageContent } from '../types/message';

const PATH = '/w/notes/kickoff.md';

function renderCard(toolResponse: ToolResponseMessageContent) {
  render(
    <ToolCallWithResponse
      isCancelledMessage={false}
      toolRequest={editRequest('call_1', PATH)}
      toolResponse={toolResponse}
      isPendingApproval={false}
    />,
    { wrapper: IntlTestWrapper }
  );
}

const edited = diffResponse(
  'call_1',
  { path: PATH, unified: KICKOFF_EDIT, added: 4, removed: 1 },
  `Edited ${PATH} (1 lines -> 4 lines)`
);

describe('ToolCallWithResponse — what an edit changed (Q-189)', () => {
  it('carries +N −M on the card face, before it is opened', () => {
    renderCard(edited);
    const card = screen.getByTestId('tool-call-card');
    expect(card).toHaveAttribute('id', 'tool-call-call_1');
    expect(within(card).getByTestId('diff-counts')).toHaveTextContent('+4−1');
    expect(screen.getByText(`Edit · ${PATH}`)).toBeInTheDocument();
  });

  it('opens to the diff itself: path, counts, the removed and the added lines', () => {
    renderCard(edited);
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`Edit · ${PATH}`) }));
    const section = screen.getByTestId('tool-call-diff');
    expect(section).toHaveTextContent('What changed');
    expect(section).toHaveTextContent(PATH);
    expect(within(section).getAllByTestId('diff-line-del')).toHaveLength(1);
    expect(within(section).getAllByTestId('diff-line-add')).toHaveLength(4);
    expect(within(section).getByText('- owners')).toHaveClass('text-lz-ok');
  });

  it('draws no diff for a failed edit — only the failure', () => {
    renderCard(failedResponse('call_1', 'No match found for the specified text.'));
    expect(screen.queryByTestId('diff-counts')).not.toBeInTheDocument();
    expect(screen.queryByTestId('tool-call-diff')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-call-failure')).toHaveTextContent('No match found');
  });

  it('opens when the Changes rail asks for it', () => {
    renderCard(edited);
    expect(screen.queryByTestId('tool-call-diff')).not.toBeInTheDocument();
    act(() => {
      window.dispatchEvent(
        new CustomEvent(REVEAL_TOOL_CALL_EVENT, { detail: { toolCallId: 'call_1' } })
      );
    });
    expect(screen.getByTestId('tool-call-diff')).toBeInTheDocument();
    expect(screen.getByTestId('tool-call-card')).toHaveClass('ring-2');
  });
});
