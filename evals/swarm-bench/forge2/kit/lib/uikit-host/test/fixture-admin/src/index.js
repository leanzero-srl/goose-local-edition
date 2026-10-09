import Resolver from '@forge/resolver';
import { kvs } from '@forge/kvs';

const resolver = new Resolver();
const DEFAULTS = { backgroundShare: 70, aiEnabled: true };

resolver.define('getSettings', async () => ({ ...DEFAULTS, ...((await kvs.get('settings')) ?? {}), changes: (await kvs.get('changes')) ?? [] }));

resolver.define('saveSettings', async ({ payload, context }) => {
  // A number input's value reaches the resolver as the string the DOM holds.
  const share = Number(payload.backgroundShare);
  if (!Number.isInteger(share) || share < 10 || share > 90) throw new Error('backgroundShare must be a whole number from 10 to 90');
  const settings = { backgroundShare: share, aiEnabled: payload.aiEnabled === true };
  await kvs.set('settings', settings);
  const changes = (await kvs.get('changes')) ?? [];
  changes.push({ when: changes.length + 1, who: context.accountId, what: `share ${share}, AI ${settings.aiEnabled ? 'on' : 'off'}` });
  await kvs.set('changes', changes);
  return settings;
});

export const handler = resolver.getDefinitions();
