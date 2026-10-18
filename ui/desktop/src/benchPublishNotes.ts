/** What the site did with a publish, in plain words for the card (renderer-safe: no node imports). */
export function publishNotes(res: {
  baseline?: boolean;
  coupled?: boolean;
  operator?: string;
  priceWarning?: string;
  cohortProblem?: string;
}): string[] {
  const notes: string[] = [];
  if (res.baseline) notes.push('Listed as a LeanZero cloud baseline.');
  if (res.coupled) notes.push('Carries its comparison receipt: it couples with this setup’s other build into one entry.');
  if (res.cohortProblem) notes.push(`No comparison receipt (${res.cohortProblem}), so this build is listed on its own.`);
  if (res.operator === 'rejected') notes.push('The LeanZero operator key was not accepted, so it is listed as a community run.');
  if (res.operator === 'unconfigured') notes.push('leanzero.net has no operator key configured, so it is listed as a community run.');
  if (res.operator === 'needs-comparison-cohort')
    notes.push('A Forge 2.0 cloud baseline needs its comparison receipt, so it is listed as a community run.');
  if (res.priceWarning) notes.push(res.priceWarning);
  return notes;
}
