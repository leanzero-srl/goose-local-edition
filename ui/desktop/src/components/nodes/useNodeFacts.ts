import { useEffect, useMemo, useState } from 'react';
import type { NodeLoadGroupDto, NodeResidency } from '@aaif/goose-sdk';
import { useMacs } from '../leanzero-swarm/useMacs';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { usePlacementPlans } from '../leanzero-swarm/usePlacementPlans';
import { nodesLoadHistory } from '../../acp/nodes';
import { acpListProviderDetails } from '../../acp/providers';
import type { PlacementGoal } from '../../acp/mlx-placement';
import type { ProviderDetails } from '../../types/providers';
import { useEngineGlance, useGlanceNodes } from '../engineGlance/glanceStore';
import { nodeGlance, type NodeFacts, type NodeGlance, type Read } from './nodeGlance';
import type { ResolvedNodeDef } from './model';

/**
 * THE FACTS every node surface on the Nodes page derives its states from — the cards (NodesTab)
 * and the strategy pickers and fit panel (StrategiesTab) read ONE set, so a node never reads
 * "Serving" on its card and "Not loaded" in a picker. Extracted from NodesTab unchanged
 * (DESIGN-NODES-AND-STRATEGIES.md §4.3); it adds no poller:
 *
 * - the glance store's `nodes/read` + `nodes/residency` (re-read when the glance changes way, model
 *   or stage — an event), and the main-pushed engine glance;
 * - `useMacs` — the caller mounts it inside `WithMacs`, exactly as Providers does (the two never
 *   render together);
 * - the planner, once per goal in use, asked again when what serves changes;
 * - each pinned MLX node's measured loads, read when the node list or what serves changes;
 * - the provider list, once, when a cloud node exists.
 */
export interface NodeFactsView {
  store: ReturnType<typeof useGlanceNodes>;
  nodes: ResolvedNodeDef[];
  /** The node goosed says serves this Mac's goose (a pinned node leads a follows one). */
  servingNode: ResolvedNodeDef | null;
  /** The measured loads per pinned MLX node id. */
  loads: Record<string, Read<NodeLoadGroupDto[]>>;
  macCount: number;
  factsFor: (node: ResolvedNodeDef) => NodeFacts;
  glanceOf: (node: ResolvedNodeDef) => NodeGlance;
}

export function useNodeFacts(): NodeFactsView {
  const macs = useMacs();
  const push = useEngineGlance();
  const glance = push?.engine ?? null;
  const store = useGlanceNodes();

  const read = store.kind === 'read' ? store.read : null;
  const residency = store.kind === 'read' ? store.residency : null;
  const nodes = useMemo(() => read?.nodes ?? [], [read]);
  const mlxNodes = nodes.filter((n) => n.def.kind === 'mlx');
  const cloudNodes = nodes.filter((n) => n.def.kind !== 'mlx');

  // Which way serves, as a key: the plans and load times are asked again when it changes.
  const servingKey = JSON.stringify(residency?.serving ?? null);

  const goalsInUse = new Set<PlacementGoal>(
    mlxNodes
      .filter((n) => n.def.placement && n.def.placement.kind !== 'follows')
      // A node written without a goal is planned for chat — Run it's own default goal.
      .map((n) => n.def.goal ?? 'chat')
  );
  const planKey = `${mlxNodes.map((n) => n.model ?? '').join('\n')}|${servingKey}`;
  const plansChat = usePlacementPlans(goalsInUse.has('chat') ? 'chat' : null, planKey);
  const plansLong = usePlacementPlans(
    goalsInUse.has('longDocuments') ? 'longDocuments' : null,
    planKey
  );
  const plansMany = usePlacementPlans(
    goalsInUse.has('manyRequests') ? 'manyRequests' : null,
    planKey
  );
  const plansFor = (goal: PlacementGoal) =>
    goal === 'chat' ? plansChat : goal === 'longDocuments' ? plansLong : plansMany;

  const [loads, setLoads] = useState<Record<string, Read<NodeLoadGroupDto[]>>>({});
  const pinnedIds = mlxNodes
    .filter((n) => n.def.placement && n.def.placement.kind !== 'follows')
    .map((n) => n.def.id)
    .join('\n');
  useEffect(() => {
    let alive = true;
    for (const id of pinnedIds ? pinnedIds.split('\n') : []) {
      nodesLoadHistory(id)
        .then((history) => {
          if (alive)
            setLoads((prev) => ({ ...prev, [id]: { kind: 'read', value: history.groups } }));
        })
        .catch((e: unknown) => {
          if (!alive) return;
          setLoads((prev) => ({
            ...prev,
            [id]: { kind: 'failed', error: mlxErrorMessage(e, String(e)) },
          }));
        });
    }
    return () => {
      alive = false;
    };
  }, [pinnedIds, servingKey]);

  const [providers, setProviders] = useState<Read<ProviderDetails[]>>({ kind: 'reading' });
  const wantsProviders = cloudNodes.length > 0;
  useEffect(() => {
    if (!wantsProviders) return;
    let alive = true;
    acpListProviderDetails()
      .then((list) => alive && setProviders({ kind: 'read', value: list }))
      .catch(
        (e: unknown) =>
          alive && setProviders({ kind: 'failed', error: mlxErrorMessage(e, String(e)) })
      );
    return () => {
      alive = false;
    };
  }, [wantsProviders]);

  const residencyOf = (id: string): Read<NodeResidency> => {
    if (store.kind === 'failed') return { kind: 'failed', error: store.error };
    if (!residency) return { kind: 'reading' };
    const row = residency.nodes.find((r) => r.node === id);
    // A node goosed did not answer for (written after this read) is still being read.
    return row ? { kind: 'read', value: row.residency } : { kind: 'reading' };
  };
  const servingNode =
    residency?.nodes
      .filter((r) => r.residency.kind === 'serving')
      .map((r) => nodes.find((n) => n.def.id === r.node))
      .find((n) => n?.def.placement && n.def.placement.kind !== 'follows') ??
    residency?.nodes
      .filter((r) => r.residency.kind === 'serving')
      .map((r) => nodes.find((n) => n.def.id === r.node))
      .find((n) => n != null) ??
    null;

  const factsFor = (node: ResolvedNodeDef): NodeFacts => ({
    residency: residencyOf(node.def.id),
    serving: residency?.serving ?? null,
    servingNodeName: servingNode?.def.name ?? null,
    glance,
    plans: plansFor(node.def.goal ?? 'chat'),
    macs: macs.macs,
    modelsOn: (key) => macs.factsOf(key).models,
    loads: loads[node.def.id] ?? { kind: 'reading' },
    // No swarm run registers a holder yet (S5's holders.rs, S8): nothing here says a build holds
    // the engine, so no card claims it.
    buildHolder: null,
    provider:
      providers.kind === 'read'
        ? { kind: 'read', value: providers.value.find((p) => p.name === node.provider) ?? null }
        : providers,
  });

  return {
    store,
    nodes,
    servingNode,
    loads,
    macCount: macs.macs.length,
    factsFor,
    glanceOf: (node) => nodeGlance(node, factsFor(node)),
  };
}
