import { kvs } from '@forge/kvs';

const SETTINGS_KEY = 'settings';

// §13 defaults: Background share (%) 70, AI explanations enabled on, Daily AI token budget 200000, no Comment group.
export const DEFAULT_SETTINGS = Object.freeze({ backgroundShare: 70, aiEnabled: true, tokenBudget: 200000, commentGroup: '' });

export async function loadSettings() {
  const stored = await kvs.get(SETTINGS_KEY);
  return { ...DEFAULT_SETTINGS, ...(stored ?? {}) };
}

export const storeSettings = (settings) => kvs.set(SETTINGS_KEY, settings);

export const LABELS = Object.freeze({
  backgroundShare: 'Background share (%)',
  aiEnabled: 'AI explanations enabled',
  tokenBudget: 'Daily AI token budget',
  commentGroup: 'Comment group',
});

const wholeNumber = (raw) => {
  const text = typeof raw === 'number' ? String(raw) : typeof raw === 'string' ? raw.trim() : '';
  if (!/^\d+$/.test(text)) return null;
  const n = Number(text);
  return Number.isSafeInteger(n) ? n : null;
};

// Returns { settings } or { error }: a share that is not a whole number 10–90 or a budget that is not a whole
// number ≥ 0 refuses the whole save.
export function validateSettings(input) {
  const share = wholeNumber(input?.backgroundShare);
  if (share === null || share < 10 || share > 90) return { error: 'Background share (%) must be a whole number from 10 to 90.' };
  const budget = wholeNumber(input?.tokenBudget);
  if (budget === null) return { error: 'Daily AI token budget must be a whole number of 0 or more.' };
  const ai = input?.aiEnabled === 'true' ? true : input?.aiEnabled === 'false' ? false : input?.aiEnabled;
  if (typeof ai !== 'boolean') return { error: 'AI explanations enabled must be on or off.' };
  const group = input?.commentGroup === undefined || input?.commentGroup === null ? '' : String(input.commentGroup).trim();
  return { settings: { backgroundShare: share, aiEnabled: ai, tokenBudget: budget, commentGroup: group } };
}
