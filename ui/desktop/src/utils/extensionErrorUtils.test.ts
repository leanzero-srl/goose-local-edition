import { beforeEach, describe, expect, it, vi } from 'vitest';

const toast = vi.hoisted(() => ({ error: vi.fn(), extensionLoading: vi.fn() }));
vi.mock('../toasts', () => ({ toastService: toast }));

import { showExtensionLoadResults } from './extensionErrorUtils';

beforeEach(() => vi.clearAllMocks());

/** UX audit C6: "Successfully loaded 12 extensions" covered the transcript on EVERY session open.
 *  An all-green load is the expected state and says nothing; a failure stays loud. */
describe('showExtensionLoadResults', () => {
  it('is silent when every extension loaded', () => {
    showExtensionLoadResults([
      { name: 'developer', success: true },
      { name: 'memory', success: true },
    ]);
    expect(toast.extensionLoading).not.toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('keeps the grouped toast when one of several failed', () => {
    showExtensionLoadResults([
      { name: 'developer', success: true },
      { name: 'jira', success: false, error: 'spawn npx ENOENT' },
    ]);
    expect(toast.extensionLoading).toHaveBeenCalledTimes(1);
    const [statuses, total, complete] = toast.extensionLoading.mock.calls[0];
    expect(total).toBe(2);
    expect(complete).toBe(true);
    expect(statuses).toEqual([
      expect.objectContaining({ name: 'developer', status: 'success' }),
      expect.objectContaining({ name: 'jira', status: 'error', error: 'spawn npx ENOENT' }),
    ]);
  });

  it('keeps the single error toast when the only extension failed', () => {
    showExtensionLoadResults([{ name: 'jira', success: false, error: 'spawn npx ENOENT' }]);
    expect(toast.error).toHaveBeenCalledWith(
      expect.objectContaining({ title: 'jira', traceback: 'spawn npx ENOENT' })
    );
  });
});
