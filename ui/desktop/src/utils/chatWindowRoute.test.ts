import { describe, expect, it } from 'vitest';
import { chatWindowRoute, NEW_SESSION_PARAM } from './chatWindowRoute';

/** Q-491: the route main.ts createChat opens a new window on. */
describe('chatWindowRoute', () => {
  it('asks the pair route for a new session when a window is opened with only a folder', () => {
    // window.electron.createChatWindow({ dir, viewType: 'pair' }) and handleFileOpen.
    expect(chatWindowRoute({ viewType: 'pair' })).toBe(`/pair?${NEW_SESSION_PARAM}=1`);
  });

  it('leaves the launcher query to make its own session (it arrives as set-initial-message)', () => {
    expect(chatWindowRoute({ initialMessage: 'fix the build' })).toBe('/pair?');
    expect(chatWindowRoute({ viewType: 'pair', initialMessage: 'fix the build' })).toBe('/pair?');
  });

  it('resumes a named session instead of making one', () => {
    expect(chatWindowRoute({ viewType: 'pair', resumeSessionId: 's 1' })).toBe(
      '/pair?resumeSessionId=s+1'
    );
    expect(chatWindowRoute({ resumeSessionId: 's1' })).toBe('/pair?resumeSessionId=s1');
  });

  it('leaves a recipe window to the recipe it carries', () => {
    expect(chatWindowRoute({ viewType: 'pair', recipeId: 'r1' })).toBe('/pair?');
    expect(chatWindowRoute({ recipeDeeplink: 'abc' })).toBe('/pair?');
  });

  it('opens every other window where it did before', () => {
    expect(chatWindowRoute({})).toBe('/?');
    expect(chatWindowRoute({ viewType: 'settings' })).toBe('/settings?');
    expect(chatWindowRoute({ viewType: 'nope' })).toBe('/?');
  });
});
