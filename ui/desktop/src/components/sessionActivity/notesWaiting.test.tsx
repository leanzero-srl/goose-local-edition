import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { IntlTestWrapper } from '../../i18n/test-utils';

vi.mock('../../acp/needsYou', () => ({
  acpSessionActivity: vi.fn(async () => ({ running: [], needsYou: [], failed: [] })),
  acpResolveNeedsYou: vi.fn(),
}));

import ActiveNowSection from './ActiveNowSection';
import SessionActivityIndicator from './SessionActivityIndicator';
import { SessionActivityMarker } from './ActivityPills';
import {
  activeSessions,
  activityOf,
  getSessionActivitySnapshot,
  isActive,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivityStore';

const WAITING = {
  sessionId: 'chat-b',
  sessionName: 'Migrate billing',
  workingDir: '/Users/me/billing',
  count: 1,
  fromName: 'Explore split mesh',
};

function mount(node: React.ReactNode) {
  return render(
    <MemoryRouter>
      <IntlTestWrapper>{node}</IntlTestWrapper>
    </MemoryRouter>
  );
}

describe('a chat with a note waiting leads the lists with a solid Note chip (Q-358)', () => {
  afterEach(() => resetSessionActivityForTests());

  it('counts as active: pinned with the running and waiting chats', () => {
    seedSessionActivityForTests({ notesWaiting: [WAITING] });
    const state = getSessionActivitySnapshot();
    const activity = activityOf(state, 'chat-b');
    expect(activity.notesWaiting).toBe(1);
    expect(activity.noteFrom).toBe('Explore split mesh');
    expect(isActive(activity)).toBe(true);
    expect(activeSessions(state).map((row) => [row.sessionId, row.notesWaiting])).toEqual([
      ['chat-b', 1],
    ]);
  });

  it('every session row shows the solid Note chip, even when the chat is otherwise idle', () => {
    seedSessionActivityForTests({ notesWaiting: [WAITING] });
    mount(<SessionActivityMarker sessionId="chat-b" idle={<span>27m ago</span>} />);
    const chip = screen.getByTestId('session-note-pill');
    expect(chip.textContent).toBe('Note');
    expect(chip.className).toContain('bg-lz-secondary');
    expect(chip.getAttribute('aria-label')).toBe(
      'A note from "Explore split mesh" is waiting in this chat'
    );
    expect(screen.queryByText('27m ago')).toBeNull();
  });

  it('Active now says "1 note waiting", and so does the top bar', () => {
    seedSessionActivityForTests({ notesWaiting: [WAITING] });
    mount(
      <>
        <ActiveNowSection />
        <SessionActivityIndicator />
      </>
    );
    const row = screen.getByTestId('active-now-row-chat-b');
    expect(row.getAttribute('data-state')).toBe('note');
    expect(within(row).getByText('billing · 1 note waiting')).toBeTruthy();
    expect(screen.getByTestId('indicator-notes-words').textContent).toBe('1 note waiting');
  });
});
