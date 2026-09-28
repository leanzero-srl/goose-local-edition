import { useCallback, useSyncExternalStore } from 'react';
import { setPendingAnswers } from '../loops/pendingUserInput';
import { answerMessage, resolveNeedsYou, type NeedsYouItemDto } from './sessionActivityStore';

/**
 * Q-341: an answer given on a needs-you card while the chat's turn runs is QUEUED here and sent when
 * the turn ends — the composer's queue is the model (a person can act while goose works; nothing is
 * interrupted). An answer reaches the model as the person's next message (`sendAnswer`), and the
 * chat takes no new message while a turn runs (`handleSubmit` refuses one), so answering at once
 * would close the question on the engine while the words it closed with went nowhere.
 *
 * THE ORDER at the turn's end (tested in ChatInput.needsYouOrder.test.tsx):
 *  1. every queued answer, oldest first, is resolved on the engine (Answered, with its words);
 *  2. ONE message carries them all to the model, marked with every item it answers (Q-344: an
 *     unmarked message is read as typed, and Q-298 closes every question open when one arrives);
 *  3. only then do the composer's queued messages go, each after the turn before it ends — the
 *     composer is held (`answersWaiting`) from the moment an answer is queued until its message has
 *     started a turn. Composer first would let its message supersede the very question the person
 *     answered on the card, and the answer would be refused as already closed.
 *
 * Kept per chat for the window's life, so leaving the chat and coming back keeps what was queued;
 * the tick door waits on it like on the composer's queue (`setPendingAnswers`).
 */

/** Sends answers to the model as one chat message, marked with the items it answers (Q-344). */
export type SendAnswer = (text: string, answered: readonly string[]) => void;

export interface QueuedAnswer {
  itemId: string;
  sessionId: string;
  question: string;
  answer: string;
}

/** An answer that was queued and could not be sent — shown until the person closes it. */
export interface UnsentAnswer extends QueuedAnswer {
  reason: 'closed' | 'failed';
  error: string | null;
}

export interface AnswerQueue {
  queued: QueuedAnswer[];
  /** The items whose queued answers are being resolved and sent right now. */
  sending: string[];
  unsent: UnsentAnswer[];
}

const EMPTY: AnswerQueue = { queued: [], sending: [], unsent: [] };

const queues = new Map<string, AnswerQueue>();
const listeners = new Map<string, Set<() => void>>();

export function getAnswerQueue(sessionId: string): AnswerQueue {
  return queues.get(sessionId) ?? EMPTY;
}

function update(sessionId: string, next: AnswerQueue): void {
  if (next.queued.length === 0 && next.sending.length === 0 && next.unsent.length === 0) {
    queues.delete(sessionId);
  } else {
    queues.set(sessionId, next);
  }
  setPendingAnswers(sessionId, next.queued.length);
  for (const listener of [...(listeners.get(sessionId) ?? [])]) listener();
}

/** Something the composer must wait behind: a queued answer, or one being sent. */
export function answersWaiting(queue: AnswerQueue): boolean {
  return queue.queued.length > 0 || queue.sending.length > 0;
}

/** A second answer to the same question replaces the first, in its place in the order. */
export function enqueueAnswer(
  item: Pick<NeedsYouItemDto, 'id' | 'sessionId' | 'question'>,
  answer: string
): void {
  const queue = getAnswerQueue(item.sessionId);
  const entry: QueuedAnswer = {
    itemId: item.id,
    sessionId: item.sessionId,
    question: item.question,
    answer,
  };
  const at = queue.queued.findIndex((queued) => queued.itemId === item.id);
  const queued =
    at === -1
      ? [...queue.queued, entry]
      : queue.queued.map((queued, i) => (i === at ? entry : queued));
  update(item.sessionId, { ...queue, queued });
}

/** Take a queued answer back; returns it so the card can put its words back in the text box. */
export function cancelQueuedAnswer(sessionId: string, itemId: string): QueuedAnswer | null {
  const queue = getAnswerQueue(sessionId);
  const entry = queue.queued.find((queued) => queued.itemId === itemId) ?? null;
  if (entry) {
    update(sessionId, {
      ...queue,
      queued: queue.queued.filter((queued) => queued.itemId !== itemId),
    });
  }
  return entry;
}

export function closeUnsentAnswer(sessionId: string, itemId: string): void {
  const queue = getAnswerQueue(sessionId);
  update(sessionId, {
    ...queue,
    unsent: queue.unsent.filter((unsent) => unsent.itemId !== itemId),
  });
}

/** Several answers in one message, each in the words a single answer carries. */
export function answersMessage(answers: readonly Pick<QueuedAnswer, 'question' | 'answer'>[]) {
  return answers.map((entry) => answerMessage(entry.question, entry.answer)).join('\n\n');
}

/**
 * Send what waits (the order above). `openIds` = this chat's questions the engine lists as open
 * now: an answer whose question closed meanwhile (dismissed elsewhere, superseded) is not sent and
 * says so. Returns once the message was handed to the chat (or nothing could be sent).
 */
export async function deliverQueuedAnswers(
  sessionId: string,
  openIds: ReadonlySet<string>,
  sendAnswer: SendAnswer
): Promise<void> {
  const start = getAnswerQueue(sessionId);
  if (start.queued.length === 0 || start.sending.length > 0) return;
  const batch = start.queued;
  update(sessionId, { ...start, sending: batch.map((entry) => entry.itemId) });

  const delivered: QueuedAnswer[] = [];
  const unsent: UnsentAnswer[] = [];
  for (const entry of batch) {
    if (!openIds.has(entry.itemId)) {
      unsent.push({ ...entry, reason: 'closed', error: null });
      continue;
    }
    try {
      await resolveNeedsYou({ id: entry.itemId, sessionId }, 'answer', entry.answer);
      delivered.push(entry);
    } catch (e) {
      unsent.push({
        ...entry,
        reason: 'failed',
        error: e instanceof Error ? e.message : String(e),
      });
    }
  }
  // The message goes BEFORE the composer is released: sending starts the turn synchronously, so
  // the composer's queue sees a busy chat, never an idle one it could send into first.
  if (delivered.length > 0) {
    sendAnswer(
      answersMessage(delivered),
      delivered.map((entry) => entry.itemId)
    );
  }

  const now = getAnswerQueue(sessionId);
  update(sessionId, {
    // An answer re-picked while this batch was sent is a new entry and stays queued.
    queued: now.queued.filter((entry) => !batch.includes(entry)),
    sending: [],
    unsent: [
      ...now.unsent.filter((old) => !unsent.some((u) => u.itemId === old.itemId)),
      ...unsent,
    ],
  });
}

export function useAnswerQueue(sessionId: string): AnswerQueue {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const set = listeners.get(sessionId) ?? new Set<() => void>();
      set.add(onChange);
      listeners.set(sessionId, set);
      return () => {
        set.delete(onChange);
        if (set.size === 0 && listeners.get(sessionId) === set) listeners.delete(sessionId);
      };
    },
    [sessionId]
  );
  return useSyncExternalStore(subscribe, () => getAnswerQueue(sessionId));
}

export function resetAnswerQueuesForTests(): void {
  for (const sessionId of [...queues.keys()]) update(sessionId, EMPTY);
}
