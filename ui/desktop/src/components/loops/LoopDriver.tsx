import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getAcpClient, onAcpConnectionClosed, onLoopsTickDue } from '../../acp/acpConnection';
import { acpChatSessionController } from '../../acp/chatSessionController';
import {
  acpChatSessionActions,
  acpChatSessionStore,
  useAcpChatSessionSnapshot,
} from '../../acp/chatSessionStore';
import { loopsReady, loopsTickRefused, loopsWake, type LoopsWake } from '../../acp/loops';
import { getPendingUserInput, usePendingUserInput } from './pendingUserInput';
import { createTickDoor, refusalCleared, type RefusalWaitsOn, type TickDoorDeps } from './tickDoor';

/**
 * The window's hands for session loops (DESIGN-SESSION-LOOPS §5.1, §9 L4r), mounted once per
 * window. goosed's runner holds the clock and offers each tick; this takes the offer off the
 * connection and fires it through the same door a typed message uses, or refuses it with the
 * reason and sends `loops/ready` the moment that reason clears — a store or queue event, never a
 * timer. When the connection dies it connects again at once, so goosed sees a live tick door and
 * re-offers what is still open. When the Mac wakes from sleep (main's `system-resumed`, L10) it
 * sends `loops/wake` so the runner re-reads the wall clock.
 */

const liveDeps: TickDoorDeps = {
  getSnapshot: (sessionId) => acpChatSessionStore.getSnapshot(sessionId),
  loadSession: (sessionId) => acpChatSessionController.loadSession(sessionId),
  submitMessage: (sessionId, message, options) =>
    acpChatSessionController.submitMessage(sessionId, message, options),
  setMessages: (sessionId, messages) => {
    acpChatSessionActions.setMessages(sessionId, messages);
  },
  pendingUserInput: getPendingUserInput,
  tickRefused: async (sessionId, loopId, n, reason) => {
    await loopsTickRefused(sessionId, loopId, n, reason);
  },
};

/** The Mac's wake and the call that hands it to goosed. */
export interface WakeDeps {
  onSystemResumed(listener: () => void): () => void;
  wake(): Promise<LoopsWake>;
}

const liveWake: WakeDeps = {
  onSystemResumed: (listener) => window.electron.onSystemResumed(listener),
  wake: loopsWake,
};

// One `loops/wake` per resume, every time: two quick resumes are two calls and the runner decides.
// The runner's `wake` (runner.rs) holds its op lock, skips a loop that is not Waiting or already
// has an offer out, and only relabels a due next tick `on_wake` — so a second call, or one from
// another window on the same goosed, either finds the offer out or rewrites the same label: one
// tick either way. A debounce here would be a clock guessing for it.
function forwardWake(wake: WakeDeps): void {
  wake.wake().then(
    (result) => {
      if (result.refusal) {
        console.warn(
          `goose did not re-read its loop clocks after the Mac woke (${result.refusal.code}): ${result.refusal.reason}`
        );
      }
    },
    (error) => {
      console.error('Could not tell goose that the Mac woke; loop ticks wait for their clock:', error);
    }
  );
}

function connect(why: string): void {
  getAcpClient().catch((error) => {
    console.error(
      `Could not reconnect to goose ${why}; loop ticks wait for the next connection:`,
      error
    );
  });
}

function ReadyWatcher({
  sessionId,
  waitsOn,
  onCleared,
}: {
  sessionId: string;
  waitsOn: Exclude<RefusalWaitsOn, null>;
  onCleared(sessionId: string): void;
}) {
  const snapshot = useAcpChatSessionSnapshot(sessionId);
  const queued = usePendingUserInput(sessionId);
  const cleared = refusalCleared(waitsOn, snapshot, queued);

  useEffect(() => {
    if (cleared) onCleared(sessionId);
  }, [cleared, onCleared, sessionId]);

  return null;
}

export function LoopDriver({
  deps = liveDeps,
  wake = liveWake,
}: {
  deps?: TickDoorDeps;
  wake?: WakeDeps;
}) {
  const door = useMemo(() => createTickDoor(deps), [deps]);
  const [refused, setRefused] = useState<ReadonlyMap<string, Exclude<RefusalWaitsOn, null>>>(
    () => new Map()
  );
  const readySent = useRef(new Set<string>());

  useEffect(
    () =>
      onLoopsTickDue((offer) => {
        void door.offer(offer).then((outcome) => {
          if (outcome.kind !== 'refused' || outcome.waitsOn === null) return;
          const waitsOn = outcome.waitsOn;
          readySent.current.delete(offer.sessionId);
          setRefused((current) => new Map(current).set(offer.sessionId, waitsOn));
        });
      }),
    [door]
  );

  useEffect(() => wake.onSystemResumed(() => forwardWake(wake)), [wake]);

  useEffect(() => {
    connect('for the loop driver');
    return onAcpConnectionClosed(() => connect('after the connection closed'));
  }, []);

  const onCleared = useCallback((sessionId: string) => {
    if (readySent.current.has(sessionId)) return;
    readySent.current.add(sessionId);
    setRefused((current) => {
      const next = new Map(current);
      next.delete(sessionId);
      return next;
    });
    loopsReady(sessionId).catch((error) => {
      console.error(`Could not tell goose that ${sessionId} is ready for its loop tick:`, error);
    });
  }, []);

  return (
    <>
      {[...refused].map(([sessionId, waitsOn]) => (
        <ReadyWatcher
          key={`${sessionId}:${waitsOn}`}
          sessionId={sessionId}
          waitsOn={waitsOn}
          onCleared={onCleared}
        />
      ))}
    </>
  );
}
