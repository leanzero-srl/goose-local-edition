// An issue's estimate for a sprint is the value of the sprint's board's estimation field; no value counts as 0.
export function estimateOf(fields, fieldId) {
  if (!fieldId) return 0;
  const v = fields?.[fieldId];
  const n = typeof v === 'number' ? v : v === null || v === undefined || v === '' ? 0 : Number(v);
  return Number.isFinite(n) ? n : 0;
}
