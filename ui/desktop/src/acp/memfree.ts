import { getAcpClient } from './acpConnection';
import { acpReadConfig } from './config';
import {
  MEMFREE_NEVER_TOUCH_KEY,
  neverTouchExtrasFrom,
  type MemoryFreeResult,
  type MemoryHolders,
} from '../components/memfree/memfree';

/**
 * "Free up memory" over ACP: `_goose/unstable/mlxEngine/memoryHolders` measures who holds memory
 * on a Mac (and, with `fitModelId`, the verdict for that model); `_goose/unstable/mlxEngine/memoryFree`
 * reclaims goose's own memory and quits the named apps. Same wire path and `nodeId` rule as
 * `acp/mlx-engine.ts`: absent nodeId = this Mac; a peer's id is forwarded over the mesh. Types are
 * the local mirror in components/memfree/memfree.ts (camelCase, serde `rename_all`).
 */
async function call<T>(method: string, params: Record<string, unknown>): Promise<T> {
  const client = await getAcpClient();
  return (await client.extMethod(method, params)) as unknown as T;
}

function withNode(params: Record<string, unknown>, nodeId?: string): Record<string, unknown> {
  if (nodeId != null) params.nodeId = nodeId;
  return params;
}

/** This Mac's own never-touch additions (Settings › App), sent with every call so they hold on a peer. */
export async function readNeverTouchExtras(): Promise<string[]> {
  return neverTouchExtrasFrom(await acpReadConfig(MEMFREE_NEVER_TOUCH_KEY, false));
}

export async function memoryHolders(opts: {
  nodeId?: string;
  fitModelId?: string | null;
  neverTouch?: string[];
}): Promise<MemoryHolders> {
  const params: Record<string, unknown> = {};
  if (opts.fitModelId) params.fitModelId = opts.fitModelId;
  if (opts.neverTouch) params.neverTouch = opts.neverTouch;
  return call<MemoryHolders>(
    '_goose/unstable/mlxEngine/memoryHolders',
    withNode(params, opts.nodeId)
  );
}

/**
 * `gooseOwned` = the soft clean. `quitApps` (bundle ids) and `unloadServers` (`ai:…` / `mlx:<pid>`)
 * are only what the person confirmed; `nuclear` lets an app that ignores quit get one SIGTERM.
 */
export async function memoryFree(opts: {
  nodeId?: string;
  gooseOwned: boolean;
  quitApps: string[];
  unloadServers: string[];
  nuclear: boolean;
  neverTouch: string[];
  fitModelId?: string | null;
}): Promise<MemoryFreeResult> {
  const params: Record<string, unknown> = {
    gooseOwned: opts.gooseOwned,
    quitApps: opts.quitApps,
    unloadServers: opts.unloadServers,
    nuclear: opts.nuclear,
    neverTouch: opts.neverTouch,
  };
  if (opts.fitModelId) params.fitModelId = opts.fitModelId;
  return call<MemoryFreeResult>(
    '_goose/unstable/mlxEngine/memoryFree',
    withNode(params, opts.nodeId)
  );
}
