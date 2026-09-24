import type { NodeAllows, NodeState, NodesResponse } from '../../acp/leanzero-link';

/**
 * The linked Macs, ONE way: every surface that names a Mac (My Macs, the Models columns, Run it,
 * the tray) calls it by `macName` — the name its owner gave it (macOS ComputerName, reported by the
 * Mac itself on the Link roster), else its hostname. Pure and React-free: main imports it for the
 * tray.
 */

/** The owner's three switches, each a goose config key its route reads. */
export type Permission = 'manage' | 'chat' | 'split';

export const PERMISSIONS: readonly Permission[] = ['manage', 'chat', 'split'];

/** goose's `ALLOW_REMOTE_EXECUTION_KEY` / `ALLOW_CHAT_SERVING_KEY` / `ALLOW_DISTRIBUTED_NODE_KEY`. */
export const PERMISSION_KEY: Record<Permission, string> = {
  manage: 'LEANZERO_LINK_ALLOW_REMOTE_EXECUTION',
  chat: 'LEANZERO_LINK_ALLOW_CHAT_SERVING',
  split: 'LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE',
};

const ALLOWS_FIELD: Record<Permission, keyof NodeAllows> = {
  manage: 'manage_models',
  chat: 'answer_chat',
  split: 'run_split',
};

/** This Mac's key in every per-Mac map; a peer's key is its Link node id. */
export const SELF_KEY = 'self';

export interface Mac {
  key: string;
  isSelf: boolean;
  /** The Link node id; null only for this Mac when Link reported no roster. */
  nodeId: string | null;
  name: string;
  hostname: string;
  meshIp: string | null;
  /** The mesh reports it reachable (a peer marked Offline is not). */
  online: boolean;
  sessionsActive: number;
  /** What its owner lets the other Macs do there; null = its goose did not say (older build). */
  allows: NodeAllows | null;
  /** Why this Mac's last poll of the peer failed, verbatim. */
  pollError: string | null;
}

export function macName(node: Pick<NodeState, 'computer_name' | 'hostname'>): string {
  const name = node.computer_name?.trim();
  return name ? name : node.hostname;
}

/**
 * The Mac a remote-single route serves chat from, named the one way: `macName` over the two facts
 * the route kept from the Link roster (its computer name, its hostname), else the node id it was
 * started with.
 */
export function routePeerName(route: {
  peer?: string | null;
  peerHostname?: string | null;
  peerComputerName?: string | null;
}): string {
  return macName({
    computer_name: route.peerComputerName ?? undefined,
    hostname: route.peerHostname || route.peer || '',
  });
}

function toMac(node: NodeState, isSelf: boolean): Mac {
  return {
    key: isSelf ? SELF_KEY : node.node_id,
    isSelf,
    nodeId: node.node_id,
    name: macName(node),
    hostname: node.hostname,
    meshIp: node.mesh_ip ?? null,
    online: isSelf || node.status.type !== 'Offline',
    sessionsActive: node.sessions_active,
    allows: node.allows ?? null,
    pollError: node.last_poll_error ?? null,
  };
}

/**
 * This Mac first, then every peer on the roster in the roster's order. With no roster (Link off or
 * not read yet) this Mac alone, named `selfName` — the caller's localized "This Mac".
 */
export function macsFrom(nodes: NodesResponse | null, selfName: string): Mac[] {
  if (!nodes) {
    return [
      {
        key: SELF_KEY,
        isSelf: true,
        nodeId: null,
        name: selfName,
        hostname: '',
        meshIp: null,
        online: true,
        sessionsActive: 0,
        allows: null,
        pollError: null,
      },
    ];
  }
  return [toMac(nodes.self, true), ...nodes.peers.map((p) => toMac(p, false))];
}

/** The `nodeId` an mlxEngine op on this Mac carries: omitted (undefined) for this Mac. */
export function macTarget(mac: Pick<Mac, 'isSelf' | 'key'>): string | undefined {
  return mac.isSelf ? undefined : mac.key;
}

/** true/false as the Mac reported it; null when its goose did not say. */
export function allowsOf(mac: Pick<Mac, 'allows'>, permission: Permission): boolean | null {
  return mac.allows ? mac.allows[ALLOWS_FIELD[permission]] : null;
}

/** A peer whose owner turned this switch OFF — known from its own report, never guessed. */
export function peerRefuses(mac: Mac, permission: Permission): boolean {
  return !mac.isSelf && allowsOf(mac, permission) === false;
}

/**
 * Which switch refused an answer from another Mac, from goose's own refusal text: the mesh's `403`
 * for model management, the `chatServingDisabled` / `servingDisabled` codes. null = not a switch.
 */
export function refusedBy(text: string): Permission | null {
  if (
    text.includes('remote model management is disabled') ||
    text.includes('remote execution disabled') ||
    text.includes('remoteManagementDisabled')
  ) {
    return 'manage';
  }
  if (text.includes('chatServingDisabled')) return 'chat';
  if (text.includes('servingDisabled')) return 'split';
  return null;
}

/** The Mac a placement node id names: `local` is this Mac, `link:<id>` a peer. */
export function macForPlacementNode(macs: readonly Mac[], id: string): Mac | null {
  if (id === 'local') return macs.find((m) => m.isSelf) ?? null;
  if (id.startsWith('link:')) {
    const nodeId = id.slice('link:'.length);
    return macs.find((m) => !m.isSelf && m.key === nodeId) ?? null;
  }
  return null;
}

/**
 * Whole minutes to move `bytes` at a MEASURED `bytesPerSec` (a finished copy's own wire rate —
 * never the link's negotiated line rate, which the checked copy does not reach), at least 1.
 */
export function minutesAt(bytes: number, bytesPerSec: number): number {
  return Math.max(1, Math.ceil(bytes / bytesPerSec / 60));
}
