import { afterEach, describe, expect, it } from 'vitest';
import { getGlobalSkillsDir } from './globalSkillsDir';

const withAppConfig = (config: Record<string, unknown> | undefined) => {
  Object.defineProperty(window, 'appConfig', {
    configurable: true,
    value: config && { get: (key: string) => config[key], getAll: () => config },
  });
};

afterEach(() => withAppConfig(undefined));

describe('the renderer takes the global skills dir from main (Q-188)', () => {
  it('returns the dir main resolved — the root under GOOSE_PATH_ROOT', () => {
    withAppConfig({ GOOSE_GLOBAL_SKILLS_DIR: '/tmp/profile/.agents/skills' });
    expect(getGlobalSkillsDir()).toBe('/tmp/profile/.agents/skills');
  });

  it("never guesses the owner's folder when main did not say", () => {
    withAppConfig({});
    expect(() => getGlobalSkillsDir()).toThrow(/GOOSE_GLOBAL_SKILLS_DIR/);
  });
});
