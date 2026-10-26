import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, renderHook, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import {
  HIDDEN_POLL_MS,
  STALE_AFTER_MS,
  cardReadAt,
  staleSeconds,
  useVisibleInterval,
  type MacFacts,
} from './useMacs';
import { UpdatedAgo } from './MyMacs';

/**
 * 3.0.130 "stale cards": My Macs stopped polling while the window was hidden and then showed old
 * facts with nothing saying so. Now: a slow read keeps going while hidden, showing the window reads
 * at once, and a card older than STALE_AFTER_MS says "updated Xs ago" until fresh data lands.
 */

let visibility: DocumentVisibilityState = 'visible';
function setVisibility(v: DocumentVisibilityState) {
  visibility = v;
  document.dispatchEvent(new Event('visibilitychange'));
}

beforeEach(() => {
  vi.useFakeTimers();
  visibility = 'visible';
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    get: () => visibility,
  });
});
afterEach(() => {
  vi.useRealTimers();
});

describe('useVisibleInterval', () => {
  it('keeps a slow read going while hidden and reads at once on show', () => {
    const fn = vi.fn();
    renderHook(() => useVisibleInterval(fn, 1500, true));
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenLastCalledWith(false);
    vi.advanceTimersByTime(3000);
    expect(fn).toHaveBeenCalledTimes(3);

    act(() => setVisibility('hidden'));
    fn.mockClear();
    vi.advanceTimersByTime(HIDDEN_POLL_MS - 1);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenLastCalledWith(true);
    vi.advanceTimersByTime(HIDDEN_POLL_MS * 2);
    expect(fn).toHaveBeenCalledTimes(3);

    fn.mockClear();
    act(() => setVisibility('visible'));
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenLastCalledWith(false);
    vi.advanceTimersByTime(1500);
    expect(fn).toHaveBeenCalledTimes(2);
  });

  it('a slower visible cadence than the hidden floor keeps its own rate while hidden', () => {
    const fn = vi.fn();
    renderHook(() => useVisibleInterval(fn, 20000, true));
    act(() => setVisibility('hidden'));
    fn.mockClear();
    vi.advanceTimersByTime(19999);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('a repeated visibilitychange with no change does not double the timer', () => {
    const fn = vi.fn();
    renderHook(() => useVisibleInterval(fn, 1500, true));
    act(() => setVisibility('visible'));
    fn.mockClear();
    vi.advanceTimersByTime(1500);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('stops entirely on unmount and when disabled', () => {
    const fn = vi.fn();
    const { unmount } = renderHook(() => useVisibleInterval(fn, 1500, true));
    unmount();
    fn.mockClear();
    vi.advanceTimersByTime(HIDDEN_POLL_MS * 2);
    expect(fn).not.toHaveBeenCalled();
    renderHook(() => useVisibleInterval(fn, 1500, false));
    vi.advanceTimersByTime(HIDDEN_POLL_MS * 2);
    expect(fn).not.toHaveBeenCalled();
  });
});

const FACTS: MacFacts = {
  status: null,
  statusError: null,
  models: null,
  modelsError: null,
  disk: null,
  activity: null,
  decodeTps: null,
  statusAt: null,
};

describe('card freshness', () => {
  it('is the older of the roster and the shown status read', () => {
    expect(cardReadAt(null, FACTS)).toBeNull();
    expect(cardReadAt(undefined, FACTS)).toBeNull();
    expect(cardReadAt(5000, FACTS)).toBe(5000);
    // a status that is not shown does not age the card
    expect(cardReadAt(5000, { ...FACTS, statusAt: 1000 })).toBe(5000);
    const status = { state: 'stopped' } as unknown as MacFacts['status'];
    expect(cardReadAt(5000, { ...FACTS, status, statusAt: 1000 })).toBe(1000);
    expect(cardReadAt(null, { ...FACTS, status, statusAt: 1000 })).toBe(1000);
  });

  it('is stale only past STALE_AFTER_MS', () => {
    expect(staleSeconds(null, 99999)).toBeNull();
    expect(staleSeconds(0, STALE_AFTER_MS)).toBeNull();
    expect(staleSeconds(0, STALE_AFTER_MS + 1)).toBe(10);
    expect(staleSeconds(0, 42_900)).toBe(42);
  });

  it('the card says "updated Xs ago" once old, counts up, and goes away on a fresh read', () => {
    vi.setSystemTime(100_000);
    const { rerender } = render(
      <IntlTestWrapper>
        <UpdatedAgo at={100_000} />
      </IntlTestWrapper>
    );
    expect(screen.queryByTestId('my-mac-updated-ago')).toBeNull();
    act(() => vi.advanceTimersByTime(11_000));
    expect(screen.getByTestId('my-mac-updated-ago').textContent).toBe('updated 11s ago');
    act(() => vi.advanceTimersByTime(4_000));
    expect(screen.getByTestId('my-mac-updated-ago').textContent).toBe('updated 15s ago');
    rerender(
      <IntlTestWrapper>
        <UpdatedAgo at={Date.now()} />
      </IntlTestWrapper>
    );
    expect(screen.queryByTestId('my-mac-updated-ago')).toBeNull();
  });
});
