import { getAcpClient } from './acpConnection';
import type { MemoryFreeResult, MemoryHolders } from '../components/memfree/memfree';

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

export async function memoryHolders(opts: {
  nodeId?: string;
  fitModelId?: string | null;
}): Promise<MemoryHolders> {
  return call<MemoryHolders>(
    '_goose/unstable/mlxEngine/memoryHolders',
    withNode(opts.fitModelId ? { fitModelId: opts.fitModelId } : {}, opts.nodeId)
  );
}

export async function memoryFree(opts: {
  nodeId?: string;
  gooseOwned: boolean;
  quitApps: string[];
  fitModelId?: string | null;
}): Promise<MemoryFreeResult> {
  const params: Record<string, unknown> = {
    gooseOwned: opts.gooseOwned,
    quitApps: opts.quitApps,
  };
  if (opts.fitModelId) params.fitModelId = opts.fitModelId;
  return call<MemoryFreeResult>(
    '_goose/unstable/mlxEngine/memoryFree',
    withNode(params, opts.nodeId)
  );
}
