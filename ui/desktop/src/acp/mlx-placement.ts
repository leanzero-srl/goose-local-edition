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
  return await call<PlacementPlanResponse>('_goose/unstable/mlxEngine/placementPlan', params);
}

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

/** The goal's headline figure of a candidate. */
export function goalFigure(
  candidate: PlacementCandidate,
  goal: PlacementGoal
): SpeedFigure | null {
  const speed = candidate.speed;
  const figure =
    goal === 'longDocuments'
      ? speed.prefill
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
