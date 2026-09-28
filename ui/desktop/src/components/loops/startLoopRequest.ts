import { createContext } from 'react';
import type { LoopRecord } from './model';

/**
 * The door into the Start dialog (§8.2) from the places that are not the composer: the rail's
 * [Start a loop] / [Start a new loop] / [Edit], and a message's "Loop this" (§7.3). The dialog
 * itself is L4's (`StartLoopDialog.tsx`); it subscribes here. A request nobody takes answers
 * `false`, and the caller says so in words ("The loop dialog is not in this build yet.") — never
 * a button that silently does nothing.
 */
export interface StartLoopRequest {
  sessionId: string;
  /** `start` opens a new loop (prefilled from `goal` or `from`); `edit` opens the current one. */
  mode: 'start' | 'edit';
  goal?: string;
  /** The loop to prefill from: the ended loop ("Start a new loop") or the one being edited. */
  from?: LoopRecord;
}

type Handler = (request: StartLoopRequest) => void;

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
  if (handlers.size === 0) return false;
  for (const handler of handlers) handler(request);
  return true;
}

/**
 * What a transcript row needs to know about the chat it sits in: its session and its loop (the
 * tick divider names the loop's cadence and a yielded tick's chat). Provided by BaseChat around
 * the conversation; absent elsewhere, where a message offers no "Loop this".
 */
export interface LoopSessionValue {
  sessionId: string;
  loop: LoopRecord | null;
}

export const LoopSessionContext = createContext<LoopSessionValue | null>(null);
