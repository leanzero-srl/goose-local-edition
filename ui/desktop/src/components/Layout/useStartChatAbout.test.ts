import { describe, expect, it } from 'vitest';
import { WORK_ON_ITEM_EXTENSIONS, extensionsForWorkOnItem } from './useStartChatAbout';
import type { FixedExtensionEntry } from '../ConfigContext';

/** A "work on this item" chat must be able to CHANGE the item: the memory, skills, developer and
 *  extension-manager extensions ride along even when the profile has them off; a disabled
 *  extension outside that set stays off; one the profile does not carry is not invented. */
const entry = (name: string, enabled: boolean): FixedExtensionEntry =>
  ({ type: 'builtin', name, enabled, timeout: 300 }) as FixedExtensionEntry;

describe('extensionsForWorkOnItem', () => {
  it('adds the required extensions the profile has turned off and keeps the rest of the selection', () => {
    const all = [
      entry('developer', true),
      entry('memory', false),
      entry('skills', false),
      entry('extensionmanager', false),
      entry('todo', false),
      entry('analyze', true),
    ];
    const names = extensionsForWorkOnItem(all).map((c) => c.name);
    expect(names).toEqual(['developer', 'memory', 'skills', 'extensionmanager', 'analyze']);
    expect(names).not.toContain('todo');
    expect(WORK_ON_ITEM_EXTENSIONS).toEqual(['developer', 'memory', 'skills', 'extensionmanager']);
  });

  it('never invents an extension the profile does not carry, and strips the enabled flag', () => {
    const configs = extensionsForWorkOnItem([entry('developer', true)]);
    expect(configs.map((c) => c.name)).toEqual(['developer']);
    expect('enabled' in configs[0]).toBe(false);
  });
});
