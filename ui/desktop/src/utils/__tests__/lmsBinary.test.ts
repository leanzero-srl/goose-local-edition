import { beforeEach, describe, expect, it, vi } from 'vitest';
import { findLmsBinary, resetLmsResolutionForTests, resolveLmsOnce } from '../lmsBinary';

const lookup = (present: string[], platform: NodeJS.Platform = 'darwin') => ({
  home: '/Users/w',
  pathEnv: '/opt/homebrew/bin:/usr/bin',
  platform,
  exists: (p: string) => present.includes(p),
});

describe('findLmsBinary — the lookup the fleet-status handler always had', () => {
  it('prefers the LM Studio-bundled ~/.lmstudio/bin/lms', () => {
    expect(findLmsBinary(lookup(['/Users/w/.lmstudio/bin/lms', '/opt/homebrew/bin/lms']))).toBe(
      '/Users/w/.lmstudio/bin/lms'
    );
  });

  it('falls back to the first PATH entry that has it', () => {
    expect(findLmsBinary(lookup(['/usr/bin/lms']))).toBe('/usr/bin/lms');
  });

  it('answers null on a Mac with no LM Studio — nothing to spawn', () => {
    expect(findLmsBinary(lookup([]))).toBeNull();
    expect(findLmsBinary({ ...lookup([]), pathEnv: undefined })).toBeNull();
  });

  it('tries the Windows launcher names on win32', () => {
    const l = { ...lookup([]), platform: 'win32' as const, pathEnv: 'C:\\lms;D:\\bin' };
    l.exists = (p: string) => p === 'D:\\bin\\lms.cmd';
    expect(findLmsBinary(l)).toBe('D:\\bin\\lms.cmd');
  });
});

/**
 * U-H2: with the panel polling every 1.5s, an absent `lms` must cost zero spawns and one log line — the
 * resolution is memoised for the app's lifetime, absent or present.
 */
describe('resolveLmsOnce', () => {
  beforeEach(() => resetLmsResolutionForTests());

  it('runs the lookup once and logs the absence once across many ticks', () => {
    const find = vi.fn(() => null);
    const onAbsent = vi.fn();
    for (let tick = 0; tick < 40; tick++) expect(resolveLmsOnce(find, onAbsent)).toBeNull();
    expect(find).toHaveBeenCalledTimes(1);
    expect(onAbsent).toHaveBeenCalledTimes(1);
  });

  it('remembers a found binary and never logs an absence for it', () => {
    const find = vi.fn(() => '/Users/w/.lmstudio/bin/lms');
    const onAbsent = vi.fn();
    expect(resolveLmsOnce(find, onAbsent)).toBe('/Users/w/.lmstudio/bin/lms');
    expect(resolveLmsOnce(find, onAbsent)).toBe('/Users/w/.lmstudio/bin/lms');
    expect(find).toHaveBeenCalledTimes(1);
    expect(onAbsent).not.toHaveBeenCalled();
  });
});
