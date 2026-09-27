import { useEffect, useState } from 'react';
import { nodesBuildEligibility, type BuildEligibility } from '../../acp/nodes';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import type { NodesConfig } from './model';
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
  const ids = (config?.strategies ?? []).map((s) => s.id);
  const key = JSON.stringify([config?.strategies ?? [], config?.defs ?? []]);
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
