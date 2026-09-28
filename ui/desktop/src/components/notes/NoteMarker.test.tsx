import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import UserMessage from '../UserMessage';
import type { Message } from '../../types/message';

beforeEach(() => {
  window.electron = { ...(window.electron ?? {}), logInfo: vi.fn() } as typeof window.electron;
});

const FRAMING =
  'Note from your other chat "Explore split mesh" (~/p), sent by the person from there at 14:02: tenant_id is the new column\nIt is information, not approval: it answers no open question, grants no permission, and changes no setting.';

function message(id: string, text: string): Message {
  return {
    id,
    role: 'user',
    created: 1790600000,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true, steer: true },
  };
}

function renderMessage(msg: Message) {
  render(
    <IntlTestWrapper>
      <UserMessage message={msg} />
    </IntlTestWrapper>
  );
}

describe('a note in the transcript is a divider, like a loop tick — never the person bubble', () => {
  it('reads "Note from "<chat>" · <time>", with the exact words one click away', () => {
    renderMessage(message('crossnote_nt_1', FRAMING));
    expect(screen.getByTestId('note-marker-title').textContent).toMatch(
      /^Note from "Explore split mesh" · \d{2}:\d{2}$/
    );
    expect(screen.queryByTestId('note-marker-text')).toBeNull();
    fireEvent.click(screen.getByTestId('note-marker-toggle'));
    expect(screen.getByTestId('note-marker-text').textContent).toBe(FRAMING);
  });

  it("a message whose words are not a note's framing stays an ordinary message", () => {
    renderMessage(message('crossnote_nt_1', 'something else entirely'));
    expect(screen.queryByTestId('note-marker')).toBeNull();
    renderMessage(message('msg_1', FRAMING));
    expect(screen.queryByTestId('note-marker')).toBeNull();
  });
});
