import { describe, expect, it } from 'vitest';
import {
  GOLDEN,
  DEFAULTS,
  PRESET_KEYS,
  detectPreset,
  presetPatch,
  type SwarmConfig,
  nodeRows,
} from './golden';

describe('swarm golden preset', () => {
  it('detects golden vs a user-diverged config', () => {
    expect(detectPreset(GOLDEN)).toBe('golden');
    expect(detectPreset({ ...GOLDEN, best_of_n_skeletons: 99 })).toBe('custom');
  });

  it('detects golden even when fleet identity + sampling differ (they are not part of a preset)', () => {
    const withFleet: SwarmConfig = {
      ...GOLDEN,
      endpoint: 'http://192.168.8.220:1234',
      planner_model: 'qwopus3.6-27b-coder',
      temperature: 0.2,
      devices: [{ id: 'a', model_id: 'm', weight: 2, enabled: true }],
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

  it('resetting to golden preserves the fleet (merge, not replace)', () => {
    const prev: SwarmConfig = {
      ...DEFAULTS,
      best_of_n_skeletons: 99,
      worker_timeout_secs: 60,
      devices: [{ id: 'mac', model_id: 'x', weight: 2, enabled: true }],
      speed_weights: { worksmacstudio: 3, local: 2, gabee: 1 },
      endpoint: 'http://192.168.8.220:1234',
    };
    const next = { ...prev, ...presetPatch(GOLDEN) };
    expect(next.devices).toEqual(prev.devices);
    expect(next.speed_weights).toEqual(prev.speed_weights);
    expect(next.endpoint).toBe('http://192.168.8.220:1234');
    // and the user's divergence was restored to the golden baseline
    expect(next.best_of_n_skeletons).toBe(DEFAULTS.best_of_n_skeletons);
    expect(next.worker_timeout_secs).toBe(DEFAULTS.worker_timeout_secs);
    expect(detectPreset(next)).toBe('golden');
  });

  // THE load-bearing invariant. The golden formula is now baked into the engine's own
  // `Default for SwarmConfig`, and `load_config` merges config.yaml OVER it — so an empty config already
  // runs the formula. The panel seeds from DEFAULTS and persists the whole block on any edit, so if a value
  // here disagreed with the bake, touching ANY control would silently write a divergent value over the
  // engine's default and the desktop would stop matching a headless run. GOLDEN is that same baseline.
  it('GOLDEN is the baseline itself — there is no second, diverging preset', () => {
    expect(GOLDEN).toBe(DEFAULTS);
  });

  it('DEFAULTS mirrors the engine bake, so the panel can never write a divergent value', () => {
    // Each of these is the value in `Default for SwarmConfig` (crates/goose-cli/src/commands/swarm.rs).
    // If you re-tune one in Rust and the panel still exposes it, change it HERE in the same commit.
    expect(DEFAULTS.worker_max_turns).toBe(40);
    expect(DEFAULTS.max_attempts).toBe(3);
    expect(DEFAULTS.worker_timeout_secs).toBe(900);
    expect(DEFAULTS.planner_timeout_secs).toBe(900);
    expect(DEFAULTS.progress_watchdog_secs).toBe(900);
    expect(DEFAULTS.research_planning).toBe('on');
    expect(DEFAULTS.max_research_questions).toBe(4);
    expect(DEFAULTS.max_replans).toBe(2);
    expect(DEFAULTS.scout_max_lookups).toBe(10);
    expect(DEFAULTS.scout_budget_secs).toBe(900);
    expect(DEFAULTS.best_of_n_skeletons).toBe(1);
    expect(DEFAULTS.planner_also_works).toBe(true);
    expect(DEFAULTS.planner_weight).toBe(1);
    expect(DEFAULTS.homogeneous_models).toBe(false);
    expect(DEFAULTS.allow_model_load).toBe(false);
    // The consult levers: baked ON so a fresh install asks instead of guessing (+5 weak-planner bump -> 85).
    expect(DEFAULTS.ask_floor).toBe(80);
    expect(DEFAULTS.ask_max_q).toBe(3);
    // The one OPTIONAL extra check, baked OFF — the panel must show it off, not on.
    expect(DEFAULTS.review).toBe(false);
  });

  it('every key the panel can reset is one the engine baseline actually defines', () => {
    // A PRESET_KEY missing from DEFAULTS would reset that control to `undefined` — silently dropping the
    // key from config.yaml rather than restoring the golden value.
    for (const k of PRESET_KEYS) {
      expect(DEFAULTS[k], `PRESET_KEYS lists "${String(k)}" but DEFAULTS does not define it`).not.toBe(
        undefined
      );
    }
  });
});

describe('nodeRows — the Nodes list is node-first and shows every node', () => {
  const local = ['gabee-qwen3.8-27b', 'mihai-qwen3.8-27b', 'workhorse-qwen3.8-27b'];

  /** THE REGRESSION. The panel used to render `devices.length > 0 ? devices : fleet.models`, so adding a
   *  single cloud node made all three local nodes disappear from settings while the engine still ran them. */
  it('keeps the local fleet visible after a cloud node is added', () => {
    const rows = nodeRows(
      [{ id: 'zai-glm', model_id: 'glm-5.3-flash', weight: 2, enabled: true, provider: 'zai' }],
      local
    );
    expect(rows).toHaveLength(4);
    expect(rows.filter((r) => r.provider === null).map((r) => r.modelId)).toEqual(local);
    expect(rows[0].provider).toBe('zai');
  });

  it('does not list a local model twice once it has a device row', () => {
    const rows = nodeRows(
      [{ id: 'mihai', model_id: 'mihai-qwen3.8-27b', weight: 1, enabled: true }],
      local
    );
    expect(rows).toHaveLength(3);
    expect(rows.filter((r) => r.modelId === 'mihai-qwen3.8-27b')).toHaveLength(1);
    expect(rows[0].configured).toBe(true);
  });

  it('carries the supervisor flag through', () => {
    const rows = nodeRows(
      [{ id: 'w', model_id: 'workhorse-qwen3.8-27b', weight: 1, enabled: true, supervision: true }],
      local
    );
    expect(rows.find((r) => r.modelId === 'workhorse-qwen3.8-27b')?.supervises).toBe(true);
    expect(rows.filter((r) => r.supervises)).toHaveLength(1);
  });
});
