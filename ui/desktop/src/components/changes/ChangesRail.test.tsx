import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SessionRail from '../session-rail/SessionRail';
import { REVEAL_TOOL_CALL_EVENT } from './fileDiff';
import {
  KICKOFF_EDIT,
  bigCreate,
  diffResponse,
  editRequest,
  failedResponse,
  message,
  unified,
} from './fixtures';
import type { Message } from '../../types/message';

/** Q-190's rail, now the Changes tab of the session rail (Q-228 L5) in a chat with no loop. */
function renderRail(messages: Message[]) {
  return render(
    <SessionRail
      sessionId="s-changes"
      messages={messages}
      loop={{ kind: 'none' }}
      control={async () => ({ kind: 'failed', error: 'unused' })}
    />,
    { wrapper: IntlTestWrapper }
  );
}

const ownerEdit = [
  message('assistant', [editRequest('c1', '/w/notes/kickoff.md')]),
  message('user', [
    diffResponse('c1', {
      path: '/w/notes/kickoff.md',
      unified: KICKOFF_EDIT,
      added: 4,
      removed: 1,
    }),
  ]),
];

describe('ChangesRail', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it('renders nothing when the chat changed no file — a failed edit included', () => {
    const { container } = renderRail([
      message('user', [{ type: 'text', text: 'rename the heading' }]),
      message('assistant', [editRequest('c1', '/w/a.md')]),
      message('user', [failedResponse('c1', 'No match found for the specified text.')]),
    ]);
    expect(container).toBeEmptyDOMElement();
  });

  it('sits collapsed as one pill saying how many files and lines', () => {
    renderRail(ownerEdit);
    const pill = screen.getByTestId('changes-rail-pill');
    expect(pill).toHaveTextContent('1 file+4−1');
    expect(pill).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByTestId('changes-rail-panel')).not.toBeInTheDocument();
  });

  it('opens over the chat with the one file already open to its hunks, and closes back', () => {
    renderRail(ownerEdit);
    fireEvent.click(screen.getByTestId('changes-rail-pill'));
    const panel = screen.getByTestId('changes-rail-panel');
    expect(within(panel).getByText('kickoff.md')).toBeInTheDocument();
    expect(within(panel).getByText('/w/notes')).toBeInTheDocument();
    expect(within(panel).getAllByTestId('diff-line-add')).toHaveLength(4);
    expect(within(panel).getByText('Agenda: TBD')).toBeInTheDocument();
    fireEvent.keyDown(panel, { key: 'Escape' });
    expect(screen.queryByTestId('changes-rail-panel')).not.toBeInTheDocument();
    expect(screen.getByTestId('changes-rail-pill')).toBeInTheDocument();
  });

  it('lists many files as a closed accordion; a file opens to each of its changes', () => {
    renderRail([
      message('assistant', [editRequest('c1', '/w/a.md'), editRequest('c2', '/w/src/b.ts')]),
      message('user', [
        diffResponse('c1', { path: '/w/a.md', unified: KICKOFF_EDIT, added: 4, removed: 1 }),
        diffResponse('c2', {
          path: '/w/src/b.ts',
          unified: bigCreate('/w/src/b.ts', 2),
          added: 2,
          removed: 0,
          before: 'none',
        }),
      ]),
      message('assistant', [editRequest('c3', '/w/a.md')]),
      message('user', [
        diffResponse('c3', {
          path: '/w/a.md',
          unified: unified('/w/a.md', '@@ -9 +9 @@\n-x\n+y\n'),
          added: 1,
          removed: 1,
        }),
      ]),
    ]);
    expect(screen.getByTestId('changes-rail-pill')).toHaveTextContent('2 files+7−2');
    fireEvent.click(screen.getByTestId('changes-rail-pill'));
    const files = screen.getAllByTestId('changes-rail-file');
    expect(files).toHaveLength(2);
    expect(files[0]).toHaveTextContent('a.md');
    expect(files[0]).toHaveTextContent('+5−2');
    expect(screen.queryAllByTestId('diff-view')).toHaveLength(0);

    fireEvent.click(within(files[0]).getByRole('button', { name: /a\.md/ }));
    const edits = within(files[0]).getAllByTestId('changes-rail-edit');
    expect(edits).toHaveLength(2);
    expect(edits[0]).toHaveTextContent('Change 1 of 2');
    expect(edits[1]).toHaveTextContent('Change 2 of 2');
    expect(within(files[1]).queryByTestId('diff-view')).not.toBeInTheDocument();
  });

  it('takes the person to the call in the chat from a hunk', () => {
    const card = document.createElement('div');
    card.id = 'tool-call-c1';
    card.scrollIntoView = vi.fn();
    document.body.appendChild(card);
    const revealed: string[] = [];
    const onReveal = (e: Event) =>
      revealed.push((e as CustomEvent<{ toolCallId: string }>).detail.toolCallId);
    window.addEventListener(REVEAL_TOOL_CALL_EVENT, onReveal);

    renderRail(ownerEdit);
    fireEvent.click(screen.getByTestId('changes-rail-pill'));
    fireEvent.click(screen.getByTestId('diff-hunk'));

    expect(card.scrollIntoView).toHaveBeenCalled();
    expect(revealed).toEqual(['c1']);
    window.removeEventListener(REVEAL_TOOL_CALL_EVENT, onReveal);
    card.remove();
  });
});
