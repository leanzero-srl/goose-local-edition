import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import type { InboxNoteDto } from '@aaif/goose-sdk';

const acp = vi.hoisted(() => ({
  notesList: vi.fn(),
  notesTargets: vi.fn(),
  notesSend: vi.fn(),
  notesDraft: vi.fn(),
  notesInbox: vi.fn(),
  notesShowing: vi.fn(),
  crossNoteMeta: vi.fn(),
}));
vi.mock('../../acp/notes', () => acp);

import NoteInboxTray, { type Give } from './NoteInboxTray';
import { ChatState } from '../../types/chatState';
import { resetChatNotesForTests, seedChatNotesForTests } from './notesStore';

const NOTE: InboxNoteDto = {
  id: 'nt_1',
  fromSessionId: 'chat-a',
  fromName: 'Explore split mesh',
  fromWorkingDir: '/Users/me/p',
  fromFolder: '~/p',
  text: 'tenant_id is the new column',
  sentAt: '2026-09-28T14:02:00Z',
  delivery: 'leave_there',
  status: 'waiting',
  offerWhenIdle: false,
  messageId: 'crossnote_nt_1',
  prompt: 'Note from your other chat "Explore split mesh" (~/p) …',
};

/** goosed's list is what the test seeds: the tray's own first read returns the same. */
function renderTray(note: InboxNoteDto, chatState: ChatState, give?: Give) {
  acp.notesList.mockResolvedValue({ drafts: [], inbox: [note] });
  seedChatNotesForTests('chat-b', { list: { drafts: [], inbox: [note] }, error: null });
  render(
    <IntlProvider locale="en" messages={{}}>
      <NoteInboxTray sessionId="chat-b" chatState={chatState} give={give} />
    </IntlProvider>
  );
}

describe('NoteInboxTray — a note waits for the person here; nothing reaches goose without a click', () => {
  beforeEach(() => {
    for (const fn of Object.values(acp)) fn.mockReset();
    acp.notesList.mockImplementation(async () => ({ drafts: [], inbox: [NOTE] }));
    acp.notesInbox.mockResolvedValue(NOTE);
  });
  afterEach(() => resetChatNotesForTests());

  it('says whom it is from, when, and that goose has not read it — in a solid violet card', () => {
    renderTray(NOTE, ChatState.Idle);
    const card = screen.getByTestId('note-inbox-card');
    expect(within(card).getByText('Note from "Explore split mesh"')).toBeTruthy();
    expect(within(card).getByText(/^· \d{2}:\d{2}$/)).toBeTruthy();
    expect(
      within(card).getByText('Sent from your other chat. goose has not read it yet.')
    ).toBeTruthy();
    expect(screen.getByTestId('note-inbox-text').textContent).toBe(NOTE.text);
    expect(card.className).toContain('border-lz-secondary');
    expect(card.className).not.toMatch(/border-l-/);
  });

  it('idle: Give it to goose now · Add to my next message · Dismiss', async () => {
    const give = vi.fn<Give>(async () => ({ kind: 'submitted' }));
    renderTray(NOTE, ChatState.Idle, give);
    const names = within(screen.getByTestId('note-inbox-card'))
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(names).toEqual(['Give it to goose now', 'Add to my next message', 'Dismiss']);
    fireEvent.click(screen.getByTestId('note-give-now'));
    await waitFor(() => expect(give).toHaveBeenCalledWith('chat-b', NOTE));
    fireEvent.click(screen.getByTestId('note-add-to-next'));
    await waitFor(() =>
      expect(acp.notesInbox).toHaveBeenCalledWith('chat-b', 'nt_1', 'add_to_next_message')
    );
  });

  it('busy: Steer this turn · After this turn · Dismiss', async () => {
    renderTray(NOTE, ChatState.Streaming);
    const names = within(screen.getByTestId('note-inbox-card'))
      .getAllByRole('button')
      .map((button) => button.textContent);
    expect(names).toEqual(['Steer this turn', 'After this turn', 'Dismiss']);
    fireEvent.click(screen.getByTestId('note-steer-this-turn'));
    await waitFor(() =>
      expect(acp.notesInbox).toHaveBeenCalledWith('chat-b', 'nt_1', 'steer_this_turn')
    );
    fireEvent.click(screen.getByTestId('note-dismiss'));
    await waitFor(() => expect(acp.notesInbox).toHaveBeenCalledWith('chat-b', 'nt_1', 'dismiss'));
  });

  it("a turn refused by goose says goose's words where the person clicked", async () => {
    const give = vi.fn<Give>(async () => ({ kind: 'refused', error: 'note nt_1 is not waiting' }));
    renderTray(NOTE, ChatState.Idle, give);
    fireEvent.click(screen.getByTestId('note-give-now'));
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Could not do that: note nt_1 is not waiting'
    );
  });

  it('a note steered into the turn says so, and a delivered one leaves the tray', () => {
    renderTray({ ...NOTE, status: 'steering' }, ChatState.Streaming);
    expect(screen.getByTestId('note-inbox-status').textContent).toBe(
      'Steered into this turn: goose reads it between tool calls.'
    );
    expect(screen.queryByTestId('note-dismiss')).toBeNull();
    const delivered = { drafts: [], inbox: [{ ...NOTE, status: 'delivered' as const }] };
    acp.notesList.mockResolvedValue(delivered);
    act(() => seedChatNotesForTests('chat-b', { list: delivered, error: null }));
    expect(screen.queryByTestId('note-inbox-tray')).toBeNull();
  });
});
