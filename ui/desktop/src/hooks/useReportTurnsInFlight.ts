import { useEffect } from 'react';
import { watchPromptsInFlight } from '../acp/chatSessionStore';
import type { TurnInFlight } from '../utils/closeGuard';

interface TurnsBridge {
  reportTurnsInFlight?: (turns: TurnInFlight[]) => void;
}

/**
 * Tells main which chats have a prompt in flight on THIS window's ACP connection, whenever that set
 * changes (Q-490). main's close guard asks before closing a window that holds one: the close ends
 * the connection, and goosed drops the prompt mid-answer.
 */
export function useReportTurnsInFlight(): void {
  useEffect(() => {
    const report = (window as unknown as { electron?: TurnsBridge }).electron?.reportTurnsInFlight;
    if (!report) return;
    return watchPromptsInFlight((prompts) => report(prompts));
  }, []);
}
