import { describe, expect, it } from 'vitest';
import { defaultKeyboardShortcuts, getKeyboardShortcuts, type Settings } from './settings';

/**
 * Pass D (owner) drift guard: sessions start from projects only, so the shortcut set carries NO
 * `newChat` (Cmd+T fresh chat) and NO `quickLauncher` (floating input that created a project-less
 * session). Cmd+N remains, but as "New Window" — it opens a window on the project landing and
 * creates no session. A key reappearing here is a decision, not an accident.
 */
describe('keyboard shortcut set (sessions start from projects only)', () => {
  it('contains exactly the surviving shortcuts', () => {
    expect(Object.keys(defaultKeyboardShortcuts).sort()).toEqual(
      [
        'alwaysOnTop',
        'find',
        'findNext',
        'findPrevious',
        'focusWindow',
        'newChatWindow',
        'openDirectory',
        'settings',
        'toggleNavigation',
      ].sort()
    );
  });

  it('keeps Cmd+N as the window spawner and has no new-chat accelerator', () => {
    expect(defaultKeyboardShortcuts.newChatWindow).toBe('CommandOrControl+N');
    expect('newChat' in defaultKeyboardShortcuts).toBe(false);
    expect('quickLauncher' in defaultKeyboardShortcuts).toBe(false);
  });

  it('the legacy globalShortcut migration seeds focusWindow only — no launcher derivation', () => {
    const legacy = {
      globalShortcut: 'CommandOrControl+Alt+G',
      keyboardShortcuts: undefined,
    } as unknown as Settings;
    const shortcuts = getKeyboardShortcuts(legacy);
    expect(shortcuts.focusWindow).toBe('CommandOrControl+Alt+G');
    expect('quickLauncher' in shortcuts).toBe(false);
  });
});
