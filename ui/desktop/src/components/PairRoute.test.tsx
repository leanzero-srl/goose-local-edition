/**
 * @vitest-environment jsdom
 */
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import { AppEvents } from '../constants/events';
import { chatWindowRoute } from '../utils/chatWindowRoute';
import PairRoute from './PairRoute';

const mocks = vi.hoisted(() => ({
  createSession: vi.fn(),
  getExtensions: vi.fn(),
}));

vi.mock('../sessions', () => ({ createSession: mocks.createSession }));
vi.mock('./ConfigContext', () => ({
  useConfig: () => ({ getExtensions: mocks.getExtensions }),
}));
vi.mock('../utils/analytics', () => ({ trackErrorWithContext: vi.fn() }));

const FOLDER = '/Users/me/work/shop';
const EXTENSIONS = [{ name: 'developer', enabled: true, type: 'builtin' }];

function LocationProbe() {
  const location = useLocation();
  return <div data-testid="location">{`${location.pathname}${location.search}`}</div>;
}

type Entry = string | { pathname: string; search?: string; state?: unknown };

function renderPair(entry: Entry) {
  return render(
    <IntlTestWrapper>
      <MemoryRouter initialEntries={[entry]}>
        <Routes>
          <Route
            path="/pair"
            element={
              <>
                <PairRoute activeSessions={[]} />
                <LocationProbe />
              </>
            }
          />
          <Route path="/" element={<LocationProbe />} />
        </Routes>
      </MemoryRouter>
    </IntlTestWrapper>
  );
}

function capture(event: string) {
  const seen: unknown[] = [];
  const onEvent = (e: Event) => seen.push((e as CustomEvent).detail);
  window.addEventListener(event, onEvent);
  return { seen, stop: () => window.removeEventListener(event, onEvent) };
}

let added: ReturnType<typeof capture>;

beforeEach(() => {
  vi.clearAllMocks();
  (window as unknown as { appConfig: unknown }).appConfig = {
    get: (key: string) => (key === 'GOOSE_WORKING_DIR' ? FOLDER : undefined),
  };
  mocks.getExtensions.mockResolvedValue(EXTENSIONS);
  mocks.createSession.mockResolvedValue({ id: 'sess-1', name: 'New Chat' });
  added = capture(AppEvents.ADD_ACTIVE_SESSION);
});

afterEach(() => added.stop());

describe('PairRoute — Q-491: a folder-only window starts a session in its folder', () => {
  it("opens main's folder-only route as a new session bound to the folder", async () => {
    // What createChatWindow({ dir, viewType: 'pair' }) loads: GOOSE_WORKING_DIR is the dir.
    renderPair(chatWindowRoute({ viewType: 'pair' }));

    expect(screen.getByTestId('pair-route-starting')).toHaveTextContent(
      'Starting a session in shop'
    );
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent('/pair?resumeSessionId=sess-1')
    );
    expect(mocks.createSession).toHaveBeenCalledTimes(1);
    expect(mocks.createSession).toHaveBeenCalledWith(FOLDER, {
      recipeDeeplink: undefined,
      recipeId: undefined,
      allExtensions: EXTENSIONS,
    });
    expect(added.seen).toContainEqual({
      sessionId: 'sess-1',
      initialMessage: undefined,
      noAutoSubmit: undefined,
    });
    // The request is gone from the URL: a reload or a back step resumes, never makes another.
    expect(screen.getByTestId('location')).not.toHaveTextContent('newSession');
    expect(screen.queryByTestId('pair-route-starting')).toBeNull();
  });

  it('never leaves a bare /pair empty: it offers a new session in the window folder', async () => {
    renderPair('/pair');

    expect(screen.getByTestId('pair-route-empty')).toHaveTextContent('No session is open');
    expect(mocks.createSession).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'New session in shop' }));
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent('/pair?resumeSessionId=sess-1')
    );
    expect(mocks.createSession).toHaveBeenCalledTimes(1);
    expect(mocks.createSession.mock.calls[0][0]).toBe(FOLDER);
  });

  it('sends the launcher query into the one session it makes', async () => {
    // main opens the launcher's window on /pair with no request; set-initial-message then
    // navigates here with the message as route state.
    const route = chatWindowRoute({ initialMessage: 'fix the build' });
    renderPair({
      pathname: '/pair',
      search: route.slice('/pair'.length),
      state: { initialMessage: { msg: 'fix the build', images: [] } },
    });

    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent('/pair?resumeSessionId=sess-1')
    );
    expect(mocks.createSession).toHaveBeenCalledTimes(1);
    expect(mocks.createSession.mock.calls[0][0]).toBe(FOLDER);
    expect(added.seen).toContainEqual({
      sessionId: 'sess-1',
      initialMessage: { msg: 'fix the build', images: [] },
      noAutoSubmit: undefined,
    });
  });

  it('says why the session did not start and makes it again on Try again', async () => {
    mocks.createSession.mockRejectedValueOnce(new Error('goosed is not reachable'));
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    renderPair(chatWindowRoute({ viewType: 'pair' }));

    const failed = await screen.findByTestId('pair-route-failed');
    expect(failed).toHaveTextContent('Could not start a session in shop');
    expect(failed).toHaveTextContent('goosed is not reachable');
    expect(mocks.createSession).toHaveBeenCalledTimes(1);

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    });
    await waitFor(() =>
      expect(screen.getByTestId('location')).toHaveTextContent('/pair?resumeSessionId=sess-1')
    );
    expect(mocks.createSession).toHaveBeenCalledTimes(2);
    consoleError.mockRestore();
  });

  it('shows nothing of its own while a session is open', () => {
    renderPair('/pair?resumeSessionId=sess-9');
    expect(screen.queryByTestId('pair-route-empty')).toBeNull();
    expect(screen.queryByTestId('pair-route-starting')).toBeNull();
    expect(mocks.createSession).not.toHaveBeenCalled();
    expect(added.seen).toContainEqual({
      sessionId: 'sess-9',
      initialMessage: undefined,
      noAutoSubmit: undefined,
    });
  });
});
