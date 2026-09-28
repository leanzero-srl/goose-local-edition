import { createContext } from 'react';
import type { LoopRecord } from './model';
import type { SessionLoop } from './useSessionLoop';

/**
 * The door into the Start dialog (§8.2) from the places that are not the composer: the rail's
 * [Start a loop] / [Start a new loop] / [Edit], and a message's "Loop this" (§7.3). The dialog
 * itself is L4's (`StartLoopDialog.tsx`, one per composer); it subscribes here and takes only the
 * requests for its own chat. A request nobody takes answers `false`, and the caller says so in
 * words ("The loop dialog is not in this build yet.") — never a button that silently does nothing.
 */
export interface StartLoopRequest {
  sessionId: string;
  /** `start` opens a new loop (prefilled from `goal` or `from`); `edit` opens the current one. */
  mode: 'start' | 'edit';
  goal?: string;
  /** The loop to prefill from: the ended loop ("Start a new loop") or the one being edited. */
  from?: LoopRecord;
}

/** Answers whether it took the request (a dialog takes only its own chat's). */
type Handler = (request: StartLoopRequest) => boolean;

const handlers = new Set<Handler>();

/** The Start dialog registers here; the returned function unregisters it. */
export function onStartLoopRequest(handler: Handler): () => void {
  handlers.add(handler);
  return () => {
    handlers.delete(handler);
  };
}

/** Whether a dialog took the request. */
export function requestStartLoop(request: StartLoopRequest): boolean {
  let taken = false;
  for (const handler of [...handlers]) {
    if (handler(request)) taken = true;
  }
  return taken;
}

/**
 * What a transcript row and the composer need to know about the chat they sit in: its session and
 * its loop (the tick divider names the loop's cadence and a yielded tick's chat; the composer's
 * Loop slot shows the loop's status and re-reads it after a start). Provided by BaseChat around
 * the conversation and the composer — ONE read of the loop for both; absent elsewhere, where a
 * message offers no "Loop this" and the composer no Loop button.
 */
export interface LoopSessionValue {
  sessionId: string;
  loop: LoopRecord | null;
  /** The loop as read now, with its effective status (`loops/get`). */
  state: SessionLoop;
  /** Read the loop again (after the Start dialog changed it). */
  reload: () => void;
}

export const LoopSessionContext = createContext<LoopSessionValue | null>(null);
