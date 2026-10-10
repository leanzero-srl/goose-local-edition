import { getAppContext } from '@forge/api';

export const HOUR_MS = 3_600_000;

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, Math.max(0, ms)));

// Virtual time left in this invocation (§11). Outside a Forge invocation (unit tests) there is no limit.
export function remainingMs() {
  try {
    const left = getAppContext().invocationRemainingTimeInMillis();
    return Number.isFinite(left) ? left : Infinity;
  } catch {
    return Infinity;
  }
}

export const hourIndex = (ms = Date.now()) => Math.floor(ms / HOUR_MS);
export const msToNextHour = (ms = Date.now()) => HOUR_MS - (ms % HOUR_MS);
export const utcDay = (ms = Date.now()) => new Date(ms).toISOString().slice(0, 10);
