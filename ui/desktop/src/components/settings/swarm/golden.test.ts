import { describe, expect, it } from 'vitest';
import {
  GOLDEN,
  DEFAULTS,
  PRESET_KEYS,
  detectPreset,
  presetPatch,
  type SwarmConfig,
} from './golden';

describe('swarm golden preset', () => {
  it('detects the active preset from the tunable subset', () => {
    expect(detectPreset(GOLDEN)).toBe('golden');
    expect(detectPreset(DEFAULTS)).toBe('default');
    expect(detectPreset({ ...GOLDEN, best_of_n_skeletons: 99 })).toBe('custom');
  });

  it('detects golden even when fleet identity + sampling differ (they are not part of a preset)', () => {
    const withFleet: SwarmConfig = {
      ...GOLDEN,
      endpoint: 'http://192.168.8.220:1234',
      planner_model: 'qwopus3.6-27b-coder',
      temperature: 0.2,
      devices: [{ id: 'a', model_id: 'm', weight: 2 }],
      speed_weights: { worksmacstudio: 3 },
    };
    expect(detectPreset(withFleet)).toBe('golden');
  });

  it('presetPatch touches ONLY the portable tuning keys — never fleet identity', () => {
    const patch = presetPatch(GOLDEN);
    for (const k of Object.keys(patch)) {
      expect(PRESET_KEYS).toContain(k);
    }
    // the four things a preset must never clobber
    expect(patch).not.toHaveProperty('endpoint');
    expect(patch).not.toHaveProperty('planner_model');
    expect(patch).not.toHaveProperty('devices');
    expect(patch).not.toHaveProperty('speed_weights');
  });

  it('applying golden over a config with devices preserves the devices (merge, not replace)', () => {
    const prev: SwarmConfig = {
      ...DEFAULTS,
      devices: [{ id: 'mac', model_id: 'x', weight: 2 }],
      speed_weights: { worksmacstudio: 3, local: 2, gabee: 1 },
      endpoint: 'http://192.168.8.220:1234',
    };
    const next = { ...prev, ...presetPatch(GOLDEN) };
    expect(next.devices).toEqual(prev.devices);
    expect(next.speed_weights).toEqual(prev.speed_weights);
    expect(next.endpoint).toBe('http://192.168.8.220:1234');
    // and the tuning DID change to golden values
    expect(next.best_of_n_skeletons).toBe(2);
    expect(next.dynamic_replan).toBe(false);
    expect(next.homogeneous_models).toBe(true);
    expect(next.worker_timeout_secs).toBe(420);
  });

  it('the golden formula diverges from defaults exactly where the tuning was hard-won', () => {
    expect(GOLDEN.dynamic_replan).toBe(false);
    expect(DEFAULTS.dynamic_replan).toBe(true);
    expect(GOLDEN.best_of_n_skeletons).toBe(2);
    expect(GOLDEN.max_replans).toBe(1);
    expect(GOLDEN.homogeneous_models).toBe(true);
    // the panel baseline is faithful to the Rust default (900), not the golden 420
    expect(DEFAULTS.worker_timeout_secs).toBe(900);
    expect(GOLDEN.worker_timeout_secs).toBe(420);
  });
});
