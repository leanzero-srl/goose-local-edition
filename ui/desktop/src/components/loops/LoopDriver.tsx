import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getAcpClient, onAcpConnectionClosed, onLoopsTickDue } from '../../acp/acpConnection';
import { acpChatSessionController } from '../../acp/chatSessionController';
import {
  acpChatSessionActions,
  acpChatSessionStore,
  useAcpChatSessionSnapshot,
} from '../../acp/chatSessionStore';
import { loopsReady, loopsTickRefused } from '../../acp/loops';
import { getPendingUserInput, usePendingUserInput } from './pendingUserInput';
import { createTickDoor, refusalCleared, type RefusalWaitsOn, type TickDoorDeps } from './tickDoor';

/**
 * The window's hands for session loops (DESIGN-SESSION-LOOPS §5.1, §9 L4r), mounted once per
 * window. goosed's runner holds the clock and offers each tick; this takes the offer off the
 * connection and fires it through the same door a typed message uses, or refuses it with the
 * reason and sends `loops/ready` the moment that reason clears — a store or queue event, never a
 * timer. When the connection dies it connects again at once, so goosed sees a live tick door and
 * re-offers what is still open.
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

export function LoopDriver({ deps = liveDeps }: { deps?: TickDoorDeps }) {
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
