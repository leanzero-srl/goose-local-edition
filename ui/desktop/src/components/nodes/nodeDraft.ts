import type { MlxPlacementKeyDto } from '@aaif/goose-sdk';
import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { PlacementCandidate, PlacementGoal } from '../../acp/mlx-placement';
import { nodesRead, nodesWrite, type NodesWrite } from '../../acp/nodes';
import { macForPlacementNode, type Mac } from '../leanzero-swarm/macs';
import {
  THIS_MAC,
  type NodeDef,
  type NodePlacement,
  type NodesConfig,
  type ResolvedNodeDef,
} from './model';

/**
 * Making a node definition — the one set of rules the New node dialog and Run it's "Save as node"
 * share: the way a planner candidate becomes a placement, the default name (§4.4: "<model short
 * name> · <where>"), a unique name and id, and the one write (read the config, add the def, write it
 * back through `nodes/write`, whose refusals the caller shows verbatim).
 */

const i18n = defineMessages({
  whereThisMac: { id: 'nodes.nameThisMac', defaultMessage: 'this Mac' },
  whereBothMacs: {
    id: 'nodes.nameAcrossMacs',
    defaultMessage: '{count, plural, =2 {both Macs} other {# Macs}}',
  },
  defaultName: { id: 'nodes.defaultName', defaultMessage: '{model} · {where}' },
});

/** A pinned way: the planner's `PlacementKey` as a node stores it. */
export type PinnedPlacement = Exclude<NodePlacement, { kind: 'follows' }>;

/** A planner key (`PlacementKey`: kind, node ids in rank order, a split's link) as a placement. */
export function placementOfKey(key: MlxPlacementKeyDto): PinnedPlacement {
  const { kind, nodes, link } = key;
  return link ? { kind, macs: [...nodes], link } : { kind, macs: [...nodes] };
}

export function placementOfCandidate(candidate: PlacementCandidate): PinnedPlacement {
  return placementOfKey(candidate.key);
}

/** Two placements name the same way: kind, Macs in rank order and (for a split) the link. */
export function samePlacement(
  a: NodePlacement | null | undefined,
  b: NodePlacement | null | undefined
): boolean {
  if (!a || !b || a.kind === 'follows' || b.kind === 'follows') return false;
  if (a.kind !== b.kind || a.macs.length !== b.macs.length) return false;
  if (a.macs.some((mac, i) => mac !== b.macs[i])) return false;
  return a.kind === 'single' || (a.link ?? null) === (b.link ?? null);
}

/** A model id as a name reads it: its last path segment. */
export function modelShortName(modelId: string): string {
  return modelId.split('/').filter(Boolean).pop() ?? modelId;
}

/** Where a pinned way runs, as a node's default name says it. */
export function whereWords(
  intl: IntlShape,
  placement: PinnedPlacement,
  macs: readonly Mac[]
): string {
  if (placement.kind !== 'single') {
    return intl.formatMessage(i18n.whereBothMacs, { count: placement.macs.length });
  }
  const mac = placement.macs[0] ?? THIS_MAC;
  if (mac === THIS_MAC) return intl.formatMessage(i18n.whereThisMac);
  return macForPlacementNode(macs, mac)?.name ?? mac;
}

/** `name`, or `name (2)`, `name (3)`… — the first that no other node carries (Q-154). */
export function uniqueName(name: string, taken: Iterable<string>): string {
  const names = new Set(taken);
  if (!names.has(name)) return name;
  for (let n = 2; ; n += 1) {
    const next = `${name} (${n})`;
    if (!names.has(next)) return next;
  }
}

export function defaultNodeName(
  intl: IntlShape,
  model: string,
  where: string,
  taken: Iterable<string>
): string {
  return uniqueName(
    intl.formatMessage(i18n.defaultName, { model: modelShortName(model), where }),
    taken
  );
}

/** A stable id from a name: lowercase words joined by '-', never ':' or '@', unique. */
export function nodeIdFor(name: string, takenIds: Iterable<string>): string {
  const base =
    name
      .toLowerCase()
      .normalize('NFKD')
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'node';
  const ids = new Set(takenIds);
  if (!ids.has(base)) return base;
  for (let n = 2; ; n += 1) {
    const next = `${base}-${n}`;
    if (!ids.has(next)) return next;
  }
}

/** The node already defined for this model on this way, if any. */
export function nodeForWay(
  nodes: readonly ResolvedNodeDef[],
  model: string,
  placement: PinnedPlacement
): ResolvedNodeDef | null {
  return (
    nodes.find(
      (n) => n.def.kind === 'mlx' && n.model === model && samePlacement(n.def.placement, placement)
    ) ?? null
  );
}

export interface MlxDraft {
  name: string;
  model: string;
  placement: PinnedPlacement;
  goal: PlacementGoal;
  keepLoaded: boolean;
  origin: 'user' | 'runIt';
}

export function mlxDef(id: string, draft: MlxDraft): NodeDef {
  return {
    id,
    name: draft.name,
    kind: 'mlx',
    model: draft.model,
    placement: draft.placement,
    goal: draft.goal,
    keepLoaded: draft.keepLoaded,
    origin: draft.origin,
  };
}

export interface ProviderDraft {
  kind: 'cloud' | 'endpoint';
  name: string;
  provider: string;
  model: string;
}

export function providerDef(id: string, draft: ProviderDraft): NodeDef {
  return {
    id,
    name: draft.name,
    kind: draft.kind,
    model: draft.model,
    provider: draft.provider,
    origin: 'user',
  };
}

/**
 * Add one def, or replace the def with the same id (an edit), through the one write door. The
 * config is read first (unless the caller just read it) so the write carries every other def and
 * strategy as goosed holds them now.
 */
export async function putNode(def: NodeDef, current?: NodesConfig): Promise<NodesWrite> {
  const config = current ?? (await nodesRead()).config;
  const defs = config.defs ?? [];
  const at = defs.findIndex((d) => d.id === def.id);
  const next = at >= 0 ? defs.map((d, i) => (i === at ? def : d)) : [...defs, def];
  return nodesWrite({ ...config, defs: next });
}
