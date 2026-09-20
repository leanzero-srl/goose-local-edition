import { expect, it } from 'vitest';
import { harnessPhase } from './benchPhase';
it('consumes explicit harness lifecycle and rejects prose, incomplete and unknown phases', () => {
  expect(harnessPhase('BENCH_PHASE {"phase":"build"}')).toBe('build');
  expect(harnessPhase('BENCH_PHASE {"phase":"score"}')).toBe('score');
  for (const line of [
    'The build is done, scoring now',
    'BENCH_PHASE {',
    'BENCH_PHASE {"phase":"finished"}',
    ' BENCH_PHASE {"phase":"score"}',
  ])
    expect(harnessPhase(line)).toBeNull();
});
