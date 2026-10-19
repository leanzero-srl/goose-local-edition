/** What the site did with a publish, in plain words for the card (renderer-safe: no node imports). */
export function publishNotes(res: {
  baseline?: boolean;
  coupled?: boolean;
  comparisonReceipt?: boolean;
  operator?: string;
  priceWarning?: string;
  cohortProblem?: string;
  operatorWarning?: string;
}): string[] {
  const notes: string[] = [];
  if (res.baseline) notes.push('Listed as a LeanZero cloud baseline.');
  // "coupled" is the site's word for what the board shows NOW; a receipt alone only means it will couple later.
  if (res.coupled) notes.push('Coupled with this setup’s other build: the board shows them as one entry.');
  else if (res.comparisonReceipt)
    notes.push('Carries its comparison receipt: it becomes one entry with this setup’s other build once that is published.');
  if (res.cohortProblem) notes.push(`No comparison receipt (${res.cohortProblem}), so this build is listed on its own.`);
  if (res.operator === 'not-operator')
    notes.push('The LeanZero operator key was not accepted, so it is listed as a community run.');
  if (res.operator === 'needs-comparison-cohort')
    notes.push('A Forge 2.0 cloud baseline needs its comparison receipt, so it is listed as a community run.');
  if (res.operatorWarning) notes.push(res.operatorWarning);
  if (res.priceWarning) notes.push(res.priceWarning);
  return notes;
}
