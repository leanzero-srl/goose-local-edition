import { expect, it } from 'vitest';
import { benchmarkProfileDirectory } from './benchProfile';
it('isolates explicit profiles while preserving the existing normal result store', () => {
  expect(benchmarkProfileDirectory('/home/user')).toBe('/home/user/.config/goose/benchmark');
  expect(benchmarkProfileDirectory('/home/user','/clean/profile')).toBe('/clean/profile/config/benchmark');
  expect(benchmarkProfileDirectory('/home/user','/another/profile')).not.toBe(benchmarkProfileDirectory('/home/user','/clean/profile'));
});
