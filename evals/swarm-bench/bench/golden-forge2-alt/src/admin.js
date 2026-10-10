import { randomBytes } from 'node:crypto';
import { kvs } from '@forge/kvs';
import { jiraJson, route, RateLimited } from './jira';
import { personPolicy } from './budget';
import { loadSettings, storeSettings, validateSettings, LABELS } from './settings';
import { CI_SECRET_KEY } from './ci';
import { discoverConfig, loadConfig, saveConfig } from './config';
import { loadMigration } from './scope';
import { migrationPlan, migrationProgress } from './migrate';

const AUDIT_KEY = 'admin-audit';
const AUDIT_ROWS = 20;
const POLICY = personPolicy(5000);

// §13: every admin resolver, reads included, asks Jira as the caller whether they hold global ADMINISTER. The
// caller is the resolver context's account, never anything in the payload.
async function isAdministrator() {
  const res = await jiraJson('user', 'GET /rest/api/3/mypermissions', route`/rest/api/3/mypermissions?permissions=ADMINISTER`, POLICY);
  return res?.permissions?.ADMINISTER?.havePermission === true;
}

const mask = (secret) => (secret ? `••••${String(secret).slice(-4)}` : 'none yet (rotate to create one)');

async function migrationText() {
  let plan = await loadMigration();
  if (!plan) {
    let cfg = await loadConfig();
    if (!cfg) {
      cfg = await discoverConfig(POLICY, null);
      await saveConfig(cfg, null);
    }
    plan = await migrationPlan(cfg);
  }
  return (await migrationProgress(plan)).text;
}

async function panel() {
  const [settings, secret, audit, migration] = await Promise.all([loadSettings(), kvs.getSecret(CI_SECRET_KEY), kvs.get(AUDIT_KEY), migrationText()]);
  return { settings, secret: `CI secret: ${mask(secret)}`, audit: audit ?? [], migration };
}

async function audit(who, what) {
  const rows = (await kvs.get(AUDIT_KEY)) ?? [];
  await kvs.set(AUDIT_KEY, [{ when: new Date().toISOString(), who, what }, ...rows].slice(0, AUDIT_ROWS));
}

const show = (v) => (v === '' ? '(none)' : String(v));

// Every resolver answers with a value; a refusal or failure is { error } and changes nothing.
const guarded = (fn) => async (req) => {
  try {
    const accountId = req?.context?.accountId;
    if (!accountId || !(await isAdministrator())) return { error: 'Only Jira administrators can use the Scope Ledger admin page.' };
    return await fn(req, accountId);
  } catch (e) {
    if (e instanceof RateLimited) return { error: `Jira is rate limiting this app; try again in ${e.retryAfterSeconds} seconds.` };
    console.error(`admin resolver ${req?.call?.functionKey ?? ''} failed: ${e?.message ?? e}`);
    return { error: `The request failed: ${e?.message ?? e}` };
  }
};

export const adminLoad = guarded(async () => panel());

export const adminSave = guarded(async ({ payload }, accountId) => {
  const checked = validateSettings(payload);
  if (checked.error) return { error: checked.error, ...(await panel()) };
  const before = await loadSettings();
  const changed = Object.keys(LABELS).filter((k) => before[k] !== checked.settings[k]);
  if (changed.length) {
    await storeSettings(checked.settings);
    await audit(accountId, changed.map((k) => `${LABELS[k]}: ${show(before[k])} → ${show(checked.settings[k])}`).join('; '));
  }
  return { saved: true, changed: changed.length, ...(await panel()) };
});

// The new secret is shown once, in this answer only; it is stored with kvs.setSecret and nowhere else.
export const adminRotate = guarded(async (_req, accountId) => {
  const secret = randomBytes(32).toString('hex');
  await kvs.setSecret(CI_SECRET_KEY, secret);
  await audit(accountId, 'Rotate CI secret');
  const view = await panel();
  return { ...view, secret: `CI secret: ${secret}`, maskedSecret: view.secret, revealed: true };
});
