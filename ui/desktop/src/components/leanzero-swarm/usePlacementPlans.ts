import { useEffect, useState } from 'react';
import { mlxPlacementPlan, type PlacementGoal, type PlacementPlan } from '../../acp/mlx-placement';
import { mlxErrorMessage } from './mlxErrorMessage';

/**
 * The placement planner's plans for one goal, as a STATE the caller must read (D10,
 * DESIGN-NODES-AND-STRATEGIES.md §2.3): a failed read carries goose's words and is never an empty
 * map — "no plans" and "could not plan" are different facts, and only the first means every model
 * has no way to run.
 *
 * - `idle`: no goal was asked (`goal === null`); nothing is read.
 * - `reading`: the first read for this goal is in flight. A re-read after a change keeps showing
 *   the last answer until the new one lands, so a card never flickers back to "reading".
 * - `read`: the plans by model id, plus the lines of the measurement store that did not parse.
 * - `failed`: the read's own words.
 */
export type PlacementPlansRead =
  | { kind: 'idle' }
  | { kind: 'reading' }
  | { kind: 'read'; plans: ReadonlyMap<string, PlacementPlan>; storeErrors: string[] }
  | { kind: 'failed'; error: string };

/**
 * Plans every model in the models folder (or only `modelId`) for `goal`, again whenever `key`
 * changes — the caller folds into `key` whatever moves a fit (the model list, what serves now).
 */
export function usePlacementPlans(
  goal: PlacementGoal | null,
  key: string,
  modelId?: string
): PlacementPlansRead {
  const [read, setRead] = useState<{ asked: string; state: PlacementPlansRead }>({
    asked: '',
    state: { kind: 'idle' },
  });
  const asked = goal == null ? '' : `${goal}\n${modelId ?? ''}`;

  useEffect(() => {
    if (goal == null) return;
    let cancelled = false;
    mlxPlacementPlan(goal, modelId)
      .then((response) => {
        if (cancelled) return;
        setRead({
          asked,
          state: {
            kind: 'read',
            plans: new Map(response.plans.map((plan) => [plan.modelId, plan])),
            storeErrors: response.storeErrors ?? [],
          },
        });
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        setRead({ asked, state: { kind: 'failed', error: mlxErrorMessage(error, String(error)) } });
      });
    return () => {
      cancelled = true;
    };
  }, [goal, modelId, key, asked]);

  if (goal == null) return { kind: 'idle' };
  // An answer for another goal or model is not this one's: until this read lands, it is reading.
  return read.asked === asked ? read.state : { kind: 'reading' };
}
