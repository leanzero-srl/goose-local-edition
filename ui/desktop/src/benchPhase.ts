export type BenchmarkPhase = 'boot' | 'build' | 'score' | 'done';

/** Recognizes the harness-format lifecycle marker in the shared console stream. */
export function harnessPhase(line: string): 'build' | 'score' | null {
  if (!line.startsWith('BENCH_PHASE ')) return null;
  try {
    const value: unknown = JSON.parse(line.slice('BENCH_PHASE '.length));
    if (
      value &&
      typeof value === 'object' &&
      'phase' in value &&
      (value.phase === 'build' || value.phase === 'score')
    )
      return value.phase;
  } catch {
    /* An incomplete or invalid lifecycle marker cannot assert a phase. */
  }
  return null;
}
