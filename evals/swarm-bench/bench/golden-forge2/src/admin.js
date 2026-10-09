import Resolver from '@forge/resolver';
import { randomBytes } from 'node:crypto';
import { kvs } from '@forge/kvs';
import { jiraJson, route, RateLimited, JiraError } from './jira';
import { loadSettings, validateSettings, saveSettings, LABELS, auditLog, audit, SECRET_KEY, SECRET_META_KEY } from './settings';
import { migrationProgress } from './migrate';

// Resolvers live 25 s; a longer Retry-After goes back to the page.
const UI = { maxWaitSeconds: 5 };
const FORBIDDEN = { ok: false, forbidden: true, error: 'Only Jira administrators can view or change Scope Ledger settings.' };

// Any user who can load the admin page can invoke these resolvers, so each one asks Jira — as the caller,
// the invocation's own user — whether they hold the global ADMINISTER permission. Nothing in the payload
// says who the caller is or what they may do.
async function isJiraAdmin() {
  const res = await jiraJson('user', route`/rest/api/3/mypermissions?permissions=ADMINISTER`, undefined, UI);
  return res?.permissions?.ADMINISTER?.havePermission === true;
}

async function actorOf(context) {
  const me = await jiraJson('user', route`/rest/api/3/myself`, undefined, UI);
  return { accountId: context.accountId, who: me.displayName };
}

const show = (v) => (v === '' ? '(everyone who can browse)' : String(v));

export function migrationText(m) {
  if (!m.started) return 'Migration has not started yet';
  return m.complete ? `Migrated ${m.migrated} of ${m.total} v1 rows — complete` : `Migrated ${m.migrated} of ${m.total} v1 rows`;
}

async function adminState() {
  const [settings, meta, migration, log] = await Promise.all([loadSettings(), kvs.get(SECRET_META_KEY), migrationProgress(), auditLog()]);
  return {
    ok: true,
    settings,
    secret: meta ? { masked: `••••${meta.last4}`, rotatedAt: meta.rotatedAt } : null,
    migration: { ...migration, text: migrationText(migration) },
    audit: log,
  };
}

const guarded = (fn) => async (req) => {
  try {
    if (!(await isJiraAdmin())) return FORBIDDEN;
    return await fn(req);
  } catch (e) {
    if (e instanceof RateLimited) return { ok: false, rateLimited: true, retryAfter: e.retryAfterSeconds, error: `Jira is rate limiting this app; try again in ${e.retryAfterSeconds} seconds.` };
    console.error(`admin resolver ${req?.call?.functionKey ?? ''} failed: ${e?.message ?? e}`);
    return { ok: false, error: e instanceof JiraError ? `Jira refused the request (${e.status}).` : `The request failed: ${e?.message ?? e}` };
  }
};

const resolver = new Resolver();

resolver.define('getAdmin', guarded(() => adminState()));

resolver.define(
  'saveSettings',
  guarded(async ({ payload, context }) => {
    const current = await loadSettings();
    const checked = validateSettings(payload?.settings ?? {}, current);
    if (checked.error) return { ok: false, error: checked.error };
    const changed = Object.keys(LABELS).filter((k) => checked.settings[k] !== current[k]);
    if (!changed.length) return { ...(await adminState()), saved: 0 };
    const actor = await actorOf(context);
    await saveSettings(checked.settings);
    const at = new Date(Date.now()).toISOString();
    await audit(changed.map((k) => ({ at, accountId: actor.accountId, who: actor.who, what: `${LABELS[k]}: ${show(current[k])} → ${show(checked.settings[k])}` })));
    return { ...(await adminState()), saved: changed.length };
  }),
);

// The new secret is in this one answer only; every later read shows ••••<last4>.
resolver.define(
  'rotateSecret',
  guarded(async ({ context }) => {
    const actor = await actorOf(context);
    const secret = randomBytes(32).toString('hex');
    await kvs.setSecret(SECRET_KEY, secret);
    const meta = { last4: secret.slice(-4), rotatedAt: new Date(Date.now()).toISOString() };
    await kvs.set(SECRET_META_KEY, meta);
    await audit([{ at: meta.rotatedAt, accountId: actor.accountId, who: actor.who, what: 'Rotate CI secret' }]);
    return { ...(await adminState()), newSecret: secret };
  }),
);

export const adminResolver = resolver.getDefinitions();
