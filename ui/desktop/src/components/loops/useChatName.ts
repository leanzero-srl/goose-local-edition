import { useEffect, useState } from 'react';
import { acpGetSessionListItem } from '../../acp/sessions';
import { AppEvents } from '../../constants/events';

/**
 * The name a chat carries NOW, for a loop line that names it (Q-279). A yielded tick's record keeps
 * the name the chat had at the yield, and a chat a user has just opened is still "New Chat" then —
 * goose names it after its first turn — so the record's name goes stale within a minute. The name
 * is read when the line is shown and follows every rename after; the recorded name is shown only
 * until that read lands, or when the chat can no longer be read (said in the console, not hidden).
 */
export function useChatName(sessionId: string, recorded: string): string {
  const [current, setCurrent] = useState<{ sessionId: string; name: string } | null>(null);

  useEffect(() => {
    if (!sessionId) return;
    let live = true;
    acpGetSessionListItem(sessionId)
      .then((item) => {
        if (live) setCurrent({ sessionId, name: item.name });
      })
      .catch((error: unknown) => {
        console.warn(
          `loop: chat ${sessionId} could not be read; showing the name it had when the tick yielded to it`,
          error
        );
      });
    const onRenamed = (event: Event) => {
      const detail = (event as CustomEvent<{ sessionId: string; newName: string }>).detail;
      if (detail?.sessionId === sessionId) setCurrent({ sessionId, name: detail.newName });
    };
    window.addEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    return () => {
      live = false;
      window.removeEventListener(AppEvents.SESSION_RENAMED, onRenamed);
    };
  }, [sessionId]);

  return current?.sessionId === sessionId ? current.name : recorded;
}
