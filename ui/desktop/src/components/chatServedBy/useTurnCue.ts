import { useEffect, useRef, useState } from 'react';
import { ChatState } from '../../types/chatState';
import type { Message } from '../../types/message';
import { reconnectingMac, type ChatServedBy } from './chatServedBy';
import {
  SILENCE_WINDOW_GAPS,
  pickTurnCue,
  silenceAfterMs,
  streamOnWords,
  streamSize,
  type TurnCue,
} from './turnStatus';

/**
 * The status line's cue for THIS chat's turn (turnStatus.ts): holds the two facts only time can
 * give — contact lost during the turn with no stream since, and the stream's own cadence — and
 * picks the cue. The silence timer is a display cue; it ends nothing.
 */
export function useTurnCue(
  served: ChatServedBy | null,
  chatState: ChatState,
  messages: Message[]
): TurnCue | null {
  const inFlight = chatState === ChatState.Streaming || chatState === ChatState.Thinking;
  const last = messages[messages.length - 1];
  const size = streamSize(last);
  const turnKey = last?.role === 'assistant' ? (last.id ?? null) : null;
  const reconnecting = served ? reconnectingMac(served) : null;

  const [lost, setLost] = useState<{ mac: string; mark: number } | null>(null);
  useEffect(() => {
    if (!inFlight) {
      setLost(null);
      return;
    }
    if (reconnecting) {
      setLost((prev) => prev ?? { mac: reconnecting, mark: size });
      return;
    }
    setLost((prev) => (prev && size > prev.mark ? null : prev));
  }, [inFlight, reconnecting, size]);

  const cadence = useRef<{ key: string | null; size: number; at: number; gaps: number[] }>({
    key: null,
    size: 0,
    at: 0,
    gaps: [],
  });
  const [silent, setSilent] = useState(false);
  const watching = inFlight && chatState === ChatState.Streaming && streamOnWords(last);
  useEffect(() => {
    const now = Date.now();
    const c = cadence.current;
    if (c.key !== turnKey) {
      cadence.current = { key: turnKey, size, at: now, gaps: [] };
    } else if (size > c.size) {
      c.gaps.push(now - c.at);
      if (c.gaps.length > SILENCE_WINDOW_GAPS) c.gaps.shift();
      c.size = size;
      c.at = now;
    }
    setSilent(false);
    if (!watching) return undefined;
    const after = silenceAfterMs(cadence.current.gaps);
    if (after == null) return undefined;
    const timer = setTimeout(() => setSilent(true), after);
    return () => clearTimeout(timer);
  }, [turnKey, size, watching]);

  return pickTurnCue({ served, inFlight, lostTo: lost?.mac ?? null, silent: silent && watching });
}
