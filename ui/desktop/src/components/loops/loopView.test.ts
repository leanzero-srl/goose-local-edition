import { describe, expect, it } from 'vitest';
import { formatMessageTimestamp } from '../../utils/timeUtils';
import { loopWords as w } from './loopWords';
import { chatClockTime, hm, tickCauseWords, viewerOffsetMinutes } from './loopView';
import { clockTime, fmtTime, type LoopCadence } from './model';

/**
 * Q-316 (live critic on 3.0.68): a loop's times read "06:45" beside the chat's "6:48 AM", and the
 * first tick — the one the person started — said "back to back" / "every 5 min".
 */
describe('a loop’s clock is the chat’s clock (Q-316)', () => {
  it('every loop time is written exactly as the transcript writes the same instant', () => {
    const now = new Date();
    const base = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
    for (const minutes of [0, 6 * 60 + 45, 12 * 60, 13 * 60 + 5, 22 * 60 + 40, 23 * 60 + 59]) {
      const ms = base + minutes * 60_000;
      const chat = formatMessageTimestamp(ms / 1000, new Date(ms));
      expect(hm(fmtTime(ms)), `${minutes} min`).toBe(chat);
      expect(chatClockTime(ms, viewerOffsetMinutes(ms))).toEqual({ ok: true, value: chat });
    }
  });

  it('at a given offset it says that offset’s time; an impossible offset is refused like clockTime’s', () => {
    const ms = Date.parse('2026-09-28T06:45:00Z');
    expect(chatClockTime(ms, 0)).toEqual({ ok: true, value: '6:45 AM' });
    expect(chatClockTime(ms, 16 * 60)).toEqual({ ok: true, value: '10:45 PM' });
    // The rule's own HH:MM is untouched — goosed and the shared fixture are pinned to it.
    expect(clockTime(ms, 16 * 60)).toEqual({ ok: true, value: '22:45' });
    expect(chatClockTime(ms, 24 * 60)).toEqual(clockTime(ms, 24 * 60));
    expect(chatClockTime(ms, 24 * 60).ok).toBe(false);
  });
});

describe('what started a tick (Q-316, after Q-279)', () => {
  it('the first tick is started by the person, whatever the cadence', () => {
    const cadences: LoopCadence[] = [
      { kind: 'every', every: '5m' },
      { kind: 'back_to_back' },
      { kind: 'self_paced' },
    ];
    for (const cadence of cadences) {
      expect(tickCauseWords('first', cadence).message).toBe(w.originFirst);
    }
    expect(tickCauseWords('cadence', { kind: 'every', every: '5m' }).message).toBe(
      w.cadenceMinutes
    );
    expect(tickCauseWords('back_to_back', { kind: 'back_to_back' }).message).toBe(
      w.cadenceBackToBack
    );
  });
});
