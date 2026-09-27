import { describe, expect, it } from 'vitest';
import type { MlxDistributedConfig, MlxDistributedDiscovery } from '../../acp/mlx-distributed';
import { cleanConfig, splitConfigFor, splitPlan } from './mlxDistributed';
import FIXTURE from '../../../../../crates/goose/src/acp/server/nodes_loader/split_config.fixture.json';

/**
 * The loader's port of Run it's split-for-another-model path (goosed's
 * `nodes_loader/split_config.rs`) and these TS functions run the SAME cases, so a split the loader
 * starts is configured exactly as Run it's `startSplitFor` would configure it (design §6.4 step 5).
 */
describe('split_config.fixture.json — Run it’s split config, pinned to the loader’s port', () => {
  it('has the cases the port is held to', () => {
    expect(FIXTURE.cases.length).toBeGreaterThanOrEqual(8);
  });

  for (const c of FIXTURE.cases) {
    it(c.name, () => {
      const discovery = c.discovery as unknown as MlxDistributedDiscovery;
      const saved = (c.saved ?? null) as MlxDistributedConfig | null;
      const config = cleanConfig(splitConfigFor(discovery, saved));
      expect(config).toEqual(c.expect.config);
      expect(splitPlan(discovery, c.modelId, config)).toEqual(c.expect.plan);
    });
  }
});
