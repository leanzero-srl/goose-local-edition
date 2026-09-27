import { describe, expect, it, vi } from 'vitest';
import type { MlxEngineStatus } from '../../acp/mlx-engine';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import FIXTURE from '../../../../../crates/goose/src/acp/server/nodes_loader/switch.fixture.json';

let remoteLatest: unknown = null;
vi.mock('../../acp/mlx-remote-single', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/mlx-remote-single')>()),
  latestMlxRemoteSingleStatus: () => remoteLatest,
}));

const { servingWays } = await import('./PlacementCard');

/**
 * Run it's switch and goosed's node loader stop the SAME ways, in the same order, before another
 * starts (design §6.4 step 9: one MLX way serves this Mac's goose at a time, across all Macs). The
 * loader's port (`nodes_loader/switch.rs`) runs these cases too; Run it keeps its own
 * choreography in v1, and this fixture is what keeps the two one rule.
 */
describe('switch.fixture.json — Run it’s stop set, pinned to the loader’s', () => {
  it('has the cases the port is held to', () => {
    expect(FIXTURE.cases.length).toBeGreaterThanOrEqual(12);
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const serving = c.serving as {
        single?: { state: string; modelId?: string };
        remote?: { state: string; peer?: string; modelId?: string };
        distributed?: { state: string; mode: string; modelId?: string };
      };
      remoteLatest = serving.remote ?? null;
      const stops = servingWays(
        [],
        [],
        (serving.single ?? null) as MlxEngineStatus | null,
        (serving.distributed ?? null) as MlxDistributedStatus | null
      ).map((s) => ({
        way: s.way.peerNodeId ? { kind: s.way.kind, peer: s.way.peerNodeId } : { kind: s.way.kind },
        modelId: s.modelId,
      }));
      expect(stops).toEqual(c.expect.stops);
    });
  }
});
