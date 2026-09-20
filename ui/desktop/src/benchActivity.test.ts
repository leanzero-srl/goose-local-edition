import { expect, it } from 'vitest';
import { appendBenchmarkActivity, emptyBenchmarkActivity } from './benchActivity';

it('keeps real tool observations when the Gemini console ends in code braces', () => {
  let state = emptyBenchmarkActivity();
  // Exact tool/argument layout observed in google-cloud-20940258 engine-console.log.
  for (const line of [
    '  ▸ shell',
    '    command: cat VISUAL-CONTRACT.md',
    '  ▸ write',
    '    path web/viz.js',
    'function draw() {',
    '}',
    '}',
  ])
    state = appendBenchmarkActivity(state, line, 'stdout', 1000);
  expect(state.entries.map((e) => [e.title, e.detail])).toEqual([
    ['shell', 'cat VISUAL-CONTRACT.md'],
    ['write', 'web/viz.js'],
  ]);
  expect(state.raw).toContain('}\n}\n');
  expect(state.entries.slice(-1)[0]?.title).toBe('write');
  const scoring = appendBenchmarkActivity(state, 'BENCH_PHASE {"phase":"score"}', 'stdout', 2000);
  expect(scoring.entries.slice(-1)[0]).toMatchObject({
    kind: 'phase',
    title: 'Scoring started',
    at: 2000,
  });
});
it('does not infer completion from prose, JSON, shell output or stderr markers', () => {
  let state = emptyBenchmarkActivity();
  for (const line of [
    'Everything verified successfully',
    '{"phase":"done"}',
    '    path fake.js',
    '}',
  ])
    state = appendBenchmarkActivity(state, line, 'stdout', 1000);
  state = appendBenchmarkActivity(state, 'BENCH_PHASE {"phase":"score"}', 'stderr', 2000);
  expect(state.entries).toEqual([]);
  expect(state.raw).toContain('[stderr]');
});
it('bounds retained raw text and preserves stable action identity across rolling updates', () => {
  let state = emptyBenchmarkActivity();
  for (let i = 0; i < 30; i++) {
    state = appendBenchmarkActivity(state, '  ▸ shell', 'stdout', i);
    state = appendBenchmarkActivity(state, '    command: ' + 'x'.repeat(10000), 'stdout', i);
  }
  expect(state.raw.length).toBeLessThanOrEqual(16000);
  expect(state.entries).toHaveLength(12);
  expect(new Set(state.entries.map((e) => e.id)).size).toBe(12);
  expect(state.entries[0].id).toBe(19);
  expect(state.entries.slice(-1)[0]?.detail).toHaveLength(4000);
});

it('retains multiline command context until the console separator, without treating output as the command', () => {
  let state = emptyBenchmarkActivity();
  for (const line of [
    '  ▸ shell',
    '    command: python3 -c "',
    "with open('SB7-CONTRACT.md') as f:",
    '    print(f.read())',
    '"',
    '',
    '# Build app',
    'verified',
  ])
    state = appendBenchmarkActivity(state, line, 'stdout', 1);
  expect(state.entries[0].detail).toBe(
    'python3 -c "\nwith open(\'SB7-CONTRACT.md\') as f:\n    print(f.read())\n"'
  );
  expect(state.entries[0].detail).not.toContain('verified');
});

it('finds the image source after nested crop arguments in the actual CLI layout', () => {
  let state = emptyBenchmarkActivity();
  for (const line of [
    '  ▸ read_image',
    '    crop:',
    '        width: 375',
    '        x: 0',
    '    source: test-mobile.png',
    '',
  ])
    state = appendBenchmarkActivity(state, line, 'stdout', 1);
  expect(state.entries[0].detail).toBe('test-mobile.png');
});
