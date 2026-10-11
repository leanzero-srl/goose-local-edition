export const instantOf = value => typeof value === 'number' ? value : Date.parse(value);
export const compareIds = (a,b) => /^\d+$/.test(a) && /^\d+$/.test(b)
  ? (BigInt(a)<BigInt(b) ? -1 : BigInt(a)>BigInt(b) ? 1 : 0)
  : (a<b ? -1 : a>b ? 1 : 0);

export function estimateValue(fields, fieldId) {
  const value = Number(fields?.[fieldId]);
  return Number.isFinite(value) ? value : 0;
}

export function estimateAt(fields, fieldId, histories, at) {
  let value = estimateValue(fields, fieldId);
  const later = histories.filter(h => instantOf(h.created) > at)
    .sort((a, b) => instantOf(b.created) - instantOf(a.created) || compareIds(String(b.id),String(a.id)));
  for (const history of later) {
    for (const item of [...(history.items ?? [])].reverse()) {
      if (item.fieldId === fieldId) value = estimateValue({ [fieldId]: item.from }, fieldId);
    }
  }
  return value;
}
