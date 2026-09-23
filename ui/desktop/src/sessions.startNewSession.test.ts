import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AppEvents } from './constants/events';

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  rename: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock('./acp/chatSessionController', () => ({
  acpChatSessionController: { createSession: mocks.createSession },
}));
vi.mock('./acp/extensions', () => ({
  getConfiguredGooseExtensions: async () => [],
  gooseExtensionName: () => '',
}));
vi.mock('./acp/sessions', () => ({ acpRenameSession: mocks.rename }));
vi.mock('./toasts', () => ({ toastError: mocks.toastError }));

import { startNewSession } from './sessions';
import { acpChatSessionActions, acpChatSessionStore } from './acp/chatSessionStore';

const created = () => ({ id: 's-1', name: 'New Chat', user_set_name: false });

function capture(event: string) {
  const seen: unknown[] = [];
  const onEvent = (e: Event) => seen.push((e as CustomEvent).detail);
  window.addEventListener(event, onEvent);
  return { seen, stop: () => window.removeEventListener(event, onEvent) };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.createSession.mockResolvedValue(created());
  mocks.rename.mockResolvedValue(undefined);
});

/** UX audit C3: an ask-AI session is named after its item at creation, through the rename API, so
 *  the stored name is USER-set and the model's auto-title (maybe_update_name) never replaces it. */
describe('startNewSession with a title', () => {
  it('renames before announcing the session, and the announced session carries the user-set name', async () => {
    const created = capture(AppEvents.SESSION_CREATED);
    const session = await startNewSession('I want to work on…', vi.fn(), '/w', {
      extensionConfigs: [],
      title: 'Memory · lms-ps-is-fleet-ground-truth',
    });
    created.stop();
    expect(mocks.rename).toHaveBeenCalledWith('s-1', 'Memory · lms-ps-is-fleet-ground-truth');
    expect(session).toMatchObject({
      name: 'Memory · lms-ps-is-fleet-ground-truth',
      user_set_name: true,
    });
    expect(created.seen).toEqual([{ session }]);
  });

  it('the chat store — what the header reads — carries the name the moment the session opens', async () => {
    // createSession seeds the store with the engine placeholder; the chat view never reloads a
    // cached session, so the header showed "New Session" over a named session (UX audit 2026-09-23).
    mocks.createSession.mockImplementation(async () => {
      acpChatSessionActions.finishSessionLoad('s-1', created() as never);
      return created();
    });
    await startNewSession('I want to work on…', vi.fn(), '/w', {
      title: 'Skill · release-checklist',
    });
    expect(acpChatSessionStore.getSnapshot('s-1')?.session).toMatchObject({
      name: 'Skill · release-checklist',
      user_set_name: true,
    });
  });

  it('a failed rename is said out loud and the session still starts under its engine name', async () => {
    mocks.rename.mockRejectedValue(new Error('session store locked'));
    const setView = vi.fn();
    const session = await startNewSession('hi', setView, '/w', { title: 'Skill · deploy' });
    expect(mocks.toastError).toHaveBeenCalledWith(
      expect.objectContaining({ msg: expect.stringContaining('session store locked') })
    );
    expect(session.user_set_name).toBe(false);
    expect(setView).toHaveBeenCalledWith(
      'pair',
      expect.objectContaining({ resumeSessionId: 's-1' })
    );
  });

  it('without a title nothing is renamed — the auto-title applies', async () => {
    await startNewSession('hi', vi.fn(), '/w');
    expect(mocks.rename).not.toHaveBeenCalled();
  });
});
