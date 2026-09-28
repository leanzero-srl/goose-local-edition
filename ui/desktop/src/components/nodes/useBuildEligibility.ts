import { useEffect, useState } from 'react';
import { nodesBuildEligibility, type BuildEligibility } from '../../acp/nodes';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { namedStrategies, type NodeStrategy, type NodesConfig } from './model';
import type { Read } from './nodeGlance';

/**
 * Whether each STORED strategy can drive a swarm build — goosed's `nodes/buildEligibility`
 * (Tier A, design §7.2), the one answer the "Swarm builds use" selector, the strategy cards and the
 * editor show. Read when the stored strategies or defs change (an event), never on a clock; a failed
 * read is its words, never "eligible".
 */
export function useBuildEligibility(
  config: NodesConfig | null
): Record<string, Read<BuildEligibility>> {
  const [answers, setAnswers] = useState<Record<string, Read<BuildEligibility>>>({});
  // A chat's own node set never drives a build (goosed refuses it): it is never asked about.
  const named = namedStrategies(config);
  const ids = named.map((s) => s.id);
  const key = JSON.stringify([named, config?.defs ?? []]);
  const idsKey = ids.join('\n');
  useEffect(() => {
    let alive = true;
    const list = idsKey ? idsKey.split('\n') : [];
    // An answer about the strategies as they WERE says nothing about them now.
    setAnswers(Object.fromEntries(list.map((id) => [id, { kind: 'reading' as const }])));
    for (const id of list) {
      nodesBuildEligibility(id)
        .then(
          (value) => alive && setAnswers((prev) => ({ ...prev, [id]: { kind: 'read', value } }))
        )
        .catch(
          (e: unknown) =>
            alive &&
            setAnswers((prev) => ({
              ...prev,
              [id]: { kind: 'failed', error: mlxErrorMessage(e, String(e)) },
            }))
        );
    }
    return () => {
      alive = false;
    };
    // `key` carries every stored fact eligibility depends on; `idsKey` which strategies to ask.
  }, [key, idsKey]);
  return answers;
}

/**
 * goosed's answer for the strategy as the editor holds it NOW, asked each time the draft changes
 * (an event, never a clock) — so the editor says whether swarm builds can use it before Save, not
 * after (Q-311). `null` while the draft IS the stored strategy (the stored answer speaks for it). An
 * answer for an older draft never lands on a newer one.
 */
export function useDraftBuildEligibility(
  draft: NodeStrategy,
  dirty: boolean
): Read<BuildEligibility> | null {
  const [answer, setAnswer] = useState<Read<BuildEligibility> | null>(null);
  const key = dirty ? JSON.stringify(draft) : null;
  useEffect(() => {
    if (key == null) {
      setAnswer(null);
      return;
    }
    let alive = true;
    const asked = JSON.parse(key) as NodeStrategy;
    setAnswer({ kind: 'reading' });
    nodesBuildEligibility(asked.id, asked)
      .then((value) => alive && setAnswer({ kind: 'read', value }))
      .catch(
        (e: unknown) => alive && setAnswer({ kind: 'failed', error: mlxErrorMessage(e, String(e)) })
      );
    return () => {
      alive = false;
    };
  }, [key]);
  return answer;
}
