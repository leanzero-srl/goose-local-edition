import { expect, it } from 'vitest';
import { publishNotes } from './benchPublishNotes';

it('says what the site did, in plain words, including a keychain-denied operator key', () => {
  expect(publishNotes({ baseline: true })).toEqual(['Listed as a LeanZero cloud baseline.']);
  expect(publishNotes({ coupled: true, comparisonReceipt: true })[0]).toMatch(/^Coupled with this setup/);
  expect(publishNotes({ coupled: false, comparisonReceipt: true })[0]).toMatch(/^Carries its comparison receipt/);
  expect(publishNotes({ operator: 'not-operator' })[0]).toMatch(/not accepted/);
  expect(publishNotes({ operatorWarning: 'Operator key unavailable (denied): published as a community entry.' })).toEqual([
    'Operator key unavailable (denied): published as a community entry.',
  ]);
  expect(publishNotes({})).toEqual([]);
});
