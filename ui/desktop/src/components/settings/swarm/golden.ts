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
  /** Wall-clock BACKSTOP for a scout, not its budget — scout_max_lookups is the real control. */
  scout_budget_secs?: number;
  /** How many tool calls (web searches / doc lookups) a research scout may spend. THE scout budget. */
  scout_max_lookups?: number;
  max_tool_response_chars?: number | null;
  temperature?: number | null;
  top_p?: number | null;
  top_k?: number | null;
  min_p?: number | null;
  repeat_penalty?: number | null;
  /** Per-node task-share weights, keyed by a substring of the device id (e.g. {"gabee":1,"mihai":2}).
   *  Higher = a larger share of tasks; the scheduler's speed_weight_for() substring-matches these. */
  speed_weights?: Record<string, number>;
  /** Confidence floor (1-100) below which the swarm asks the USER clarifying questions before building,
   *  instead of guessing. 0 / undefined = never ask. Surfaced live in the run panel's clarify prompt. */
  ask_floor?: number;
  /** How many clarifying questions goose may ask at once. Default 3.
   *  MEASURED: a run's probe found FIVE material open decisions — every one of them on the spec's explicit
   *  "do NOT guess them" list — and the cap of 3 meant two were guessed anyway, silently. This cap is the
   *  difference between asking about the user's product and inventing part of it. */
  ask_max_q?: number;
  /** Convergence molding — steer the weak planner to one canonical decomposition + role-normalize the
   *  agreement metric. The proven confidence raiser. Default ON. */
  converge?: boolean;
  /** Dynamic confidence-retarget loop: when plan confidence is below ask_floor, re-draft toward consensus
   *  or research the open decisions BEFORE the one-shot ask. Needs a floor. Default OFF (experimental). */
  retarget?: boolean;
  /** Two-stage backbone-lock: extract the majority-consensus module set across drafts, lock it, and re-draft
   *  so the fleet's plans genuinely converge. Default OFF (experimental). */
  backbone?: boolean;
  /** Draft plan skeletons at this temperature (steadies structural drafting). Blank = model default. */
  draft_temp?: number | null;
  /** A crash the repro oracle PROVED (twice-run traceback in a clean snapshot) and the fix loop did not
   *  repair demotes the run's `verified` claim. Never flips `passed` red. Default OFF; inert unless the
   *  repro oracle is on. */
  repro_demotes_verified?: boolean;
  /** Inject the DOMAIN_PITFALLS facts relevant to a subtask into the WORKER's prompt, so the author is
   *  told the convention BEFORE writing rather than only reviewed against it afterwards. Default OFF. */
  author_pitfalls?: boolean;
  /** Run the model-free AST wiring review (built-but-unwired modules, stub functions) after the build.
   *  Was previously reachable only via the assured bundle. Default OFF. */
  review?: boolean;
  /** A newly-unwired PURE-LIBRARY module the wire-fix did not resolve demotes the run's `verified` claim.
   *  Never flips `passed` red. Requires `review`. Default OFF. */
  unwired_demotes_verified?: boolean;
  /** Only a GROUNDED research finding (agent actually called web-search/context7/shell) may mark an open
   *  decision "settled". An invented finding stays as context but no longer suppresses the clarifying ask,
   *  so a guessed product decision still gets put to the user. Default OFF. */
  grounded_research_only?: boolean;
  /** Run the project's own `npm test` in the TypeScript smoke gate. Python already runs pytest; TS was gated
   *  on `npm run build` alone, so a TS app with a failing suite shipped as verified. Default OFF. */
  ts_smoke_tests?: boolean;
  /** A failed planned task blocks the green claim and drives the completion fix loop. The loop only reads the
   *  smoke gate today, so a task can fail outright while the run still reports verified. Default OFF. */
  failed_tasks_block_green?: boolean;
  /** Give the integrate-verify sink the built entry's REAL --help before it writes its golden checks, so it
   *  targets the interface the app actually has instead of the one the spec describes. Default OFF. */
  sink_prebuild?: boolean;
  /** LEARN & REFLECT: after a build that provably worked, goose reflects and writes a reusable per-STACK
   *  skill, then starts from it on the next build of that stack. Structural only, advisory only, and the
   *  skill is a plain markdown file the user can edit or delete. Default OFF. */
  persona?: boolean;
  /** Let the user add background notes WHILE a build runs; they are folded into the next dispatched worker,
   *  so a live worker is never disturbed. Advisory — the spec always wins. Default OFF. */
  user_notes?: boolean;
  /** Parse the frozen contract stubs and record what was frozen in the contracts event. Today the bundle is
   *  accepted raw on a non-empty check and never persisted, so it cannot be audited. Measures, gates nothing.
   *  Default OFF. */
  contract_validate?: boolean;
  /** Stop the confidence re-draft ladder once a round fails to beat the best confidence already measured.
   *  MEASURED: a run went 84 -> 70 -> 70 -> 52 over three re-draft rounds (~60 min of the whole fleet) and
   *  shipped the round-2 plan anyway. Cannot lower quality — the best plan is kept regardless. Default OFF. */
  retarget_stall_guard?: boolean;
  /** After the build, statically check that no module reads a field off a sibling's class that the class does
   *  not define. MEASURED: a run shipped an expense splitter whose POST /api/expenses 500'd on every call —
   *  api.py read `body.group_id` while models.py never declared it — and reported verified, because the tests
   *  only touched the pure module and the smoke gate only checks that the file imports. Default OFF. */
  cross_module_check?: boolean;
  /** When the run has NO lookup tools, put an open decision to the USER instead of to a research round that
   *  cannot look anything up. MEASURED: with no tools configured the engine still sent 5 decisions to
   *  research as kind:"web" ("Use the web-search tool.") and counted all 5 guesses as settled — silencing the
   *  clarifying ask for 90 minutes. The user answered them in 1.8 min. Default OFF. */
  no_tools_means_ask?: boolean;
  [k: string]: unknown; // preserve fields we don't edit (devices, worker_extensions, …)
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
  scout_budget_secs: 900,
  scout_max_lookups: 10,
  converge: true, // default_converge (swarm.rs) — the proven agreement raiser, ON by default
  // Mihai: "don't let goose start implementing something below 80 — research until confidence is > 80."
  // Floor 80 + retarget ON so a sub-80 plan RE-DRAFTS toward consensus (and asks) to raise the meter before
  // EXECUTE, instead of building a low-confidence plan. Backbone on too — the structural convergence lever.
  ask_floor: 80,
  retarget: true,
  backbone: true,
  ask_max_q: 3, // swarm.rs ask_max_q — .unwrap_or(3); anything past the cap is guessed, not asked
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
