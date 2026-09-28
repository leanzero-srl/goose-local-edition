import { useCallback, useEffect, useRef, useState } from 'react';
import { loopsControl, loopsGet } from '../../acp/loops';
import { errorMessage } from '../../utils/conversionUtils';
import type {
  LoopControlAction,
  LoopRecord,
  LoopRefusal,
  LoopStatus,
  LoopStatusReason,
} from './model';

/**
 * The chat's loop as the rail reads it (`loops/get`, a pure read that never claims the clock).
 * Absent loop and absent error = no loop; an unreadable record or a failed read is its own state
 * with its words — never "no loop" (gate 1).
 */
export type SessionLoop =
  | { kind: 'loading' }
  | { kind: 'none' }
  | { kind: 'loop'; loop: LoopRecord; status: LoopStatus; reason?: LoopStatusReason | null }
  | { kind: 'unreadable'; error: string };

export type ControlResult =
  | { kind: 'done'; loop: LoopRecord }
  | { kind: 'refused'; refusal: LoopRefusal }
  | { kind: 'failed'; error: string };

/**
 * `refreshKey` is what the rail already knows changed — the transcript's length and the chat's
 * state (a tick's marker lands, a turn ends) — so the record is re-read on events, never on a
 * timer. The runner's own `loops/changed` notification reaches the rail through L4r's emitter.
 */
export function useSessionLoop(sessionId: string, refreshKey: string) {
  const [state, setState] = useState<SessionLoop>({ kind: 'loading' });
  const asked = useRef(0);

  const read = useCallback(async () => {
    const mine = ++asked.current;
    try {
      const got = await loopsGet(sessionId);
      if (mine !== asked.current) return;
      if (got.error) {
        setState({ kind: 'unreadable', error: got.error });
      } else if (got.loop) {
        setState({
          kind: 'loop',
          loop: got.loop,
          status: got.effectiveStatus ?? got.loop.status,
          reason: got.effectiveStatus ? got.effectiveReason : got.loop.statusReason,
        });
      } else {
        setState({ kind: 'none' });
      }
    } catch (error) {
      if (mine !== asked.current) return;
      setState({ kind: 'unreadable', error: errorMessage(error) });
    }
  }, [sessionId]);

  useEffect(() => {
    setState({ kind: 'loading' });
  }, [sessionId]);

  useEffect(() => {
    void read();
  }, [read, refreshKey]);

  /** Pause / Resume / Stop loop / Run a tick now / Stop check — goosed answers or refuses by name. */
  const control = useCallback(
    async (action: LoopControlAction): Promise<ControlResult> => {
      try {
        const got = await loopsControl(sessionId, action);
        if (got.refusal) return { kind: 'refused', refusal: got.refusal };
        if (!got.loop) {
          return { kind: 'failed', error: 'goose answered with neither a loop nor a refusal' };
        }
        asked.current++;
        setState({
          kind: 'loop',
          loop: got.loop,
          status: got.loop.status,
          reason: got.loop.statusReason,
        });
        return { kind: 'done', loop: got.loop };
      } catch (error) {
        return { kind: 'failed', error: errorMessage(error) };
      }
    },
    [sessionId]
  );

  return { state, control, reload: read };
}
