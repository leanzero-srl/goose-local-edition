import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import {
  linkPeerOf,
  measuredFigure,
  seenPlan,
  type MeasuredFigure,
  type PlacementCandidate,
  type PlacementGoal,
  type PlacementPlan,
  type PlansSeen,
} from '../../acp/mlx-placement';

/**
 * The Engine tile's measured runs, read from the SAME figures the Run it card draws: goose's plan for
 * the model, the candidate that is the way the engine runs now (this Mac, a linked Mac, the split).
 * goose keeps every measured run per model and per way in its measurement store under its data dir
 * (goose-sidecar `placement::store`, `mlx-speed-measurements.jsonl`), so a relaunch loses nothing;
 * the tile's old in-memory run book lost them all (Q-123).
 */
export type MeasuredRuns =
  /** No plan for this model has answered yet in this window. */
  | { kind: 'pending' }
  /** The runs cannot be read — the words say why, verbatim where goose gave them. */
  | { kind: 'unread'; detail: string }
  | { kind: 'read'; writing: MeasuredFigure | null; reading: MeasuredFigure | null };

/** Which way the engine the tile shows is running, as the plan keys ways. */
export type EngineWay =
  | { kind: 'thisMac'; modelId: string | null }
  | { kind: 'peer'; modelId: string | null; peer: string | null }
  | { kind: 'split'; status: MlxDistributedStatus };

export function engineWayOf(
  dist: MlxDistributedStatus | null,
  remote: MlxRemoteSingleStatus | null,
  localModelId: string | null
): EngineWay {
  if (dist) return { kind: 'split', status: dist };
  if (remote) return { kind: 'peer', modelId: remote.modelId ?? null, peer: remote.peer ?? null };
  return { kind: 'thisMac', modelId: localModelId };
}

/** The split's runner in the plan's words (goose `mlx_speed.rs` records it the same way). */
const RUNNER_KIND: Record<string, 'tensor' | 'pipeline'> = {
  mlxLmTensor: 'tensor',
  pipelineQwen4: 'pipeline',
};

function sameNodes(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((node, i) => node === b[i]);
}

type Match = { ok: true; candidate: PlacementCandidate } | { ok: false; detail: string };

function candidateFor(plan: PlacementPlan, way: EngineWay): Match {
  const candidates = plan.candidates ?? [];
  const found = (candidate: PlacementCandidate | undefined, missing: string): Match =>
    candidate ? { ok: true, candidate } : { ok: false, detail: missing };
  switch (way.kind) {
    case 'thisMac':
      return found(
        candidates.find((c) => c.key.kind === 'single' && c.key.nodes[0] === 'local'),
        "goose's plan for this model has no this-Mac way"
      );
    case 'peer':
      if (way.peer == null) return { ok: false, detail: 'the route names no linked Mac' };
      return found(
        candidates.find((c) => linkPeerOf(c) === way.peer),
        `goose's plan for this model has no way on the linked Mac ${way.peer}`
      );
    case 'split': {
      const { status } = way;
      const kind = RUNNER_KIND[status.runner ?? ''];
      const nodes = status.config?.nodes.map((node) => node.ssh ?? 'local');
      const link = status.config?.backend ?? status.backend ?? null;
      if (!kind || !nodes) {
        return {
          ok: false,
          detail: `the split reports ${kind ? 'no node list' : `runner "${status.runner ?? ''}"`}, which goose's plan does not key`,
        };
      }
      return found(
        candidates.find(
          (c) =>
            c.key.kind === kind && sameNodes(c.key.nodes, nodes) && (c.key.link ?? null) === link
        ),
        `goose's plan for this model has no ${kind} split over ${nodes.join(' + ')}`
      );
    }
  }
}

function modelOf(way: EngineWay): string | null {
  return way.kind === 'split' ? (way.status.modelId ?? null) : way.modelId;
}

/**
 * The runs goose measured for the model on this way. Writing is goal-independent in the plan (a
 * one-conversation reply at the chat size), so any goal's plan answers it; reading is the chat
 * goal's — another goal reads prompts of another size, and the tile says "reading" for chat.
 */
export function measuredRunsOf(plans: PlansSeen, way: EngineWay): MeasuredRuns {
  const modelId = modelOf(way);
  if (modelId == null) return { kind: 'unread', detail: 'goose names no model for this engine' };
  const chat = seenPlan(plans, 'chat', modelId);
  const plan = chat ?? anyGoalPlan(plans, modelId);
  if (!plan) {
    return plans.failure != null ? { kind: 'unread', detail: plans.failure } : { kind: 'pending' };
  }
  if (plan.error) return { kind: 'unread', detail: plan.error };
  const match = candidateFor(plan, way);
  if (!match.ok) return { kind: 'unread', detail: match.detail };
  return {
    kind: 'read',
    writing: measuredFigure(match.candidate.speed.decode),
    reading: plan.goal === 'chat' ? measuredFigure(match.candidate.speed.prefill) : null,
  };
}

const OTHER_GOALS: readonly PlacementGoal[] = ['longDocuments', 'manyRequests'];

function anyGoalPlan(plans: PlansSeen, modelId: string): PlacementPlan | null {
  for (const goal of OTHER_GOALS) {
    const plan = seenPlan(plans, goal, modelId);
    if (plan) return plan;
  }
  return null;
}
