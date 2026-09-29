import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import SessionListView from './SessionListView';
import { resetSessionActivityForTests } from '../sessionActivity/sessionActivityStore';

vi.mock('../../acp/needsYou', () => ({
  acpSessionActivity: vi.fn(async () => ({ running: [], needsYou: [], failed: [] })),
  acpResolveNeedsYou: vi.fn(),
}));

const sessionsAcp = vi.hoisted(() => ({ acpListSessions: vi.fn() }));
vi.mock('../../acp/sessions', () => ({
  acpListSessions: sessionsAcp.acpListSessions,
  acpDeleteSession: vi.fn(),
  acpExportSession: vi.fn(),
  acpForkSession: vi.fn(),
  acpImportSession: vi.fn(),
  acpRenameSession: vi.fn(),
  acpShareSessionNostr: vi.fn(),
}));

const SESSION = {
  id: 'kickoff-1',
  name: 'Kickoff',
  workingDir: '/Users/me/api',
  messageCount: 3,
  createdAt: '2026-09-26T10:00:00Z',
  updatedAt: '2026-09-26T10:00:00Z',
  lastMessageAt: '2026-09-26T10:00:00Z',
};

function view(onSelectSession: (sessionId: string) => void) {
  return (
    <IntlTestWrapper>
      <SessionListView onSelectSession={onSelectSession} />
    </IntlTestWrapper>
  );
}

/** Renders the list and waits until it has settled: the card shown and the content layer faded in. */
async function renderSettled(onSelectSession: (sessionId: string) => void) {
  const utils = render(view(onSelectSession));
  await waitFor(() =>
    expect(screen.getByTestId(`session-card-${SESSION.id}`).closest('.opacity-100')).not.toBeNull()
  );
  return utils;
}

/**
 * Q-512: SessionItem was declared inside SessionListView, so every render of the list made a new
 * component type and remounted every session card — its React.memo was rebuilt with it and never
 * held. A card or button a person had under the pointer when the list re-rendered (a page of
 * sessions arriving, the activity store ticking, a dialog opening) was detached, and the click
 * landed on nothing.
 */
describe('SessionListView cards survive a re-render (Q-512)', () => {
  beforeEach(() => {
    Object.assign(window.electron, { getConfig: () => ({}) });
    sessionsAcp.acpListSessions.mockReset();
    sessionsAcp.acpListSessions.mockResolvedValue({ sessions: [SESSION], nextCursor: null });
  });
  afterEach(() => resetSessionActivityForTests());

  it('the card held across a re-render is the card on screen, and a click on it opens the session', async () => {
    const first = vi.fn();
    const { rerender } = await renderSettled(first);
    const card = screen.getByTestId(`session-card-${SESSION.id}`);

    const second = vi.fn();
    rerender(view(second));

    expect(screen.getByTestId(`session-card-${SESSION.id}`)).toBe(card);
    fireEvent.click(card);
    expect(second).toHaveBeenCalledWith(SESSION.id);
    expect(first).not.toHaveBeenCalled();
  });

  it('a Delete button held across a re-render still asks to delete that session', async () => {
    const { rerender } = await renderSettled(vi.fn());
    const button = screen.getByTitle('Delete session');

    rerender(view(vi.fn()));

    fireEvent.click(button);
    expect(
      screen.getByText(
        'Are you sure you want to delete the session "Kickoff"? This action cannot be undone.'
      )
    ).toBeInTheDocument();
  });
});
