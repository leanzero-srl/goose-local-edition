import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createIntl } from 'react-intl';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import type { MlxLiveRequest } from '../leanzero-swarm/mlxLiveStats';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import type { ChatServedBy } from './chatServedBy';
import type { PeerGone } from '../../utils/routeContact';
import {
  SILENCE_GAP_MULTIPLE,
  SILENCE_MIN_GAPS,
  pickTurnCue,
  readingProgress,
  silenceAfterMs,
} from './turnStatus';
import { turnCueText } from './turnCueText';
import { useTurnCue } from './useTurnCue';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });
const MAC = "Work's Mac Studio";
const ROUTE: MlxRemoteSingleStatus = {
  state: 'ready',
  peer: 'worksmacstudio-lan-6a972f',
  peerComputerName: MAC,
};

/** The 3.0.38 round's first answer: the Studio reading a 49,293-token prompt, nothing cached. */
const READING: MlxLiveRequest = {
  id: 'r1',
  status: 'running',
  phase: 'prefill',
  elapsedS: 72,
  promptTokens: 49293,
  completionTokens: 0,
  maxTokens: null,
  tokensPerSecond: null,
  ttftS: null,
  cachedTokens: 0,
  prefilledTokens: null,
  promptTps: null,
};

const served = (over: Partial<ChatServedBy> = {}): ChatServedBy => ({
  engine: 'remote',
  model: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  where: [MAC],
  peerNodeId: ROUTE.peer ?? null,
  foreign: false,
  contextWindow: 262144,
  phase: 'writing',
  activity: 'generating',
  work: 'thisChat',
  busyWithOthers: null,
  turnRequest: null,
  readTps: null,
  readiness: { kind: 'remote', status: ROUTE },
  ...over,
});
const reconnecting = served({
  phase: 'loading',
  readiness: {
    kind: 'reconnecting',
    status: { ...ROUTE, state: 'reconnecting' },
    why: 'timeout: no answer within 1500 ms',
    cause: null,
    gone: null,
    instead: { kind: 'none' },
  },
});

describe('Q-13: the prompt THIS turn is waiting on, from what the engine actually reports', () => {
  it('Rapid-MLX reports no tokens read: elapsed against its measured read rate — never a percentage', () => {
    const cue = pickTurnCue({
      served: served({ turnRequest: READING, readTps: 318 }),
      inFlight: true,
      lostTo: null,
      silent: false,
    });
    expect(cue).toMatchObject({ kind: 'reading', progress: 'eta', promptTokens: 49293 });
    expect(turnCueText(intl, cue!)).toBe(
      "Work's Mac Studio is reading your prompt (49k tokens) — 1m 12s of about 2m 35s at its measured 318 tok/s"
    );
  });

  it('the engine reports tokens read (the split’s rank 0): that percentage', () => {
    const cue = pickTurnCue({
      served: served({ turnRequest: { ...READING, prefilledTokens: 18732 }, readTps: 318 }),
      inFlight: true,
      lostTo: null,
      silent: false,
    });
    expect(turnCueText(intl, cue!)).toBe(
      "Work's Mac Studio is reading your prompt (49k tokens) — 38%"
    );
  });

  it('past the time its rate needs, or with no rate measured: only the elapsed time', () => {
    expect(readingProgress({ ...READING, elapsedS: 400 }, 318)).toEqual({
      progress: 'elapsed',
      promptTokens: 49293,
      elapsedS: 400,
    });
    expect(readingProgress(READING, null)).toMatchObject({ progress: 'elapsed' });
    expect(readingProgress({ ...READING, phase: 'generation' }, 318)).toBeNull();
    expect(readingProgress(null, 318)).toBeNull();
  });

  it('no turn in flight, or no request of ours: no cue — the default words', () => {
    expect(
      pickTurnCue({
        served: served({ turnRequest: READING }),
        inFlight: false,
        lostTo: null,
        silent: false,
      })
    ).toBeNull();
    expect(
      pickTurnCue({ served: served(), inFlight: true, lostTo: null, silent: false })
    ).toBeNull();
  });
});

describe('Q-55: silence is a multiple of the turn’s OWN cadence', () => {
  it('needs a measured cadence, then waits that many of its median gaps', () => {
    expect(silenceAfterMs([50, 50, 50])).toBeNull();
    const gaps = Array.from({ length: SILENCE_MIN_GAPS }, () => 60);
    expect(silenceAfterMs(gaps)).toBe(60 * SILENCE_GAP_MULTIPLE);
  });
});

