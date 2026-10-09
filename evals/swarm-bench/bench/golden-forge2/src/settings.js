import { kvs } from '@forge/kvs';

const SETTINGS_KEY = 'settings';
const AUDIT_KEY = 'admin-audit';
const AUDIT_SIZE = 20;
export const SECRET_KEY = 'ci-secret'; // kvs.setSecret only; never returned by any resolver
export const SECRET_META_KEY = 'ci-secret-meta'; // { last4, rotatedAt }

// The admin panel's settings, with the contract's defaults until an admin saves.
export const DEFAULTS = Object.freeze({ backgroundShare: 70, aiEnabled: true, dailyTokenBudget: 200000, commentGroup: '' });

export async function loadSettings() {
  return { ...DEFAULTS, ...((await kvs.get(SETTINGS_KEY)) ?? {}) };
}

// Returns { settings } or { error } for an admin's submitted values; nothing is coerced silently.
export function validateSettings(input, current) {
  const next = { ...current };
  const errors = [];
  if (input.backgroundShare !== undefined) {
    const n = Number(input.backgroundShare);
    if (!Number.isInteger(n) || n < 10 || n > 90) errors.push('Background share (%) must be a whole number from 10 to 90.');
    else next.backgroundShare = n;
  }
  if (input.aiEnabled !== undefined) {
    if (typeof input.aiEnabled !== 'boolean') errors.push('AI explanations enabled must be on or off.');
    else next.aiEnabled = input.aiEnabled;
  }
  if (input.dailyTokenBudget !== undefined) {
    const n = Number(input.dailyTokenBudget);
    if (!Number.isInteger(n) || n < 0) errors.push('Daily AI token budget must be a whole number of 0 or more.');
    else next.dailyTokenBudget = n;
  }
  if (input.commentGroup !== undefined) {
    if (typeof input.commentGroup !== 'string' || input.commentGroup.trim().length > 255) errors.push('Comment group must be a group name of at most 255 characters.');
    else next.commentGroup = input.commentGroup.trim();
  }
  return errors.length ? { error: errors.join(' ') } : { settings: next };
}

export const saveSettings = (settings) => kvs.set(SETTINGS_KEY, settings);

export const LABELS = {
  backgroundShare: 'Background share (%)',
  aiEnabled: 'AI explanations enabled',
  dailyTokenBudget: 'Daily AI token budget',
  commentGroup: 'Comment group',
};

export async function auditLog() {
  return (await kvs.get(AUDIT_KEY)) ?? [];
}

export async function audit(entries) {
  if (!entries.length) return;
  const log = [...entries, ...(await auditLog())].slice(0, AUDIT_SIZE);
  await kvs.set(AUDIT_KEY, log);
}
