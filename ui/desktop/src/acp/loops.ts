import type {
  LoopControlAction,
  LoopEdit,
  LoopRefuseReason,
  LoopsChangeResponse_unstable,
  LoopsGetResponse_unstable,
  LoopsListResponse_unstable,
  LoopsReadyResponse_unstable,
  LoopsStartRequest_unstable,
  LoopsTemplatesResponse_unstable,
  LoopsTickRefusedResponse_unstable,
  LoopsWakeResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for session loops (`_goose/unstable/loops/*`, design DESIGN-SESSION-LOOPS.md
 * §9 L0). The whole contract lives here and in goosed's `custom_requests/loops.rs`: the rail, the
 * start dialog, the composer's `/loop` controls and the tick driver call these and never write the
 * `loop.v0` record another way. Raw `extMethod` like the nodes surface, so a field a newer backend
 * adds is never stripped by the generated zod parse.
 *
 * Every mutation answers `{ loop }` or a named `refusal` (nothing written). Until the loop runner
 * is in the build, that refusal is "The loop runner is not in this build" — shown as it is.
 */

export type LoopsGet = LoopsGetResponse_unstable;
export type LoopsChange = LoopsChangeResponse_unstable;
export type LoopsList = LoopsListResponse_unstable;
export type LoopsTemplates = LoopsTemplatesResponse_unstable;
export type LoopsTickRefused = LoopsTickRefusedResponse_unstable;
export type LoopsReady = LoopsReadyResponse_unstable;
export type LoopsWake = LoopsWakeResponse_unstable;
export type LoopsStart = LoopsStartRequest_unstable;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/**
 * The chat's loop, as read now — a pure read that never claims the clock. `loop` and `error` both
 * absent = no loop; an unreadable record comes back as `error`, never as "no loop".
 */
export async function loopsGet(sessionId: string): Promise<LoopsGet> {
  return call<LoopsGet>('_goose/unstable/loops/get', { sessionId });
}

/** Start (or replace) the chat's loop; the first tick runs now. */
export async function loopsStart(start: LoopsStart): Promise<LoopsChange> {
  return call<LoopsChange>('_goose/unstable/loops/start', { ...start });
}

/** Save the Edit dialog (the whole editable set). Editing does not start a tick. */
export async function loopsUpdate(sessionId: string, patch: LoopEdit): Promise<LoopsChange> {
  return call<LoopsChange>('_goose/unstable/loops/update', { sessionId, patch });
}

/** Pause / Resume / Stop loop / Run a tick now / Stop check. */
export async function loopsControl(
  sessionId: string,
  action: LoopControlAction
): Promise<LoopsChange> {
  return call<LoopsChange>('_goose/unstable/loops/control', { sessionId, action });
}

/** The window could not submit an offered tick now; `loopsReady` follows when that clears. */
export async function loopsTickRefused(
  sessionId: string,
  loopId: string,
  n: number,
  reason: LoopRefuseReason
): Promise<LoopsTickRefused> {
  return call<LoopsTickRefused>('_goose/unstable/loops/tickRefused', {
    sessionId,
    loopId,
    n,
    reason,
  });
}

/** What refused an offer for this chat has cleared: re-send the open offer. */
export async function loopsReady(sessionId: string): Promise<LoopsReady> {
  return call<LoopsReady>('_goose/unstable/loops/ready', { sessionId });
}

/** The Mac woke: one tick if any came due, never a burst. */
export async function loopsWake(): Promise<LoopsWake> {
  return call<LoopsWake>('_goose/unstable/loops/wake', {});
}

/** The four starting templates, their steps with slots, suggested cadence and check need. */
export async function loopsTemplates(): Promise<LoopsTemplates> {
  return call<LoopsTemplates>('_goose/unstable/loops/templates', {});
}

/** Every chat's loop with its status as read now (ended ones included). A pure read. */
export async function loopsList(): Promise<LoopsList> {
  return call<LoopsList>('_goose/unstable/loops/list', {});
}