function assistant(text: string): Message {
  return {
    id: 'a1',
    role: 'assistant',
    created: 1,
    content: [{ type: 'text', text }],
    metadata: { userVisible: true, agentVisible: true },
  };
}

describe('useTurnCue — one cue from the drop until the turn streams again or ends', () => {
  afterEach(() => vi.useRealTimers());

  it('reconnecting → (the Mac answers) checking, held → gone the moment the stream grows (Q-52)', () => {
    const { result, rerender } = renderHook(
      ({ s, text, state }: { s: ChatServedBy; text: string; state: ChatState }) =>
        useTurnCue(s, state, [assistant(text)]),
      { initialProps: { s: reconnecting, text: 'He climbed', state: ChatState.Streaming } }
    );
    expect(result.current).toEqual({ kind: 'reconnecting', mac: MAC });
    rerender({ s: served(), text: 'He climbed', state: ChatState.Streaming });
    expect(result.current).toEqual({ kind: 'checking', mac: MAC });
    expect(turnCueText(intl, result.current!)).toBe(
      "Checking whether Work's Mac Studio still has your answer…"
    );
    rerender({ s: served(), text: 'He climbed the stairs', state: ChatState.Streaming });
    expect(result.current).toBeNull();
  });

  it('Q-111: a Mac that is away is named so under the composer — "isn’t running" only on its own word', () => {
    const LOST_AT = Date.UTC(2026, 8, 25, 23, 14);
    const away = (gone: PeerGone) =>
      served({
        phase: 'held',
        readiness: {
          ...(reconnecting.readiness as Extract<
            ChatServedBy['readiness'],
            { kind: 'reconnecting' }
          >),
          gone,
        },
      });
    const silent: PeerGone = { because: 'silent', lostSinceMs: LOST_AT, lostForMs: 3_600_000 };
    const { result, rerender } = renderHook(
      ({ s }: { s: ChatServedBy }) => useTurnCue(s, ChatState.Streaming, [assistant('He climbed')]),
      { initialProps: { s: reconnecting } }
    );
    expect(result.current).toEqual({ kind: 'reconnecting', mac: MAC });
    rerender({ s: away(silent) });
    expect(result.current).toEqual({ kind: 'gone', mac: MAC, gone: silent });
    const time = intl.formatTime(LOST_AT, { hour: 'numeric', minute: '2-digit' });
    expect(turnCueText(intl, result.current!)).toBe(
      `Work's Mac Studio hasn’t answered since ${time} — its goose may be closed, or it’s offline`
    );
    rerender({ s: away({ because: 'said-quit' }) });
    expect(turnCueText(intl, result.current!)).toBe(
      "Work's Mac Studio’s goose isn’t running — open goose there, or run chat on this Mac"
    );
    // It comes back on its own: the turn is then checked as after any lost contact.
    rerender({ s: served() });
    expect(result.current).toEqual({ kind: 'checking', mac: MAC });
  });

  it('the turn ends (the dropped-turn notice) — the check ends with it, no lingering', () => {
    const { result, rerender } = renderHook(
      ({ s, state }: { s: ChatServedBy; state: ChatState }) =>
        useTurnCue(s, state, [assistant('He climbed')]),
      { initialProps: { s: reconnecting, state: ChatState.Streaming } }
    );
    rerender({ s: served(), state: ChatState.Streaming });
    expect(result.current?.kind).toBe('checking');
    rerender({ s: served(), state: ChatState.Idle });
    expect(result.current).toBeNull();
  });

  it('a stream that stops past its own cadence says so before any poll — and the next words clear it (Q-55)', () => {
    vi.useFakeTimers();
    let text = 'w';
    const { result, rerender } = renderHook(
      ({ t }: { t: string }) => useTurnCue(served(), ChatState.Streaming, [assistant(t)]),
      { initialProps: { t: text } }
    );
    for (let i = 0; i < SILENCE_MIN_GAPS + 1; i++) {
      act(() => {
        vi.advanceTimersByTime(50);
      });
      text += ' w';
      rerender({ t: text });
    }
    expect(result.current).toBeNull();
    act(() => {
      vi.advanceTimersByTime(50 * SILENCE_GAP_MULTIPLE + 10);
    });
    expect(result.current).toEqual({ kind: 'silent', mac: MAC });
    expect(turnCueText(intl, result.current!)).toBe(
      "No words from Work's Mac Studio for a moment — checking…"
    );
    rerender({ t: `${text} more` });
    expect(result.current).toBeNull();
  });
});
