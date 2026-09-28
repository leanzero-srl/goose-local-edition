import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import type { NoteChatDto, NoteDraftDto } from '@aaif/goose-sdk';

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

import NoteDraftTray from './NoteDraftCard';
import { resetChatNotesForTests, seedChatNotesForTests } from './notesStore';

const BILLING: NoteChatDto = {
  sessionId: 'chat-b',
  name: 'Migrate billing',
  workingDir: '/Users/me/billing',
  folder: '~/billing',
  live: 'working',
  lastActiveAt: '2026-09-28T14:02:00Z',
};

function draft(over: Partial<NoteDraftDto> = {}): NoteDraftDto {
  return {
    id: 'nt_1',
    toQuery: 'the billing chat',
    text: 'tenant_id is the new column',
    createdAt: '2026-09-28T14:00:00Z',
    resolution: 'title_words',
    target: BILLING,
    candidates: [],
    status: 'draft',
    ...over,
  };
}

/** goosed's list is what the test seeds: the tray's own first read returns the same. */
function renderTray(drafts: NoteDraftDto[]) {
  acp.notesList.mockResolvedValue({ drafts, inbox: [] });
  seedChatNotesForTests('chat-a', { list: { drafts, inbox: [] }, error: null });
  render(
    <IntlProvider locale="en" messages={{}}>
      <NoteDraftTray sessionId="chat-a" />
    </IntlProvider>
  );
}

describe('NoteDraftCard — the person decides where a note goes, and whether it goes at all', () => {
  beforeEach(() => {
    for (const fn of Object.values(acp)) fn.mockReset();
    acp.notesList.mockImplementation(async () => ({ drafts: [draft()], inbox: [] }));
    acp.notesSend.mockResolvedValue(draft({ status: 'sent' }));
    acp.notesDraft.mockResolvedValue(draft());
  });
  afterEach(() => {
    resetChatNotesForTests();
    try {
      window.localStorage.clear();
    } catch {
      // no storage in this environment
    }
  });

  it('names the target, where it is and what goose is doing there, in a solid violet card', () => {
    renderTray([draft()]);
    const card = screen.getByTestId('note-draft-card');
    expect(within(card).getByText('Note to another chat')).toBeTruthy();
    expect(screen.getByTestId('note-target').textContent).toBe(
      '"Migrate billing"· ~/billing · goose is working there'
    );
    expect(card.className).toContain('border-2');
    expect(card.className).toContain('border-lz-secondary');
    expect(card.className).not.toMatch(/border-l-/);
    for (const button of ['Steer it now', 'Leave it there', 'Not this chat', 'Cancel']) {
      expect(within(card).getByRole('button', { name: button })).toBeTruthy();
    }
  });

  it('says "idle since" or "not open in any window" when goose is not working there', () => {
    renderTray([
      draft({ id: 'nt_idle', target: { ...BILLING, live: 'idle' } }),
      draft({ id: 'nt_closed', target: { ...BILLING, live: 'not_open' } }),
    ]);
    const lives = screen.getAllByTestId('note-target-live').map((node) => node.textContent);
    expect(lives[0]).toMatch(/^idle since \d{2}:\d{2}$/);
    expect(lives[1]).toBe('not open in any window');
  });

  it('sends only on the click, with the text as the person left it and the way they chose', async () => {
    renderTray([draft()]);
    expect(acp.notesSend).not.toHaveBeenCalled();
    fireEvent.change(screen.getByTestId('note-text'), {
      target: { value: 'tenant_id replaces org_id' },
    });
    fireEvent.click(screen.getByTestId('note-steer-now'));
    await waitFor(() =>
      expect(acp.notesSend).toHaveBeenCalledWith(
        'chat-a',
        'nt_1',
        'tenant_id replaces org_id',
        'steer_now'
      )
    );
    fireEvent.click(screen.getByTestId('note-leave-there'));
    await waitFor(() =>
      expect(acp.notesSend).toHaveBeenLastCalledWith(
        'chat-a',
        'nt_1',
        'tenant_id replaces org_id',
        'leave_there'
      )
    );
  });

  it('"Not this chat" lists the other chats — buttons, never a native select — and repoints it', async () => {
    acp.notesTargets.mockResolvedValue([
      BILLING,
      { ...BILLING, sessionId: 'chat-c', name: 'Fix the build', folder: '~/ci', live: 'idle' },
    ]);
    renderTray([draft()]);
    fireEvent.click(screen.getByTestId('note-not-this-chat'));
    const picker = await screen.findByTestId('note-picker');
    expect(picker.querySelector('select')).toBeNull();
    await waitFor(() => expect(screen.getAllByTestId('note-candidate')).toHaveLength(2));
    fireEvent.change(screen.getByTestId('note-picker-filter'), { target: { value: 'build' } });
    const rows = screen.getAllByTestId('note-candidate');
    expect(rows).toHaveLength(1);
    fireEvent.click(rows[0]);
    await waitFor(() =>
      expect(acp.notesDraft).toHaveBeenCalledWith('chat-a', 'nt_1', {
        kind: 'retarget',
        toSessionId: 'chat-c',
      })
    );
  });

  it('an ambiguous draft lists the equal matches and cannot be sent until one is picked', async () => {
    renderTray([
      draft({
        target: undefined,
        resolution: 'ambiguous',
        candidates: [BILLING, { ...BILLING, sessionId: 'chat-d', name: 'Migrate billing' }],
      }),
    ]);
    expect(screen.getByTestId('note-ambiguous').textContent).toBe(
      'Several chats match "the billing chat". Which one?'
    );
    expect(screen.getAllByTestId('note-candidate')).toHaveLength(2);
    expect((screen.getByTestId('note-steer-now') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('note-leave-there') as HTMLButtonElement).disabled).toBe(true);
  });

  it('no matching chat is said plainly, with every chat to pick from', async () => {
    acp.notesTargets.mockResolvedValue([BILLING]);
    renderTray([draft({ target: undefined, resolution: 'no_match', toQuery: 'payments' })]);
    expect(screen.getByTestId('note-no-match').textContent).toBe(
      'No chat matches "payments". Pick the chat this note is for.'
    );
    await waitFor(() => expect(screen.getAllByTestId('note-candidate')).toHaveLength(1));
  });

  it('Cancel sends nothing', async () => {
    renderTray([draft()]);
    fireEvent.click(screen.getByTestId('note-cancel'));
    await waitFor(() =>
      expect(acp.notesDraft).toHaveBeenCalledWith('chat-a', 'nt_1', { kind: 'cancel' })
    );
    expect(acp.notesSend).not.toHaveBeenCalled();
  });

  it('a sent note is one line that follows what became of it there', () => {
    renderTray([
      draft({ id: 'nt_w', status: 'sent', outcome: { state: 'waiting' } }),
      draft({
        id: 'nt_r',
        status: 'sent',
        outcome: { state: 'delivered', at: '2026-09-28T14:05:00Z', how: 'steered' },
      }),
      draft({ id: 'nt_d', status: 'sent', outcome: { state: 'dismissed' } }),
    ]);
    const lines = screen.getAllByTestId('note-sent-line').map((line) => line.textContent);
    expect(lines[0]).toBe('Note sent to "Migrate billing" · waiting there');
    expect(lines[1]).toMatch(/^Note sent to "Migrate billing" · read in its turn at \d{2}:\d{2}$/);
    expect(lines[2]).toBe('Note sent to "Migrate billing" · dismissed there');
    expect(screen.queryByTestId('note-draft-card')).toBeNull();
  });
});
