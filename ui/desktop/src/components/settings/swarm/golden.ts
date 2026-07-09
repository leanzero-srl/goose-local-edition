// Swarm tunables: the type, the faithful defaults, and the GOLDEN formula preset.
//
// DEFAULTS mirror the Rust serde defaults in crates/goose-cli/src/commands/swarm.rs so the panel
// baseline is truthful about what goose actually does when a field is absent from config.yaml.
// GOLDEN is the tested-working tuning profile (the config.yaml swarm: block that produced the
// passing swarm-gym exploration apps on the 3-node qwopus fleet) — hard-won tuning, one click away.

export const RESEARCH_MODES = ['off', 'on', 'auto'] as const;
export type ResearchMode = (typeof RESEARCH_MODES)[number];

export interface SwarmConfig {
  endpoint?: string;
  planner_model?: string;
  worker_max_turns?: number;
  max_attempts?: number;
  worker_timeout_secs?: number;
  planner_timeout_secs?: number;
  context_cap?: number | null;
  research_planning?: ResearchMode;
  parallel_planning?: boolean;
  dynamic_replan?: boolean;
  max_research_questions?: number;
  max_replans?: number;
  research_scouts?: boolean;
  best_of_n_skeletons?: number;
  planner_also_works?: boolean;
  planner_weight?: number;
  homogeneous_models?: boolean;
  allow_model_load?: boolean;
  scout_budget_secs?: number;
  max_tool_response_chars?: number | null;
  temperature?: number | null;
  top_p?: number | null;
  top_k?: number | null;
  min_p?: number | null;
  repeat_penalty?: number | null;
  [k: string]: unknown; // preserve fields we don't edit (devices, speed_weights, worker_extensions, …)
}

// Panel baseline = faithful to the Rust `Default for SwarmConfig` (swarm.rs:240-290).
export const DEFAULTS: SwarmConfig = {
  endpoint: 'http://localhost:1234',
  planner_model: 'qwen/qwen3.6-27b',
  worker_max_turns: 40,
  max_attempts: 3,
  worker_timeout_secs: 900, // default_worker_timeout_secs (swarm.rs:82); panel previously wrongly showed 420
  planner_timeout_secs: 900, // default_planner_timeout_secs (swarm.rs:88)
  research_planning: 'on',
  parallel_planning: true,
  dynamic_replan: true,
  max_research_questions: 4,
  max_replans: 2,
  research_scouts: true,
  best_of_n_skeletons: 1,
  planner_also_works: true,
  planner_weight: 1,
  homogeneous_models: false,
  allow_model_load: false,
  scout_budget_secs: 120,
};

// The keys a preset controls — PORTABLE tuning only. Fleet identity (endpoint, planner_model,
// devices, speed_weights, worker_extensions) and free-form sampling are intentionally NOT touched,
// so applying a preset never clobbers a machine-specific pool.
export const PRESET_KEYS: (keyof SwarmConfig)[] = [
  'worker_max_turns',
  'max_attempts',
  'worker_timeout_secs',
  'planner_timeout_secs',
  'research_planning',
  'parallel_planning',
  'dynamic_replan',
  'max_research_questions',
  'max_replans',
  'research_scouts',
  'best_of_n_skeletons',
  'planner_also_works',
  'planner_weight',
  'homogeneous_models',
  'allow_model_load',
];

// GOLDEN = the exact tuning that produced the passing exploration apps. NOTE it diverges from the
// Rust defaults in several load-bearing ways: dynamic_replan OFF, best_of_n 2, max_replans 1,
// homogeneous_models ON, worker_timeout 420. The reliability GATE half of the golden formula
// (GOOSE_SWARM_ASSURED + REVIEW_REPRO + REVIEW_FIX) is env-layer only today and is the deferred,
// Rust-backed half — not settable from this panel yet.
export const GOLDEN: SwarmConfig = {
  worker_max_turns: 40,
  max_attempts: 3,
  worker_timeout_secs: 420,
  planner_timeout_secs: 900,
  research_planning: 'on',
  parallel_planning: true,
  dynamic_replan: false,
  max_research_questions: 4,
  max_replans: 1,
  research_scouts: true,
  best_of_n_skeletons: 2,
  planner_also_works: true,
  planner_weight: 1,
  homogeneous_models: true,
  allow_model_load: false,
};

export type PresetId = 'golden' | 'default' | 'custom';

export const PRESETS: { id: 'golden' | 'default'; label: string; values: SwarmConfig }[] = [
  { id: 'golden', label: 'Golden formula', values: GOLDEN },
  { id: 'default', label: 'Defaults', values: DEFAULTS },
];

/** Which preset (if any) the current config's tunable subset exactly matches. */
export function detectPreset(cfg: SwarmConfig): PresetId {
  const matches = (p: SwarmConfig) =>
    PRESET_KEYS.every((k) => {
      const a = cfg[k];
      const b = p[k];
      // treat undefined on the config side as "not diverging" only if the preset also omits it
      return a === b || (a == null && b == null);
    });
  if (matches(GOLDEN)) return 'golden';
  if (matches(DEFAULTS)) return 'default';
  return 'custom';
}

/** A preset's values restricted to the PRESET_KEYS (never touches fleet identity / sampling). */
export function presetPatch(values: SwarmConfig): Partial<SwarmConfig> {
  const patch: Partial<SwarmConfig> = {};
  for (const k of PRESET_KEYS) {
    if (values[k] !== undefined) (patch as Record<string, unknown>)[k] = values[k];
  }
  return patch;
}
