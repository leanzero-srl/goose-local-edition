export type ToolOutcome = 'loading' | 'success' | 'error' | 'pending';
export function toolOutcome(result: unknown, streaming: boolean, cancelled: boolean): ToolOutcome {
  if (!result) return streaming && !cancelled ? 'loading' : 'pending';
  if (typeof result !== 'object') return 'pending';
  const record = result as Record<string, unknown>;
  const value =
    record.value && typeof record.value === 'object'
      ? (record.value as Record<string, unknown>)
      : record;
  if (record.status === 'error' || value.isError === true) return 'error';
  if (record.status === 'success' || Array.isArray(value.content)) return 'success';
  return 'pending';
}
