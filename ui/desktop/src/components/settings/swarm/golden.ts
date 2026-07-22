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
  /** #sink-time: max seconds a worker may stream reasoning-only (no productive tool/output/text) before it's
   *  cut as a thinking-only spiral. 0 = off. The worker timeout is idle-based and thinking resets it, so it
   *  never catches this; measured live, tasks ran 15-26 min emitting only thinking. Never touches the sink. */
  progress_watchdog_secs?: number;
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
  /** Bind the ask's OPTIONS to the spec: never offer a choice the spec already ruled out, and never ask
   *  about something the spec fixed. Default OFF.
   *  MEASURED: a run offered "SQLite / JSON / CSV" against a spec that MANDATED tab-separated storage — every
   *  option violated the spec, so no click was correct. The spec was in the prompt the whole time; the prompt
   *  fought it with a spec-independent worked example (storage question -> SQLite/JSON/plain-text) and two
   *  separate "most-common default FIRST" nudges pulling away from an unusual mandated format. */
  clarify_spec_bound?: boolean;
  /** After you answer the clarify questions, RE-PLAN with your answers folded in instead of keeping the plan
   *  drafted before you answered. Undefined = keep the plan (unchanged).
   *  MEASURED: agreement 55 + clarity 30 -> min 30 -> ask -> your answers lift clarity to 100 -> conf = 55 and
   *  AGREEMENT now binds. With the plan kept, the loop never re-enters and the retarget (which exists to fix
   *  agreement) never fires — the run built at 55 against a floor of 85. Was env-only, so the desktop could
   *  never set it, while the engine printed "(set GOOSE_SWARM_ASK_REPLAN=1 ...)" at a user who had no way to. */
  ask_replan?: boolean;
  /** The user's spec OUTRANKS any default a research pass chose for it; each appended default names the
   *  decision it settles; and the run records what was added. Default OFF.
   *  The retarget appends its findings INTO the spec under "settled defaults, do not re-ask" with no clause
   *  saying the user's words win — violating this engine's own invariant that research is ADVISORY and the
   *  spec is BINDING. Because the guess then lives inside the spec, the user's LIVE mid-run notes lose to it:
   *  they are explicitly told "it does NOT override the spec ... where they disagree, the spec wins". */
  spec_wins?: boolean;
  /** Turn budget for the integrate-verify SINK. Undefined = the same as workers (40). Floored at the worker
   *  budget, capped at 200.
   *  MEASURED across 9 runs: 5 sinks NEVER reached their own verdict, and 3 died ON the worker cap — their
   *  last words are literally "I've reached the maximum number of actions I can do without user input." The
   *  sink depends on every task and must verify the whole tree; a worker owns 1-3 files. Same budget. */
  sink_max_turns?: number;
  /** APP PILLARS: distil the app-wide acceptance criteria and inject them into EVERY worker as
   *  "NON-NEGOTIABLE". Undefined = the historical assured-only behaviour, i.e. OFF.
   *  MEASURED: this has NEVER RUN, in any run ever made. Its gate read swarm_gate("GOOSE_SWARM_GOALS", true)
   *  and that `true` is in_assured_bundle, NOT a default — so it resolved to assured_enabled(), and nothing
   *  sets GOOSE_SWARM_ASSURED. The live run's own run_started says assured:false, gates.goals:false, and
   *  `pillars` appears in ZERO logs across the whole corpus. */
  goals?: boolean;
  /** Seconds the spec-clarity probe may take (undefined = 120, clamped [30,900]).
   *  When it is abandoned the engine falls back to cross-draft agreement ALONE and asserts the product is
   *  specified — so the ask that protects an under-specified spec silently never fires. MEASURED on 2 of 14
   *  runs: both reported conf 93-95 and asked NOTHING on the same spec that made every successful-probe run
   *  report conf 30 and ask 5 questions. */
  clarity_probe_secs?: number;
  /** Fail-close the spec-clarity probe: a probe that DIED for a reason that taught us nothing (timeout /
   *  agent_error / no_final_output) counts as LOW clarity, so goose ASKS instead of proceeding on cross-draft
   *  agreement alone (which agrees perfectly on an invented product — the 2/14 conf-93-95-with-0-questions
   *  bug). `unparseable` stays fail-open (the model DID answer). Default OFF. */
  clarity_fail_closed?: boolean;
  /** Spec-contract verify (#120): after the smoke gate, start the spec's advertised `python3 -m PKG` and curl
   *  each concrete advertised GET endpoint — a 5xx / 404 / 405 on an advertised path fails the build (red +
   *  fix), an entry that never binds → unverified. Deterministic, no model. Python web apps for now. Default OFF. */
  spec_contract?: boolean;
  /** #129: after you answer the clarify ask and the deterministic rescore lifts the plan to/over the ask
   *  floor, KEEP that answered plan instead of letting a non-structural re-plan re-draft it away — a fresh
   *  draft re-lists the decisions you already answered and ships a WORSE plan below the floor (measured
   *  nf-hexohm: rescored 85, re-planned, shipped 52). A language change or a newly-defined product still
   *  re-plans. Default OFF. */
  answers_win_floor?: boolean;
  /** #121: a fleet node dropping the HTTP body mid-stream surfaces as a "Stream decode error" that the
   *  agent loop swallows into the task's text (not an error) — so a truncated verdict would be accepted as
   *  done, a silent false-green. When on, that deterministic signature is re-dispatched onto a different
   *  device so the task re-runs and produces a real result. Default ON (only fires on an actual body-drop). */
  stream_decode_retry?: boolean;
  /** #135: during plan drafting, once every draft but one has returned a valid skeleton, wait only the grace
   *  window for the lone lagging draft, then stop it and proceed on the quorum — instead of blocking the
   *  whole run on one slow node. Mihai's "if 2 of 3 finish and the 3rd lags, stop it". Default OFF. */
  straggler_stop?: boolean;
  /** #135: seconds the last lagging plan draft gets once the quorum is in, before it is stopped. Clamped to
   *  [10, draft timeout]. Blank = 45. Only used when straggler_stop is on. */
  straggler_grace_secs?: number;
  /** #135: extend straggler-stop to the CONTRACTS and DETAIL planning steps too. Separate from straggler_stop
   *  because a stopped contract/detail changes a build input — the module builds without its frozen interface
   *  (exactly like a timed-out contract; integrate-verify reconciles). At most one module degrades. Default OFF. */
  straggler_stop_degrade?: boolean;
  /** #median-slash: drop the frozen contract bundle from the final integrate-verify step's prompt. It RUNS the
   *  built app (reads the real files), so the stubs are dead context that only slows the slowest task. Default OFF. */
  sink_lean_prefill?: boolean;
  /** #122: skip the backbone-lock round-2 re-draft when round-1 cross-draft agreement is already high (>=90).
   *  The lock re-drafts the whole fleet a SECOND time (~250s) to pin the consensus modules — but when the free
   *  drafts already agree they share that structure, so the round is discarded (measured 28/29 real runs, never
   *  adopted at conf >=90). Keeps the lock for genuinely shaky plans; skips a dead planner round otherwise. Default OFF. */
  backbone_skip_confident?: boolean;
  /** #122: memoize subtask details across re-plan rounds. When goose re-drafts the plan (retarget/replan) it
   *  otherwise re-details EVERY subtask from scratch (~75s each) even when a subtask is unchanged. This reuses
   *  the detail for a subtask whose full detailer input is byte-identical to the previous round; a subtask
   *  changed by a new answer/research is correctly re-detailed. Default OFF. */
  detail_memo?: boolean;
  /** #136: stop a worker that repeats the SAME command returning the SAME output many times in a row. Measured
   *  live: a worker ran the identical `cat` 31 times and no existing guard saw it (every successful tool result
   *  resets the progress watchdog). A repeat that returns DIFFERENT output is progress and never counts, and any
   *  different command in between resets it, so an edit-then-retest cycle can never trip it. Default OFF. */
  repeat_break?: boolean;
  /** #135: cap how much REASONING one call may emit before goose stops it, in characters. This is the only
   *  supervision that exists outside the build phase — goose's judge only ever looks at build tasks, so a
   *  scout or planning step that starts repeating itself is watched by nothing. Measured: one scout emitted
   *  30,403 characters of the same paragraph while two machines sat idle. Blank/0 = off. */
  spiral_break_chars?: number;
  /** #135: let the JUDGE watch every step in every phase, not just the build steps. It looks at what a step
   *  is actually producing every minute and stops it if it is visibly repeating itself. This catches what a
   *  size limit cannot — a planning draft can legitimately reason for 50,000 characters, so only reading the
   *  text tells a deep thinker from a stuck one. It can only stop the one step; it can never fail a run. */
  omni_judge?: boolean;
  delegated_decisions_ok?: boolean;
  /** #130: flatten the planner's false-serialization dependency CHAINS between code modules into a flat fan
   *  so the fleet builds independent modules at once (a chain idles all but one node — measured 1 of 3). Safe
   *  because the frozen interface already gives every importer its siblings' signatures and only
   *  integrate-verify compiles the crate; tests and the sink join are untouched. Gated on contracts. Default OFF. */
  relax_contracted_deps?: boolean;
  /** #119 sink decomposition: split the single final integrate-verify check — which depends on every module
   *  and does ALL verification on ONE node (it stalls) — into one scoped, read-only verify task per module
   *  (these fan across the fleet like the build tasks) plus a THIN final integration run. The thin run stays
   *  the sole end-to-end gate; per-module green never substitutes for it. Default OFF (byte-identical when off). */
  fan_verify?: boolean;
  /** An app that ships NO tests currently reads as GREEN: pytest's "no tests ran" exit is treated exactly
   *  like a pass, so "nothing was checked" and "everything passed" are indistinguishable to the final gate.
   *  That also makes deleting a failing test a working way to go green. With this on, an empty suite is a
   *  finding and the build is driven to write real tests instead. Default OFF (byte-identical when off). */
  require_tests?: boolean;
  /** Give each module its OWN test subtask (depending only on that module) instead of one big test task for
   *  the whole app. Per-module tests are ready as soon as their module is, so they run alongside the rest of
   *  the build instead of queueing at the end behind the entry point. Unset = the previous behaviour (this
   *  had no setting at all before, and could never actually switch on). */
  parallel_tests?: boolean;
  dep_signatures?: boolean;
  scoped_contracts?: boolean;
  owned_file_fence?: boolean;
  contract_retry?: boolean;
  diverse_plan?: boolean;
  struct_stop?: number;
  spiral_thinking_chars?: number;
  /** Name each swarm build's chat session with a short AI-generated title (one cheap local-planner call)
   *  instead of the first-four-words truncation ("Build X — a"). Runs off the critical path so it never slows
   *  a build. Default OFF (an extra planner call can queue behind the planning burst on a busy fleet). */
  ai_session_name?: boolean;
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
  ai_session_name: true, // swarm provider default is ON — title each build's chat with a short AI name
};

