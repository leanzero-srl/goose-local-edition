import { afterEach, describe, expect, it } from 'vitest';
import { getGooseConfigDir } from './gooseConfigDir';

const withAppConfig = (config: Record<string, unknown> | undefined) => {
  Object.defineProperty(window, 'appConfig', {
    configurable: true,
    value: config && { get: (key: string) => config[key], getAll: () => config },
  });
};

afterEach(() => withAppConfig(undefined));

describe('the renderer takes the goose config dir from main (Q-198)', () => {
  it('returns the dir main resolved — the root under GOOSE_PATH_ROOT', () => {
    withAppConfig({ GOOSE_CONFIG_DIR: '/tmp/profile/config' });
    expect(getGooseConfigDir()).toBe('/tmp/profile/config');
  });

  it("never guesses the owner's folder when main did not say", () => {
    withAppConfig({});
    expect(() => getGooseConfigDir()).toThrow(/GOOSE_CONFIG_DIR/);
  });
});
