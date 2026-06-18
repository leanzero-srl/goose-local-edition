type SessionScopedNotificationListener<TNotification> = (
  notification: TNotification
) => Promise<void> | void;

interface SessionScopedNotification {
  sessionId: string;
}

export function createSessionScopedNotificationRouter<
  TNotification extends SessionScopedNotification,
>() {
  const listenersBySessionId = new Map<
    string,
    Set<SessionScopedNotificationListener<TNotification>>
  >();

  const subscribe = (
    sessionId: string,
    listener: SessionScopedNotificationListener<TNotification>
  ): (() => void) => {
    const listeners = listenersBySessionId.get(sessionId) ?? new Set();
    listeners.add(listener);
    listenersBySessionId.set(sessionId, listeners);

    let subscribed = true;

    return () => {
      if (!subscribed) {
        return;
      }

      subscribed = false;
      const currentListeners = listenersBySessionId.get(sessionId);
      if (!currentListeners) {
        return;
      }

      currentListeners.delete(listener);
      if (currentListeners.size === 0) {
        listenersBySessionId.delete(sessionId);
      }
    };
  };

  const route = async (notification: TNotification): Promise<boolean> => {
    const listeners = listenersBySessionId.get(notification.sessionId);
    if (!listeners) {
      return false;
    }

    await Promise.all([...listeners].map((listener) => listener(notification)));
    return true;
  };

  return {
    route,
    subscribe,
  };
}
