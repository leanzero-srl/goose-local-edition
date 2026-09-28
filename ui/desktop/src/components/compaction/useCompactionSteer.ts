import { useCallback, useEffect, useRef, useState } from 'react';
import { compactionPreview, compactionSteer, type CompactionSteer } from '../../acp/compaction';
import { errorMessage } from '../../utils/conversionUtils';
import {
  announceCompactionSteerChanged,
  onCompactionSteerChanged,
} from '../contextRail/contextRailRequest';

/**
 * The person's note and pins for this chat's compactions, read from and written to goosed
 * (`compaction.v0`). A read or a write that fails is its own state with its words — never an empty
 * note (gate 1).
 */
export type SteerState =
  | { kind: 'loading' }
  | { kind: 'ready'; steer: CompactionSteer; readError?: string }
  | { kind: 'unreadable'; error: string };

const EMPTY: CompactionSteer = { standing: false, pins: [], followAsWritten: false };

export function useCompactionSteer(sessionId: string | null | undefined) {
  const [state, setState] = useState<SteerState>({ kind: 'loading' });
  const [saveError, setSaveError] = useState<string | null>(null);
  const asked = useRef(0);

  const read = useCallback(async () => {
    if (!sessionId) return;
    const mine = ++asked.current;
    try {
      const preview = await compactionPreview(sessionId);
      if (mine !== asked.current) return;
      setState({
        kind: 'ready',
        steer: preview.steer ?? EMPTY,
        ...(preview.steerError ? { readError: preview.steerError } : {}),
      });
    } catch (error) {
      if (mine !== asked.current) return;
      setState({ kind: 'unreadable', error: errorMessage(error) });
    }
  }, [sessionId]);

  useEffect(() => {
    setState({ kind: 'loading' });
    void read();
  }, [read]);

  useEffect(
    () =>
      onCompactionSteerChanged((changed) => {
        if (changed === sessionId) void read();
      }),
    [read, sessionId]
  );

  /** Saves `patch` over what goosed holds now; answers whether it was saved. */
  const save = useCallback(
    async (patch: Partial<CompactionSteer>): Promise<boolean> => {
      if (!sessionId) return false;
      try {
        // A patch goes over what goosed holds — read it first when this window has not yet, or
        // the note would be written over with nothing.
        const base =
          state.kind === 'ready'
            ? state.steer
            : ((await compactionPreview(sessionId)).steer ?? EMPTY);
        const saved = await compactionSteer(sessionId, { ...base, ...patch });
        asked.current++;
        setState({ kind: 'ready', steer: saved });
        setSaveError(null);
        announceCompactionSteerChanged(sessionId);
        return true;
      } catch (error) {
        setSaveError(errorMessage(error));
        return false;
      }
    },
    [sessionId, state]
  );

  return { state, save, saveError, reload: read };
}
