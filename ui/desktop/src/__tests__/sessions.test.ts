import { describe, it, expect } from 'vitest';
import {
  displaySessionListName,
  getSessionDisplayName,
  shouldShowNewChatTitle,
} from '../sessions';
import { MAX_RECENT_SESSIONS, prependUnique } from '../hooks/useNavigationSessions';
import type { SessionListItem } from '../acp/sessions';
import type { Session } from '../types/session';

// Helper to build a minimal Session object for testing.
function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'untitled',
    message_count: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    working_dir: '/tmp',
    extension_data: { active: [], installed: [] },
    ...overrides,
  };
}

function makeListItem(overrides: Partial<SessionListItem> = {}): SessionListItem {
  return {
    id: 'sess-1',
    name: 'untitled',
    workingDir: '/tmp',
    updatedAt: new Date().toISOString(),
    messageCount: 0,
    createdAt: new Date().toISOString(),
    ...overrides,
  };
}

describe('shouldShowNewChatTitle — name-based, never message_count (pass E)', () => {
  it("returns true for the engine's stored placeholder name", () => {
    const session = makeSession({ name: 'New Chat', user_set_name: false });
    expect(shouldShowNewChatTitle(session)).toBe(true);
  });

  it('returns true for an empty or whitespace name', () => {
    expect(shouldShowNewChatTitle(makeSession({ name: '' }))).toBe(true);
    expect(shouldShowNewChatTitle(makeSession({ name: '   ' }))).toBe(true);
  });

  it('returns true for the default title itself', () => {
    expect(shouldShowNewChatTitle(makeSession({ name: 'New Session' }))).toBe(true);
  });

  // The backend auto-names after the first turns via session_info_update; the renderer's
  // message_count is NOT maintained while streaming, so the generated name must win even
  // when the stale metadata still says message_count === 0.
  it('returns false for a backend-generated name even with message_count 0', () => {
    const session = makeSession({
      name: 'MLX confirmation',
      message_count: 0,
      user_set_name: false,
    });
    expect(shouldShowNewChatTitle(session)).toBe(false);
  });

  it('returns false when the user has set a custom name', () => {
    const session = makeSession({ name: 'My work', user_set_name: true });
    expect(shouldShowNewChatTitle(session)).toBe(false);
  });

  it('returns false when the session has a recipe', () => {
    const session = makeSession({
      name: 'New Chat',
      user_set_name: false,
      recipe: { title: 'Recipe', steps: [] } as unknown as Session['recipe'],
    });
    expect(shouldShowNewChatTitle(session)).toBe(false);
  });
});

describe('getSessionDisplayName titles a fresh session "New Session"', () => {
  it('normalizes the engine placeholder to the session title', () => {
    const session = makeSession({ name: 'New Chat', user_set_name: false });
    expect(getSessionDisplayName(session)).toBe('New Session');
  });

  it('shows the backend-generated name once the session is named', () => {
    const session = makeSession({
      name: 'MLX confirmation',
      message_count: 0,
      user_set_name: false,
    });
    expect(getSessionDisplayName(session)).toBe('MLX confirmation');
  });
});

describe('displaySessionListName — list rows never say "New Chat"', () => {
  it('normalizes the engine placeholder and emptiness to "New Session"', () => {
    expect(displaySessionListName('New Chat')).toBe('New Session');
    expect(displaySessionListName('')).toBe('New Session');
    expect(displaySessionListName(undefined)).toBe('New Session');
  });

  it('keeps a real name untouched', () => {
    expect(displaySessionListName('MLX confirmation')).toBe('MLX confirmation');
  });
});

describe('getSessionDisplayName (fix for #8865)', () => {
  it('returns the user-set name for a recipe session that has been renamed', () => {
    const session = makeSession({
      name: 'My Renamed Chat',
      user_set_name: true,
      message_count: 2,
      recipe: { title: 'Some Recipe' } as unknown as Session['recipe'],
    });
    expect(getSessionDisplayName(session)).toBe('My Renamed Chat');
  });

  it('falls back to the recipe title when the user has not renamed', () => {
    const session = makeSession({
      name: 'auto-generated',
      user_set_name: false,
      message_count: 2,
      recipe: { title: 'Some Recipe' } as unknown as Session['recipe'],
    });
    expect(getSessionDisplayName(session)).toBe('Some Recipe');
  });
});

describe('prependUnique', () => {
  it('prepends a new session to the front', () => {
    const prev = [makeListItem({ id: 'a' })];
    const result = prependUnique(prev, makeListItem({ id: 'b' }));
    expect(result.map((s) => s.id)).toEqual(['b', 'a']);
  });

  it('returns the same reference when the session is already present', () => {
    const prev = [makeListItem({ id: 'a' }), makeListItem({ id: 'b' })];
    const result = prependUnique(prev, makeListItem({ id: 'a' }));
    expect(result).toBe(prev);
  });

  // Asserts against the EXPORTED cap, never a copy of the number. The previous version hardcoded 25;
  // when the render cap was deliberately raised to 200 the test kept asserting 25 and had been failing
  // ever since — a stale test that reported a healthy change as a regression.
  it('caps the rendered list at MAX_RECENT_SESSIONS and puts the newest first', () => {
    const full = Array.from({ length: MAX_RECENT_SESSIONS }, (_, i) =>
      makeListItem({ id: `s-${i}` })
    );
    const result = prependUnique(full, makeListItem({ id: 'new' }));
    expect(result).toHaveLength(MAX_RECENT_SESSIONS);
    expect(result[0].id).toBe('new');
    // The OLDEST is the one dropped, not an arbitrary entry.
    expect(result.some((s) => s.id === `s-${MAX_RECENT_SESSIONS - 1}`)).toBe(false);
    expect(result.some((s) => s.id === 's-0')).toBe(true);
  });

  it('does not truncate a list that is under the cap', () => {
    const few = Array.from({ length: 3 }, (_, i) => makeListItem({ id: `s-${i}` }));
    expect(prependUnique(few, makeListItem({ id: 'new' }))).toHaveLength(4);
  });
});