// The keys a preset controls — PORTABLE tuning only. Fleet identity (endpoint, planner_model,
// devices, speed_weights, worker_extensions) and free-form sampling are intentionally NOT touched,
// so applying a preset never clobbers a machine-specific pool.
export const PRESET_KEYS: (keyof SwarmConfig)[] = [
  'worker_max_turns',
  'max_attempts',
  'worker_timeout_secs',
  'progress_watchdog_secs',
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
  // The consultation set (see GOLDEN below). In PRESET_KEYS so the preset actually applies them —
  // a value in GOLDEN that is not listed here is decoration.
  'ask_floor',
  'ask_max_q',
  'clarify_spec_bound',
  'ask_replan',
  'spec_wins',
  'sink_max_turns',
  'clarity_probe_secs',
  'no_tools_means_ask',
  'grounded_research_only',
  'contract_validate',
  'ai_session_name',
  'stream_decode_retry',
  'straggler_stop',
  'straggler_grace_secs',
  'straggler_stop_degrade',
  'sink_lean_prefill',
  'backbone_skip_confident',
  'detail_memo',
  'repeat_break',
  'spiral_break_chars',
  'omni_judge',
  'delegated_decisions_ok',
];

// GOLDEN = the exact tuning that produced the passing exploration apps. NOTE it diverges from the
// Rust defaults in several load-bearing ways: dynamic_replan OFF, best_of_n 2, max_replans 1,
// homogeneous_models ON, worker_timeout 420. The reliability GATE half of the golden formula
// (GOOSE_SWARM_ASSURED + REVIEW_REPRO + REVIEW_FIX) is env-layer only today and is the deferred,
// Rust-backed half — not settable from this panel yet.
//
// THE CONSULTATION SET, added 2026-07-17. These four are here on MECHANISM evidence — a deterministic
// engine event proving each fired — never on an A/B, because an A/B cannot decide this: Fisher's exact on
// a 1-vs-1 table returns p=1.000 for EVERY possible outcome, and at ~25-37% base failure there is a 37.5%
// chance an inert lever fabricates a win. Detecting a real effect needs ~46 runs. So the bar for entry is:
// (1) the engine event proves it fired, and (2) it is STRUCTURALLY INCAPABLE of making the app worse.
// All four clear both. Measured on the same spec, three configurations:
//     levers off (what shipped):  5 open decisions · 0 asked · 5 INVENTED · confidence laundered 30 -> 96
//     levers on, ask cap 3:       5 open · 3 asked · 2 invented in silence · confidence 30 (honest)
//     levers on, ask_max_q 6:     5 open · 5 asked · 0 invented
// The spec had exactly five "DELIBERATELY NOT DECIDED — do NOT guess them" items. The shipping default
// guessed all five and reported 96/100 confidence about it.
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
  // Never build below the bar; ask instead. (The engine adds +5 for a weak planner, so 80 -> 85 on a 27B.)
  ask_floor: 80,
  // Was 3, and it TRUNCATED with a bare .take(): two of five decisions were invented anyway, silently.
  // A cap that discards user consultation is not a safety limit. 6 covers a spec's realistic open set.
  ask_max_q: 6,
  // Asking the user is only worth anything if the OPTIONS are answerable. A run offered "SQLite / JSON /
  // CSV" against a spec that mandated tab-separated storage: five questions asked, and on that one there
  // was no correct click. Consulting the user with illegal choices is worse than not asking — it launders
  // a spec violation into "the user chose it".
  clarify_spec_bound: true,
  // Consulting the user is pointless if the engine can overwrite the answer. The retarget writes researched
  // guesses INTO the spec as "settled defaults, do not re-ask" — and the user's own live notes are told the
  // spec wins, so the guess beats the human typing the truth at it. This makes those defaults subordinate,
  // labelled, and recorded. Same family as clarify_spec_bound: nothing may edit a FIXED requirement.
  spec_wins: true,
  // The verifier must be able to FINISH. 3 of 9 sinks died on the worker cap of 40 mid-verify — and the run
  // still reported a verdict. A verifier that was cut off has not verified anything, and "verified" on an
  // interrupted check is the false green this whole formula exists to prevent. 120 is not a measured optimum;
  // it is enough headroom that the cap stops being the thing that ends the check.
  sink_max_turns: 120,
  // The probe that decides whether to ASK died on 2 of 14 runs and took the entire consultation with it
  // (conf 95, zero questions, product invented). The fleet is PARALLEL:1, so the probe queues behind the
  // drafts it runs alongside and can burn 120s without reaching the model. 300 buys it a real chance; the
  // engine now records WHY it failed, so this number can be judged on evidence rather than on my hunch.
  clarity_probe_secs: 300,
  // With no lookup tools nothing is lookupable, so an open decision goes to the USER, not to a research
  // round that can only guess. Cost: a pause. It cannot worsen the app — it replaces an invented answer
  // with the user's real one.
  no_tools_means_ask: true,
  // The same failure through the second door: an INVENTED finding may no longer mark a decision "settled".
  // Measured: research reported "0 actually looked up, 5 counted as settled" and that lifted the meter
  // over the floor, silencing the ask for the whole run.
  grounded_research_only: true,
  // Pure annotation — it only adds `validated` to an event that already fires, so it cannot change a
  // build. Its FIRST measurement falsified the engine's own `frozen: true`: both validated contract stubs
  // failed to parse (invalid syntax, public: []). Every worker is told to honour those stubs.
  contract_validate: true,
  ai_session_name: true,
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
