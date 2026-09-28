import type {
  NodeEnsureServing,
  NodesBuildEligibilityResponse_unstable,
  NodesConfig,
  NodeStrategy,
  NodesEnsureServingResponse_unstable,
  NodesLoadHistoryResponse_unstable,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
  NodesServedLastResponse_unstable,
  NodesWriteResponse_unstable,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for nodes and strategies (`_goose/unstable/nodes/*`, design
 * DESIGN-NODES-AND-STRATEGIES.md §4.2). The whole contract lives here and in goosed's
 * `custom_requests/nodes.rs`: every later slice calls these and never opens another door to the
 * `nodes` config key (the desktop never upserts it raw). Raw `extMethod` like the placement
 * surface, so a field a newer backend adds is never stripped by the generated zod parse.
 */

export type NodesRead = NodesReadResponse_unstable;
export type NodesWrite = NodesWriteResponse_unstable;
export type BuildEligibility = NodesBuildEligibilityResponse_unstable;
export type Residency = NodesResidencyResponse_unstable;
export type LoadHistory = NodesLoadHistoryResponse_unstable;
export type ServedLast = NodesServedLastResponse_unstable;
export type EnsureServing = NodeEnsureServing;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/** The config with the pool adopted and every def resolved. Never writes config. */
export async function nodesRead(): Promise<NodesRead> {
  return call<NodesRead>('_goose/unstable/nodes/read', {});
}

/**
 * Validate and store the whole config. Refusals come back in `refusals` (nothing written), each
 * shown verbatim. A change of `forNewChats` also sets the global defaults in the same call.
 */
export async function nodesWrite(config: NodesConfig): Promise<NodesWrite> {
  return call<NodesWrite>('_goose/unstable/nodes/write', { config });
}

export interface RemoveNodeOptions {
  /** "Remove from those strategies too". */
  alsoFromStrategies?: boolean;
  /** "and start new chats on Any node (Auto)". */
  andNewChatsAuto?: boolean;
  /** The count of live chats set to this node the person acknowledged. */
  acknowledgedSessions?: number;
}

export async function nodesRemoveNode(
  id: string,
  options: RemoveNodeOptions = {}
): Promise<NodesWrite> {
  return call<NodesWrite>('_goose/unstable/nodes/removeNode', { id, ...options });
}

export interface RemoveStrategyOptions {
  andNewChatsAuto?: boolean;
  andBuildsPool?: boolean;
}

export async function nodesRemoveStrategy(
  id: string,
  options: RemoveStrategyOptions = {}
): Promise<NodesWrite> {
  return call<NodesWrite>('_goose/unstable/nodes/removeStrategy', { id, ...options });
}

/**
 * Whether a strategy can drive a swarm build, and every reason when it cannot. With `draft`, goosed
 * answers for the strategy as the editor holds it (in place of the stored one, or as a new one) —
 * checked while it is edited, never written (Q-311).
 */
export async function nodesBuildEligibility(
  strategy: string,
  draft?: NodeStrategy
): Promise<BuildEligibility> {
  return call<BuildEligibility>(
    '_goose/unstable/nodes/buildEligibility',
    draft ? { strategy, draft } : { strategy }
  );
}

/** Per node: serving / loading / waiting / not running / refused last time. */
export async function nodesResidency(): Promise<Residency> {
  return call<Residency>('_goose/unstable/nodes/residency', {});
}

/** The measured loads of a node's model, way and Macs; a median is absent until one is measured. */
export async function nodesLoadHistory(node: string): Promise<LoadHistory> {
  return call<LoadHistory>('_goose/unstable/nodes/loadHistory', { node });
}

/** The last served-turn record of a session (read at the end of each turn, never polled). */
export async function nodesServedLast(sessionId: string): Promise<ServedLast> {
  return call<ServedLast>('_goose/unstable/nodes/servedLast', { sessionId });
}

/** Make a node servable: ready, wait(reason) or refused(code, reason). */
export async function nodesEnsureServing(node: string, sessionId?: string): Promise<EnsureServing> {
  const params: Record<string, unknown> = { node };
  if (sessionId) params.sessionId = sessionId;
  const response = await call<NodesEnsureServingResponse_unstable>(
    '_goose/unstable/nodes/ensureServing',
    params
  );
  return response.answer;
}
