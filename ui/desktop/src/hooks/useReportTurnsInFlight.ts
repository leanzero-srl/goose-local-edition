import { useEffect } from 'react';
import { acpChatSessionStore, watchPromptsInFlight } from '../acp/chatSessionStore';
import { acpChatSessionController } from '../acp/chatSessionController';
import type { TurnInFlight } from '../utils/closeGuard';
import { STOP_TURN_CHANNEL } from '../utils/runningElsewhere';

interface TurnsBridge {
  reportTurnsInFlight?: (turns: TurnInFlight[]) => void;
  on?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
  off?: (channel: string, fn: (event: unknown, ...args: unknown[]) => void) => void;
}

/**
 * Another window asked to stop a chat whose prompt THIS window's connection holds (Q-500): goosed's
 * cancel reaches a prompt only through the connection that sent it, so the stop is this window's own
 * Stop. A chat with no prompt here any more is left alone — its turn already ended.
 */
export function stopTurnForAnotherWindow(sessionId: unknown): void {
  if (typeof sessionId !== 'string') return;
  const attempt = acpChatSessionStore.getSnapshot(sessionId)?.activePromptAttemptId;
  if (attempt === null || attempt === undefined) return;
  acpChatSessionController.stop(sessionId);
}

/**
 * Tells main which chats have a prompt in flight on THIS window's ACP connection, whenever that set
 * changes (Q-490). main's close guard asks before closing a window that holds one: the close ends
 * the connection, and goosed drops the prompt mid-answer. main also hands the set to the other
 * windows (Q-500), and relays their Stop for one of these prompts here.
 */
export function useReportTurnsInFlight(): void {
  useEffect(() => {
    const electron = (window as unknown as { electron?: TurnsBridge }).electron;
    const report = electron?.reportTurnsInFlight;
    if (!report) return;
    const onStop = (_event: unknown, ...args: unknown[]) => stopTurnForAnotherWindow(args[0]);
    electron.on?.(STOP_TURN_CHANNEL, onStop);
    const unwatch = watchPromptsInFlight((prompts) => report(prompts));
    return () => {
      unwatch();
      electron.off?.(STOP_TURN_CHANNEL, onStop);
    };
  }, []);
}
