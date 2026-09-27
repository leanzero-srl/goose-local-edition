/**
 * Nodes and strategies — the desktop's view of the `nodes` model (design
 * DESIGN-NODES-AND-STRATEGIES.md §4.2). The TYPES are the generated mirror of goosed's
 * `custom_requests/nodes.rs` (one shape for the config key, the wire and this file); the RULES here
 * mirror `crates/goose/src/nodes/mod.rs` and both suites run `nodes.fixture.json`.
 */
import type {
  NodeChainEntry,
  NodeDef,
  NodeDefKind,
  NodeIfNotLoaded,
  NodePlacement,
  NodeRole,
  NodeRoleEntry,
  NodeStrategy,
  NodeStrategyRoles,
  NodeWhen,
  NodesConfig,
  NodesForBuilds,
  NodesForNewChats,
  ResolvedNodeDef,
} from '@aaif/goose-sdk';

export type {
  NodeChainEntry,
  NodeDef,
  NodeDefKind,
  NodeIfNotLoaded,
  NodePlacement,
  NodeRole,
  NodeRoleEntry,
  NodeStrategy,
  NodeStrategyRoles,
  NodeWhen,
  NodesConfig,
  NodesForBuilds,
  NodesForNewChats,
  ResolvedNodeDef,
};

export const ROLES: readonly NodeRole[] = [
  'chat',
  'planning',
  'build',
  'testing',
  'frontend',
  'backend',
] as const;

/** The Mac key a placement uses for this Mac. */
export const THIS_MAC = 'local';

/** The role an unset role inherits from (the editor says "Same as <role>"). */
export function inheritsFrom(role: NodeRole): NodeRole {
  switch (role) {
    case 'chat':
      return 'build';
    case 'planning':
    case 'build':
      return 'chat';
    case 'testing':
    case 'frontend':
    case 'backend':
      return 'build';
  }
}

/**
 * The role whose entry `role` uses: itself when set, else the first set role up its inheritance.
 * `null` only when Chat and Build are both unset (the cycle `nodes/write` refuses).
 */
export function effectiveRole(roles: NodeStrategyRoles, role: NodeRole): NodeRole | null {
  const seen = new Set<NodeRole>();
  let current = role;
  for (;;) {
    if (roles[current]) return current;
    if (seen.has(current)) return null;
    seen.add(current);
    current = inheritsFrom(current);
  }
}

export function effectiveEntry(strategy: NodeStrategy, role: NodeRole): NodeRoleEntry | null {
  const source = effectiveRole(strategy.roles ?? {}, role);
  return source ? (strategy.roles?.[source] ?? null) : null;
}

/** What a model name on the `swarm` provider means (design §7.1). */
export type RouteModel =
  | { kind: 'auto' }
  | { kind: 'build' }
  | { kind: 'buildStrategy'; id: string }
  | { kind: 'node'; id: string }
  | { kind: 'strategy'; id: string; role?: NodeRole };

/** An id is non-empty and never carries the grammar's delimiters. */
export function validId(id: string): boolean {
  return id.length > 0 && !/[:@]/.test(id) && id.trim() === id;
}

/**
 * `null` = not a nodes route (the router's existing handling of any other name applies); a
 * malformed `node:`/`strategy:` id is `null` too, so it can never be read as another node.
 */
export function parseRouteModel(name: string): RouteModel | null {
  if (name === 'swarm') return { kind: 'auto' };
  if (name === 'swarm-build') return { kind: 'build' };
  const build = stripPrefix(name, 'swarm-build:strategy:');
  if (build !== null) return validId(build) ? { kind: 'buildStrategy', id: build } : null;
  const node = stripPrefix(name, 'node:');
  if (node !== null) return validId(node) ? { kind: 'node', id: node } : null;
  const rest = stripPrefix(name, 'strategy:');
  if (rest === null) return null;
  const at = rest.indexOf('@');
  if (at < 0) return validId(rest) ? { kind: 'strategy', id: rest } : null;
  const id = rest.slice(0, at);
  const role = rest.slice(at + 1);
  if (!(ROLES as readonly string[]).includes(role) || !validId(id)) return null;
  return { kind: 'strategy', id, role: role as NodeRole };
}

export function formatRouteModel(route: RouteModel): string {
  switch (route.kind) {
    case 'auto':
      return 'swarm';
    case 'build':
      return 'swarm-build';
    case 'buildStrategy':
      return `swarm-build:strategy:${route.id}`;
    case 'node':
      return `node:${route.id}`;
    case 'strategy':
      return route.role ? `strategy:${route.id}@${route.role}` : `strategy:${route.id}`;
  }
}

/** The model name "new chats start on" writes as the global default. */
export function newChatsModel(forNewChats: NodesForNewChats): string {
  switch (forNewChats.kind) {
    case 'auto':
      return formatRouteModel({ kind: 'auto' });
    case 'node':
      return formatRouteModel({ kind: 'node', id: forNewChats.id });
    case 'strategy':
      return formatRouteModel({ kind: 'strategy', id: forNewChats.id });
  }
}

/** The Macs and link of a pinned way; `null` for a node that follows this Mac's engine. */
export function placementMacs(
  placement: NodePlacement
): { macs: string[]; link: string | null } | null {
  if (placement.kind === 'follows') return null;
  return { macs: placement.macs, link: placement.link ?? null };
}

function stripPrefix(value: string, prefix: string): string | null {
  return value.startsWith(prefix) ? value.slice(prefix.length) : null;
}
