/** Exact provider IDs can be short (for example o3); length never proves authenticity. */
export function benchmarkModelIdProblem(modelId: string | undefined): string | null {
  const n = (modelId ?? '').trim().length;
  if (n === 0) return 'This result carries no model id from the engine — run the benchmark again.';
  if (n > 120) return 'The recorded model id exceeds the publication limit of 120 characters.';
  return null;
}
