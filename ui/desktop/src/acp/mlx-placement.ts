import type {
  MlxEngineMeasureSpeedResponse_unstable,
  MlxEnginePlacementPlanResponse_unstable,
  MlxEngineSpeedHistoryResponse_unstable,
  MlxPlacementBadgeDto,
  MlxPlacementCandidateDto,
  MlxPlacementGoalDto,
  MlxPlacementNodeDto,
  MlxPlacementPlanDto,
  MlxSpeedFigureDto,
  MlxSpeedRecordDto,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * Client surface for the placement planner (`_goose/unstable/mlxEngine/{placementPlan,
 * measureSpeed,speedHistory}`, capability `mlxPlacement`): which way of running a model is best on
 * the Macs goose can reach, and "Measure speed". Raw `extMethod` like the distributed surface, so
 * a field a newer backend adds is never stripped by the generated zod parse.
 */

export type PlacementGoal = MlxPlacementGoalDto;
export type PlacementPlan = MlxPlacementPlanDto;
export type PlacementCandidate = MlxPlacementCandidateDto;
export type PlacementBadge = MlxPlacementBadgeDto;
export type PlacementNode = MlxPlacementNodeDto;
export type SpeedFigure = MlxSpeedFigureDto;
export type SpeedRecord = MlxSpeedRecordDto;
export type PlacementPlanResponse = MlxEnginePlacementPlanResponse_unstable;
export type MeasureSpeedResponse = MlxEngineMeasureSpeedResponse_unstable;
export type SpeedHistoryResponse = MlxEngineSpeedHistoryResponse_unstable;

async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

/** Plan one model, or every model in the models folder (`modelId` absent — the picker's badges). */
export async function mlxPlacementPlan(
  goal: PlacementGoal,
  modelId?: string,
  context?: number
): Promise<PlacementPlanResponse> {
  const params: Record<string, unknown> = { goal };
  if (modelId) params.modelId = modelId;
  if (context != null) params.context = context;
  try {
    const response = await call<PlacementPlanResponse>(
      '_goose/unstable/mlxEngine/placementPlan',
      params
    );
    rememberPlacementPlans(response.plans);
    return response;
  } catch (error) {
    publish({ ...seen, failure: error instanceof Error ? error.message : String(error) });
    throw error;
  }
}

/**
 * Every plan goose has answered in this window, by goal and model — the Run it card's plan and the
 * picker's, as they land. The Engine tile reads its measured runs from HERE, the very figures the
 * card draws, so the two can never disagree (Q-123: the tile said "No runs measured yet" beside the
 * card's "~29.6 tok/s measured", because the tile kept its own run book in memory and a relaunch
 * emptied it). goose persists the runs themselves (its measurement store under the data dir); this
 * only holds the latest answer, so nothing here outlives the window or needs a bound.
 */
export interface PlansSeen {
  plans: ReadonlyMap<string, PlacementPlan>;
  /** The last plan call's failure, verbatim; null once a call succeeds. */
  failure: string | null;
}

let seen: PlansSeen = { plans: new Map(), failure: null };
const seenListeners = new Set<() => void>();

function planKey(goal: PlacementGoal, modelId: string): string {
  return `${goal}\n${modelId}`;
}

function publish(next: PlansSeen): void {
  seen = next;
  seenListeners.forEach((listener) => listener());
}

export function latestPlacementPlans(): PlansSeen {
  return seen;
}

export function subscribePlacementPlans(listener: () => void): () => void {
  seenListeners.add(listener);
  return () => {
    seenListeners.delete(listener);
  };
}

export function seenPlan(
  plans: PlansSeen,
  goal: PlacementGoal,
  modelId: string
): PlacementPlan | null {
  return plans.plans.get(planKey(goal, modelId)) ?? null;
}

/** Record plans goose answered (every `mlxPlacementPlan` response lands here). */
export function rememberPlacementPlans(answered: readonly PlacementPlan[]): void {
  const plans = new Map(seen.plans);
  for (const plan of answered) plans.set(planKey(plan.goal, plan.modelId), plan);
  publish({ plans, failure: null });
}

/** Test seam: forget every plan seen. */
export function resetPlacementPlansSeen(): void {
  publish({ plans: new Map(), failure: null });
}

export { measuredFigure, type MeasuredFigure } from '../utils/mlxMeasuredRuns';

/**
 * The fixed, token-counted workload on the RUNNING engine of `placementId`: ~1.9k prompt tokens then
 * 256 greedy tokens (+ a ~30k-token document with `longDocument`). Recorded; the plan then shows it
 * as measured.
 */
export async function mlxMeasureSpeed(
  modelId: string,
  placementId: string,
  longDocument: boolean
): Promise<MeasureSpeedResponse> {
  return await call<MeasureSpeedResponse>('_goose/unstable/mlxEngine/measureSpeed', {
    modelId,
    placementId,
    longDocument,
  });
}

export async function mlxSpeedHistory(modelId?: string): Promise<SpeedHistoryResponse> {
  return await call<SpeedHistoryResponse>(
    '_goose/unstable/mlxEngine/speedHistory',
    modelId ? { modelId } : {}
  );
}

/**
 * The goal's headline figure of a candidate — the one goose ranked it by. Long documents rank by a
 * whole turn (reading the document AND writing the answer, `turn`); a goose that sends no turn
 * figure ranked by reading alone.
 */
export function goalFigure(candidate: PlacementCandidate, goal: PlacementGoal): SpeedFigure | null {
  const speed = candidate.speed;
  const figure =
    goal === 'longDocuments'
      ? (speed.turn ?? speed.prefill)
      : goal === 'manyRequests'
        ? speed.throughput
        : speed.decode;
  return figure ?? null;
}

/** The peer's Link node id of a single placement on a `link:<id>` host; `null` otherwise. */
export function linkPeerOf(candidate: PlacementCandidate): string | null {
  if (candidate.key.kind !== 'single') return null;
  const host = candidate.key.nodes[0] ?? '';
  return host.startsWith('link:') ? host.slice('link:'.length) : null;
}
