import Resolver from '@forge/resolver';
import { kvs } from '@forge/kvs';

const resolver = new Resolver();
const DEFAULTS = { retentionDays: 30, digestEnabled: true };

resolver.define('getPreferences', async () => ({ ...DEFAULTS, ...((await kvs.get('preferences')) ?? {}), changes: (await kvs.get('changes')) ?? [] }));

resolver.define('savePreferences', async ({ payload, context }) => {
  // A number input's value reaches the resolver as the string the DOM holds.
  const days = Number(payload.retentionDays);
  if (!Number.isInteger(days) || days < 1 || days > 365) throw new Error('retentionDays must be a whole number from 1 to 365');
  const preferences = { retentionDays: days, digestEnabled: payload.digestEnabled === true };
  await kvs.set('preferences', preferences);
  const changes = (await kvs.get('changes')) ?? [];
  changes.push({ when: changes.length + 1, who: context.accountId, what: `retention ${days}, digest ${preferences.digestEnabled ? 'on' : 'off'}` });
  await kvs.set('changes', changes);
  return preferences;
});

export const handler = resolver.getDefinitions();
