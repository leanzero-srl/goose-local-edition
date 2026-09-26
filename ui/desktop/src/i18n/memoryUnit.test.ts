import { describe, expect, it } from 'vitest';
import en from './messages/en.json';

/**
 * Q-156: memory reads in ONE unit. goose's memory figures are binary (bytes / 2^30, the sidecar's
 * `availableMemoryGb`, the planner's budgets) and the app says GB for them — My Macs, the Engine
 * tab's memory line, Run it's fit. The 3.0.52 live round read "32.1 of 86.8 GiB peak" on the Engine
 * card beside "59.8 GB available of 128.0 GB". No message a person reads says GiB again.
 */
describe('memory has one unit in every message a person reads', () => {
  it('no catalog message says GiB', () => {
    const offenders = Object.entries(en as Record<string, { defaultMessage: string }>)
      .filter(([, { defaultMessage }]) => /\bGiB\b/.test(defaultMessage))
      .map(([id, { defaultMessage }]) => `${id}: ${defaultMessage}`);
    expect(offenders).toEqual([]);
  });
});
