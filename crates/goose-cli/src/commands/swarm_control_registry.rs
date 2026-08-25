use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::swarm::SwarmConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlDisposition {
    RetainEnabled,
    RetainDisabled,
    Modify,
    RemoveMerge,
    RuntimeProfile,
}

impl ControlDisposition {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RetainEnabled => "retain_enabled",
            Self::RetainDisabled => "retain_disabled",
            Self::Modify => "modify",
            Self::RemoveMerge => "remove_merge",
            Self::RuntimeProfile => "runtime_profile",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CampaignRole {
    Behavior,
    RuntimeProfile,
    Removal,
    Telemetry,
}

impl CampaignRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Behavior => "behavior",
            Self::RuntimeProfile => "runtime_profile",
            Self::Removal => "removal",
            Self::Telemetry => "telemetry",
        }
    }
}

const fn campaign_role(disposition: ControlDisposition) -> CampaignRole {
    match disposition {
        ControlDisposition::RetainEnabled
        | ControlDisposition::RetainDisabled
        | ControlDisposition::Modify => CampaignRole::Behavior,
        ControlDisposition::RemoveMerge => CampaignRole::Removal,
        ControlDisposition::RuntimeProfile => CampaignRole::RuntimeProfile,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfigControlSpec {
    pub canonical: &'static str,
    pub disposition: ControlDisposition,
    campaign_role: CampaignRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EnvironmentOnlyControlSpec {
    pub canonical: &'static str,
    pub environment: &'static str,
    pub disposition: ControlDisposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ControlAlias {
    pub alias: &'static str,
    pub canonical: &'static str,
}

const fn config(canonical: &'static str, disposition: ControlDisposition) -> ConfigControlSpec {
    ConfigControlSpec {
        canonical,
        disposition,
        campaign_role: campaign_role(disposition),
    }
}

const fn telemetry(canonical: &'static str, disposition: ControlDisposition) -> ConfigControlSpec {
    ConfigControlSpec {
        canonical,
        disposition,
        campaign_role: CampaignRole::Telemetry,
    }
}

const fn environment_only(
    canonical: &'static str,
    environment: &'static str,
    disposition: ControlDisposition,
) -> EnvironmentOnlyControlSpec {
    EnvironmentOnlyControlSpec {
        canonical,
        environment,
        disposition,
    }
}

/// Source of truth for every persisted `SwarmConfig` field.
///
/// This is intentionally exhaustive rather than a UI/campaign allowlist. A field that exists but is
/// absent here is configuration the engine cannot account for, so the coverage test fails the build.
pub(crate) const CONFIG_CONTROLS: &[ConfigControlSpec] = &[
    // Retain enabled (27).
    config("stream_decode_retry", ControlDisposition::RetainEnabled),
    config("planner_also_works", ControlDisposition::RetainEnabled),
    config("sink_lean_prefill", ControlDisposition::RetainEnabled),
    config("e2e_oracle", ControlDisposition::RetainEnabled),
    config("spec_sized_plan", ControlDisposition::RetainEnabled),
    config("delegated_decisions_ok", ControlDisposition::RetainEnabled),
    config("clarify_spec_bound", ControlDisposition::RetainEnabled),
    config("spec_wins", ControlDisposition::RemoveMerge),
    config("clarity_fail_closed", ControlDisposition::RetainEnabled),
    config("spec_contract", ControlDisposition::RetainEnabled),
    config("retarget_stall_guard", ControlDisposition::RetainEnabled),
    config("answers_win_floor", ControlDisposition::RetainEnabled),
    config("cross_module_check", ControlDisposition::RetainEnabled),
    config("smoke", ControlDisposition::RetainEnabled),
    config("verify_commands", ControlDisposition::RetainEnabled),
    config("fan_e2e", ControlDisposition::RetainEnabled),
    config("no_tools_means_ask", ControlDisposition::RemoveMerge),
    config("author_pitfalls", ControlDisposition::RetainEnabled),
    config("grounded_research_only", ControlDisposition::RemoveMerge),
    config("ts_smoke_tests", ControlDisposition::RetainEnabled),
    config(
        "failed_tasks_block_green",
        ControlDisposition::RetainEnabled,
    ),
    config("sink_prebuild", ControlDisposition::RetainEnabled),
    config("user_notes", ControlDisposition::RetainEnabled),
    config("contract_validate", ControlDisposition::RetainEnabled),
    config("kind_prompt", ControlDisposition::RetainEnabled),
    telemetry("occupancy", ControlDisposition::RetainEnabled),
    config("doc_prefetch", ControlDisposition::RetainEnabled),
    config("dep_signatures", ControlDisposition::RetainEnabled),
    config("act_now_nudge", ControlDisposition::RetainEnabled),
    config("require_tests", ControlDisposition::RetainEnabled),
    // Retain disabled pending evidence (8).
    config("straggler_stop_degrade", ControlDisposition::RetainDisabled),
    config("goals", ControlDisposition::RetainDisabled),
    config("ask_replan", ControlDisposition::RetainDisabled),
    config("contract_retry", ControlDisposition::RetainDisabled),
    config("incremental_replan", ControlDisposition::RetainDisabled),
    config("ask_away", ControlDisposition::RetainDisabled),
    config("write_first", ControlDisposition::RetainDisabled),
    config("think_off_test_authors", ControlDisposition::RetainDisabled),
    // Modify before another causal arm (31).
    config("max_attempts", ControlDisposition::Modify),
    config("max_research_questions", ControlDisposition::Modify),
    config("dynamic_replan", ControlDisposition::Modify),
    config("max_replans", ControlDisposition::Modify),
    config("research_scouts", ControlDisposition::Modify),
    config("parallel_planning", ControlDisposition::Modify),
    config("best_of_n_skeletons", ControlDisposition::Modify),
    config("progress_watchdog_secs", ControlDisposition::Modify),
    config("omni_judge", ControlDisposition::Modify),
    config("converge", ControlDisposition::Modify),
    config("diverse_plan", ControlDisposition::Modify),
    config("retarget", ControlDisposition::Modify),
    config("supervision_pool", ControlDisposition::Modify),
    config("judge_nudge", ControlDisposition::Modify),
    config("fix_sched", ControlDisposition::Modify),
    config("ask_max_q", ControlDisposition::Modify),
    config("split", ControlDisposition::Modify),
    config("contracts", ControlDisposition::Modify),
    config("complete", ControlDisposition::Modify),
    config("backbone", ControlDisposition::Modify),
    config("review", ControlDisposition::Modify),
    config("unwired_demotes_verified", ControlDisposition::Modify),
    config("persona", ControlDisposition::Modify),
    config("relax_contracted_deps", ControlDisposition::Modify),
    config("split_fat", ControlDisposition::Modify),
    config("doc_fetch", ControlDisposition::RemoveMerge),
    config("fan_verify", ControlDisposition::Modify),
    config("parallel_tests", ControlDisposition::Modify),
    config("repeat_break", ControlDisposition::Modify),
    config("straggler_stop", ControlDisposition::Modify),
    config("backbone_skip_confident", ControlDisposition::Modify),
    config("degrade_on_stall", ControlDisposition::Modify),
    // Remove or merge (16).
    config("sink_review", ControlDisposition::RemoveMerge),
    config("detail_memo", ControlDisposition::RemoveMerge),
    config("spiral_break_chars", ControlDisposition::RemoveMerge),
    config("homogeneous_models", ControlDisposition::RemoveMerge),
    config("speed_weights", ControlDisposition::RemoveMerge),
    config("delivery", ControlDisposition::RemoveMerge),
    config("owned_file_fence", ControlDisposition::RemoveMerge),
    config("spiral_thinking_chars", ControlDisposition::RemoveMerge),
    config("read_on_fix", ControlDisposition::RemoveMerge),
    config("force_write_tool", ControlDisposition::RemoveMerge),
    config("scoped_contracts", ControlDisposition::RemoveMerge),
    config("split_secs", ControlDisposition::RemoveMerge),
    // Runtime/profile inputs, not causal arms (34).
    config("endpoint", ControlDisposition::RuntimeProfile),
    config("planner_model", ControlDisposition::RuntimeProfile),
    config("devices", ControlDisposition::RuntimeProfile),
    config("worker_max_turns", ControlDisposition::RuntimeProfile),
    config("straggler_grace_secs", ControlDisposition::RuntimeProfile),
    config("worker_extensions", ControlDisposition::RuntimeProfile),
    config("planner_weight", ControlDisposition::RuntimeProfile),
    config("context_cap", ControlDisposition::RuntimeProfile),
    config("research_planning", ControlDisposition::RuntimeProfile),
    config("worker_timeout_secs", ControlDisposition::RuntimeProfile),
    config("planner_timeout_secs", ControlDisposition::RuntimeProfile),
    config("allow_model_load", ControlDisposition::RuntimeProfile),
    config("temperature", ControlDisposition::RuntimeProfile),
    config("top_p", ControlDisposition::RuntimeProfile),
    config("top_k", ControlDisposition::RuntimeProfile),
    config("min_p", ControlDisposition::RuntimeProfile),
    config("repeat_penalty", ControlDisposition::RuntimeProfile),
    config(
        "max_tool_response_chars",
        ControlDisposition::RuntimeProfile,
    ),
    config("scout_budget_secs", ControlDisposition::RuntimeProfile),
    config("scout_max_lookups", ControlDisposition::RuntimeProfile),
    config("sink_cap_secs", ControlDisposition::RuntimeProfile),
    config("sink_cap_ref_bytes", ControlDisposition::RuntimeProfile),
    config("uncapped", ControlDisposition::RuntimeProfile),
    config("lm_extra_body", ControlDisposition::RuntimeProfile),
    config("ask_floor", ControlDisposition::RuntimeProfile),
    config("struct_stop", ControlDisposition::RuntimeProfile),
    config("clarity_probe_secs", ControlDisposition::RuntimeProfile),
    config("sink_max_turns", ControlDisposition::RuntimeProfile),
    config("draft_timeout_secs", ControlDisposition::RuntimeProfile),
    config("retarget_rounds", ControlDisposition::RuntimeProfile),
    config("complete_cap_secs", ControlDisposition::RuntimeProfile),
    config("draft_temp", ControlDisposition::RuntimeProfile),
    config("ask_rounds_max", ControlDisposition::RuntimeProfile),
    config("research_tools", ControlDisposition::RuntimeProfile),
];

/// Controls that genuinely have no `SwarmConfig` field. Their disposition remains explicit, but they are
/// never smuggled into a config-field allowlist. The exact reader-set test prevents stale names living here.
pub(crate) const ENVIRONMENT_ONLY_CONTROLS: &[EnvironmentOnlyControlSpec] = &[
    // Retain enabled (14).
    environment_only(
        "boundary_probe",
        "GOOSE_SWARM_BOUNDARY_PROBE",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "cli_contract",
        "GOOSE_SWARM_CLI_CONTRACT",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "compile_gate",
        "GOOSE_SWARM_COMPILE_GATE",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "css_coherence",
        "GOOSE_SWARM_CSS_COHERENCE",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "dom_id_scan",
        "GOOSE_SWARM_DOM_ID_SCAN",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "done_gate",
        "GOOSE_SWARM_DONE_GATE",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "overview",
        "GOOSE_SWARM_OVERVIEW",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "pillar_flow",
        "GOOSE_SWARM_PILLAR_FLOW",
        ControlDisposition::RetainEnabled,
    ),
    environment_only("qa", "GOOSE_SWARM_QA", ControlDisposition::RetainEnabled),
    environment_only(
        "require_servable",
        "GOOSE_SWARM_REQUIRE_SERVABLE",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "resume",
        "GOOSE_SWARM_RESUME",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "salvage_require_critical",
        "GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "skeleton_first",
        "GOOSE_SWARM_SKELETON_FIRST",
        ControlDisposition::RetainEnabled,
    ),
    environment_only(
        "split_inherit_spec",
        "GOOSE_SWARM_SPLIT_INHERIT_SPEC",
        ControlDisposition::RetainEnabled,
    ),
    // Retain disabled pending evidence (3).
    environment_only(
        "physical_broker",
        "GOOSE_SWARM_PHYSICAL_BROKER",
        ControlDisposition::RetainDisabled,
    ),
    environment_only(
        "speculate",
        "GOOSE_SWARM_SPECULATE",
        ControlDisposition::RetainDisabled,
    ),
    environment_only(
        "testgen",
        "GOOSE_SWARM_TESTGEN",
        ControlDisposition::RetainDisabled,
    ),
    // Modify before another causal arm (9).
    environment_only(
        "complete_rounds",
        "GOOSE_SWARM_COMPLETE_ROUNDS",
        ControlDisposition::Modify,
    ),
    environment_only(
        "complete_stall_rounds",
        "GOOSE_SWARM_COMPLETE_STALL_ROUNDS",
        ControlDisposition::Modify,
    ),
    environment_only("judge", "GOOSE_SWARM_JUDGE", ControlDisposition::Modify),
    environment_only(
        "prereview",
        "GOOSE_SWARM_PREREVIEW",
        ControlDisposition::Modify,
    ),
    environment_only(
        "salvage_spin",
        "GOOSE_SWARM_SALVAGE_SPIN",
        ControlDisposition::Modify,
    ),
    environment_only(
        "ship_best",
        "GOOSE_SWARM_SHIP_BEST",
        ControlDisposition::Modify,
    ),
    environment_only(
        "sink_shard",
        "GOOSE_SWARM_SINK_SHARD",
        ControlDisposition::Modify,
    ),
    environment_only(
        "spec_repair",
        "GOOSE_SWARM_SPEC_REPAIR",
        ControlDisposition::Modify,
    ),
    environment_only(
        "tail_review",
        "GOOSE_SWARM_TAIL_REVIEW",
        ControlDisposition::Modify,
    ),
    // Remove or merge (8).
    environment_only(
        "ask_scale",
        "GOOSE_SWARM_ASK_SCALE",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "assured",
        "GOOSE_SWARM_ASSURED",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "complete_parallel",
        "GOOSE_SWARM_COMPLETE_PARALLEL",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "fill_fan",
        "GOOSE_SWARM_FILL_FAN",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "prereview_dims",
        "GOOSE_SWARM_PREREVIEW_DIMS",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "probe_advertised_post",
        "GOOSE_SWARM_PROBE_ADVERTISED_POST",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "split_fat_files",
        "GOOSE_SWARM_SPLIT_FAT_FILES",
        ControlDisposition::RemoveMerge,
    ),
    environment_only(
        "web_vocab",
        "GOOSE_SWARM_WEB_VOCAB",
        ControlDisposition::RemoveMerge,
    ),
    // Runtime/profile inputs (15).
    environment_only(
        "ai_name",
        "GOOSE_SWARM_AI_NAME",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "ask_file",
        "GOOSE_SWARM_ASK_FILE",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "ask_wait_secs",
        "GOOSE_SWARM_ASK_WAIT_SECS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "fix_cap_secs",
        "GOOSE_SWARM_FIX_CAP_SECS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "inherit_hints",
        "GOOSE_SWARM_INHERIT_HINTS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "max_nodes",
        "GOOSE_SWARM_MAX_NODES",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "name_timeout_secs",
        "GOOSE_SWARM_NAME_TIMEOUT_SECS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "pin_device",
        "GOOSE_SWARM_PIN_DEVICE",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "render_node",
        "GOOSE_SWARM_RENDER_NODE",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "render_probe",
        "GOOSE_SWARM_RENDER_PROBE",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "retarget_draft_step",
        "GOOSE_SWARM_RETARGET_DRAFT_STEP",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "retarget_stall_tolerance",
        "GOOSE_SWARM_RETARGET_STALL_TOLERANCE",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "run_deadline_unix_ms",
        "GOOSE_SWARM_RUN_DEADLINE_UNIX_MS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "tail_review_secs",
        "GOOSE_SWARM_TAIL_REVIEW_SECS",
        ControlDisposition::RuntimeProfile,
    ),
    environment_only(
        "telemetry_file",
        "GOOSE_SWARM_TELEMETRY_FILE",
        ControlDisposition::RuntimeProfile,
    ),
];

/// Environment-only controls whose execution value is available in `levers_resolved.levers`. The remaining
/// rows are still real, registered controls, but consumers must use their phase-specific events until their
/// execution resolver is shared with the run-level echo.
pub(crate) const EFFECTIVE_ENVIRONMENT_ONLY_ECHOES: &[&str] = &[
    "judge",
    "pillar_flow",
    "prereview",
    "qa",
    "salvage_require_critical",
    "salvage_spin",
    "ship_best",
    "sink_shard",
    "spec_repair",
    "split_inherit_spec",
    "tail_review",
    "testgen",
];

/// Accepted historical/operator spellings. Events and new tooling emit only the canonical name.
pub(crate) const CONTROL_ALIASES: &[ControlAlias] = &[
    ControlAlias {
        alias: "act_now",
        canonical: "act_now_nudge",
    },
    ControlAlias {
        alias: "ask_maxq",
        canonical: "ask_max_q",
    },
    ControlAlias {
        alias: "ask_rounds",
        canonical: "ask_rounds_max",
    },
    ControlAlias {
        alias: "delegated_ok",
        canonical: "delegated_decisions_ok",
    },
    ControlAlias {
        alias: "dynamic_replan_cfg",
        canonical: "dynamic_replan",
    },
    ControlAlias {
        alias: "force_write",
        canonical: "force_write_tool",
    },
    ControlAlias {
        alias: "stream_retry",
        canonical: "stream_decode_retry",
    },
    ControlAlias {
        alias: "temp",
        canonical: "temperature",
    },
    ControlAlias {
        alias: "think_off",
        canonical: "think_off_test_authors",
    },
];

/// Every literal production reader, including readers reached through `swarm_gate*` helpers. This list is
/// checked bidirectionally against the Rust source: a new reader without metadata and a stale catalog-only
/// name both fail.
pub(crate) const SWARM_ENV_READERS: &[&str] = &[
    "GOOSE_SWARM_ACT_NOW",
    "GOOSE_SWARM_AI_NAME",
    "GOOSE_SWARM_ANSWERS_WIN_FLOOR",
    "GOOSE_SWARM_ASK_AWAY",
    "GOOSE_SWARM_ASK_FILE",
    "GOOSE_SWARM_ASK_FLOOR",
    "GOOSE_SWARM_ASK_MAXQ",
    "GOOSE_SWARM_ASK_REPLAN",
    "GOOSE_SWARM_ASK_ROUNDS",
    "GOOSE_SWARM_ASK_SCALE",
    "GOOSE_SWARM_ASK_WAIT_SECS",
    "GOOSE_SWARM_ASSURED",
    "GOOSE_SWARM_AUTHOR_PITFALLS",
    "GOOSE_SWARM_BACKBONE",
    "GOOSE_SWARM_BACKBONE_SKIP_CONFIDENT",
    "GOOSE_SWARM_BOUNDARY_PROBE",
    "GOOSE_SWARM_CLARIFY_SPEC_BOUND",
    "GOOSE_SWARM_CLARITY_FAIL_CLOSED",
    "GOOSE_SWARM_CLARITY_PROBE_SECS",
    "GOOSE_SWARM_CLI_CONTRACT",
    "GOOSE_SWARM_COMPILE_GATE",
    "GOOSE_SWARM_COMPLETE",
    "GOOSE_SWARM_COMPLETE_CAP_SECS",
    "GOOSE_SWARM_COMPLETE_PARALLEL",
    "GOOSE_SWARM_COMPLETE_ROUNDS",
    "GOOSE_SWARM_COMPLETE_STALL_ROUNDS",
    "GOOSE_SWARM_CONTRACTS",
    "GOOSE_SWARM_CONTRACT_RETRY",
    "GOOSE_SWARM_CONTRACT_VALIDATE",
    "GOOSE_SWARM_CONVERGE",
    "GOOSE_SWARM_CROSS_MODULE_CHECK",
    "GOOSE_SWARM_CSS_COHERENCE",
    "GOOSE_SWARM_DEGRADE_ON_STALL",
    "GOOSE_SWARM_DELEGATED_OK",
    "GOOSE_SWARM_DELIVERY",
    "GOOSE_SWARM_DEP_SIGNATURES",
    "GOOSE_SWARM_DETAIL_MEMO",
    "GOOSE_SWARM_DIVERSE_PLAN",
    "GOOSE_SWARM_DOC_PREFETCH",
    "GOOSE_SWARM_DOM_ID_SCAN",
    "GOOSE_SWARM_DONE_GATE",
    "GOOSE_SWARM_DRAFT_TEMP",
    "GOOSE_SWARM_DRAFT_TIMEOUT_SECS",
    "GOOSE_SWARM_E2E_ORACLE",
    "GOOSE_SWARM_FAILED_TASKS_BLOCK_GREEN",
    "GOOSE_SWARM_FAN_E2E",
    "GOOSE_SWARM_FAN_VERIFY",
    "GOOSE_SWARM_FILL_FAN",
    "GOOSE_SWARM_FIX_CAP_SECS",
    "GOOSE_SWARM_FIX_SCHED",
    "GOOSE_SWARM_FORCE_WRITE",
    "GOOSE_SWARM_GOALS",
    "GOOSE_SWARM_INCREMENTAL_REPLAN",
    "GOOSE_SWARM_INHERIT_HINTS",
    "GOOSE_SWARM_JUDGE",
    "GOOSE_SWARM_JUDGE_NUDGE",
    "GOOSE_SWARM_KIND_PROMPT",
    "GOOSE_SWARM_MAX_NODES",
    "GOOSE_SWARM_MIN_P",
    "GOOSE_SWARM_NAME_TIMEOUT_SECS",
    "GOOSE_SWARM_OCCUPANCY",
    "GOOSE_SWARM_OMNI_JUDGE",
    "GOOSE_SWARM_OVERVIEW",
    "GOOSE_SWARM_OWNED_FILE_FENCE",
    "GOOSE_SWARM_PARALLEL_TESTS",
    "GOOSE_SWARM_PERSONA",
    "GOOSE_SWARM_PHYSICAL_BROKER",
    "GOOSE_SWARM_PILLAR_FLOW",
    "GOOSE_SWARM_PIN_DEVICE",
    "GOOSE_SWARM_PLANNER_ALSO_WORKS",
    "GOOSE_SWARM_PREREVIEW",
    "GOOSE_SWARM_PREREVIEW_DIMS",
    "GOOSE_SWARM_PROBE_ADVERTISED_POST",
    "GOOSE_SWARM_PROGRESS_WATCHDOG_SECS",
    "GOOSE_SWARM_QA",
    "GOOSE_SWARM_READ_ON_FIX",
    "GOOSE_SWARM_RELAX_CONTRACTED_DEPS",
    "GOOSE_SWARM_RENDER_NODE",
    "GOOSE_SWARM_RENDER_PROBE",
    "GOOSE_SWARM_REPEAT_BREAK",
    "GOOSE_SWARM_REPEAT_PENALTY",
    "GOOSE_SWARM_REQUIRE_SERVABLE",
    "GOOSE_SWARM_REQUIRE_TESTS",
    "GOOSE_SWARM_RESEARCH_TOOLS",
    "GOOSE_SWARM_RESUME",
    "GOOSE_SWARM_RETARGET",
    "GOOSE_SWARM_RETARGET_DRAFT_STEP",
    "GOOSE_SWARM_RETARGET_ROUNDS",
    "GOOSE_SWARM_RETARGET_STALL_GUARD",
    "GOOSE_SWARM_RETARGET_STALL_TOLERANCE",
    "GOOSE_SWARM_REVIEW",
    "GOOSE_SWARM_RUN_DEADLINE_UNIX_MS",
    "GOOSE_SWARM_SALVAGE_REQUIRE_CRITICAL",
    "GOOSE_SWARM_SALVAGE_SPIN",
    "GOOSE_SWARM_SCOPED_CONTRACTS",
    "GOOSE_SWARM_SHIP_BEST",
    "GOOSE_SWARM_SINK_CAP_REF_BYTES",
    "GOOSE_SWARM_SINK_CAP_SECS",
    "GOOSE_SWARM_SINK_LEAN_PREFILL",
    "GOOSE_SWARM_SINK_MAX_TURNS",
    "GOOSE_SWARM_SINK_PREBUILD",
    "GOOSE_SWARM_SINK_REVIEW",
    "GOOSE_SWARM_SINK_SHARD",
    "GOOSE_SWARM_SKELETON_FIRST",
    "GOOSE_SWARM_SMOKE",
    "GOOSE_SWARM_SPECULATE",
    "GOOSE_SWARM_SPEC_CONTRACT",
    "GOOSE_SWARM_SPEC_REPAIR",
    "GOOSE_SWARM_SPEC_SIZED_PLAN",
    "GOOSE_SWARM_SPIRAL_BREAK_CHARS",
    "GOOSE_SWARM_SPIRAL_THINKING_CHARS",
    "GOOSE_SWARM_SPLIT",
    "GOOSE_SWARM_SPLIT_FAT",
    "GOOSE_SWARM_SPLIT_FAT_FILES",
    "GOOSE_SWARM_SPLIT_INHERIT_SPEC",
    "GOOSE_SWARM_SPLIT_SECS",
    "GOOSE_SWARM_STRAGGLER_GRACE_SECS",
    "GOOSE_SWARM_STRAGGLER_STOP",
    "GOOSE_SWARM_STRAGGLER_STOP_DEGRADE",
    "GOOSE_SWARM_STREAM_RETRY",
    "GOOSE_SWARM_STRUCT_STOP",
    "GOOSE_SWARM_SUPERVISION_POOL",
    "GOOSE_SWARM_TAIL_REVIEW",
    "GOOSE_SWARM_TAIL_REVIEW_SECS",
    "GOOSE_SWARM_TELEMETRY_FILE",
    "GOOSE_SWARM_TEMP",
    "GOOSE_SWARM_TESTGEN",
    "GOOSE_SWARM_THINK_OFF",
    "GOOSE_SWARM_TOP_K",
    "GOOSE_SWARM_TOP_P",
    "GOOSE_SWARM_TS_SMOKE_TESTS",
    "GOOSE_SWARM_UNCAPPED",
    "GOOSE_SWARM_UNWIRED_DEMOTES_VERIFIED",
    "GOOSE_SWARM_USER_NOTES",
    "GOOSE_SWARM_VERIFY_COMMANDS",
    "GOOSE_SWARM_WEB_VOCAB",
    "GOOSE_SWARM_WRITE_FIRST",
];

fn canonical_suffix(environment: &str) -> String {
    environment
        .strip_prefix("GOOSE_SWARM_")
        .unwrap_or(environment)
        .to_ascii_lowercase()
}

pub(crate) fn canonical_control_name(environment: &str) -> Option<&'static str> {
    if let Some(spec) = ENVIRONMENT_ONLY_CONTROLS
        .iter()
        .find(|spec| spec.environment == environment)
    {
        return Some(spec.canonical);
    }
    let suffix = canonical_suffix(environment);
    let canonical = CONTROL_ALIASES
        .iter()
        .find(|alias| alias.alias == suffix)
        .map(|alias| alias.canonical)
        .unwrap_or(suffix.as_str());
    CONFIG_CONTROLS
        .iter()
        .find(|spec| spec.canonical == canonical)
        .map(|spec| spec.canonical)
}

fn value_type(value: &Value) -> Option<&'static str> {
    match value {
        Value::Null => None,
        Value::Bool(_) => Some("boolean"),
        Value::Number(number) if number.is_i64() || number.is_u64() => Some("integer"),
        Value::Number(_) => Some("number"),
        Value::String(_) => Some("string"),
        Value::Array(_) => Some("array"),
        Value::Object(_) => Some("object"),
    }
}

fn config_control_value_type(canonical: &str, defaults: &Map<String, Value>) -> &'static str {
    if let Some(kind) = defaults.get(canonical).and_then(value_type) {
        return kind;
    }

    // Serde is the authority for optional fields whose serialized default is null. Probe the real
    // `SwarmConfig` decoder instead of maintaining a second hand-written type catalogue beside the field
    // registry. Fractional numbers precede integers because a float accepts 0 while an integer rejects 0.5.
    for (kind, candidate) in [
        ("boolean", Value::Bool(false)),
        ("number", serde_json::json!(0.5)),
        ("integer", Value::from(0)),
        ("string", Value::String(String::new())),
        ("array", Value::Array(Vec::new())),
        ("object", Value::Object(Map::new())),
    ] {
        let mut object = defaults.clone();
        object.insert(canonical.to_string(), candidate);
        if serde_json::from_value::<SwarmConfig>(Value::Object(object)).is_ok() {
            return kind;
        }
    }
    "unknown"
}

pub(crate) fn control_registry_manifest() -> Value {
    let defaults = serialized_config_controls(&SwarmConfig::default());
    serde_json::json!({
        "schema_version": 2,
        "config": CONFIG_CONTROLS.iter().map(|spec| serde_json::json!({
            "canonical": spec.canonical,
            "disposition": spec.disposition.as_str(),
            "campaign_role": spec.campaign_role.as_str(),
            "source": "config",
            "value_type": config_control_value_type(spec.canonical, &defaults),
            "default": defaults.get(spec.canonical).cloned().unwrap_or(Value::Null),
            "effective_echo": true,
        })).collect::<Vec<_>>(),
        "environment_only": ENVIRONMENT_ONLY_CONTROLS.iter().map(|spec| serde_json::json!({
            "canonical": spec.canonical,
            "environment": spec.environment,
            "disposition": spec.disposition.as_str(),
            "campaign_role": campaign_role(spec.disposition).as_str(),
            "source": "environment",
            "effective_echo": EFFECTIVE_ENVIRONMENT_ONLY_ECHOES.contains(&spec.canonical),
        })).collect::<Vec<_>>(),
        "aliases": CONTROL_ALIASES.iter().map(|alias| serde_json::json!({
            "alias": alias.alias,
            "canonical": alias.canonical,
        })).collect::<Vec<_>>(),
        "environment_readers": SWARM_ENV_READERS.iter().map(|environment| serde_json::json!({
            "environment": environment,
            "canonical": canonical_control_name(environment)
                .expect("the compile-time registry test requires every reader to resolve"),
        })).collect::<Vec<_>>(),
    })
}

fn lowercase_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonical_json(&object[key]));
            }
            Value::Object(canonical)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn json_sha256(value: &Value) -> String {
    let encoded = serde_json::to_vec(&canonical_json(value))
        .expect("the control evidence always serializes as JSON");
    let digest = Sha256::digest(encoded);
    lowercase_hex(&digest)
}

fn control_environment_sha256() -> String {
    let inputs = SWARM_ENV_READERS
        .iter()
        .map(|environment| {
            (
                (*environment).to_string(),
                std::env::var(environment)
                    .ok()
                    .map_or(Value::Null, Value::String),
            )
        })
        .collect::<Map<_, _>>();
    json_sha256(&Value::Object(inputs))
}

pub(crate) fn control_registry_export() -> Value {
    let control_registry = control_registry_manifest();
    let registry_sha256 = json_sha256(&control_registry);
    serde_json::json!({
        "schema_version": 1,
        "engine": {
            "version": option_env!("GOOSE_BUILD_VERSION").unwrap_or("dev"),
            "build_sha": option_env!("GOOSE_BUILD_SHA").unwrap_or("dev"),
            "crate_version": env!("CARGO_PKG_VERSION"),
        },
        "registry_sha256": registry_sha256,
        "control_environment_sha256": control_environment_sha256(),
        "control_registry": control_registry,
    })
}

pub(crate) fn serialized_config_controls(config: &SwarmConfig) -> Map<String, Value> {
    let Value::Object(mut object) = serde_json::to_value(config)
        .expect("SwarmConfig serialization is infallible for its supported field types")
    else {
        unreachable!("SwarmConfig serializes as an object")
    };
    object.retain(|name, _| CONFIG_CONTROLS.iter().any(|spec| spec.canonical == name));
    for spec in CONFIG_CONTROLS {
        object
            .entry(spec.canonical.to_string())
            .or_insert(Value::Null);
    }
    object
}

pub(crate) fn merge_effective_config_controls(
    config: &SwarmConfig,
    effective: &mut Map<String, Value>,
) {
    let mut complete = serialized_config_controls(config);
    complete.append(effective);
    *effective = complete;
}

/// Apply the `uncapped` runtime profile to the values emitted as engine truth. These are the config fields
/// whose execution sites are transformed by that profile; emitting their persisted inputs would claim a
/// ceiling that the run did not execute.
pub(crate) fn apply_uncapped_effective_values(
    values: &mut Map<String, Value>,
    uncapped: bool,
    unbounded_secs: u64,
    worker_max_turns: u32,
) {
    values.insert("uncapped".into(), Value::Bool(uncapped));
    if !uncapped {
        return;
    }
    for name in [
        "planner_timeout_secs",
        "scout_budget_secs",
        "complete_cap_secs",
        "draft_timeout_secs",
        "clarity_probe_secs",
    ] {
        values.insert(name.into(), Value::from(unbounded_secs));
    }
    for name in [
        "sink_cap_secs",
        "progress_watchdog_secs",
        "spiral_break_chars",
        "spiral_thinking_chars",
    ] {
        values.insert(name.into(), Value::from(0));
    }
    values.insert("straggler_stop_degrade".into(), Value::Bool(false));
    values.insert("split".into(), Value::Bool(false));
    values.insert("worker_max_turns".into(), Value::from(worker_max_turns));
    values.insert(
        "sink_max_turns".into(),
        Value::from(worker_max_turns.max(100_000)),
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ValueOrigin {
    Environment,
    Config,
    Profile,
    Default,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedValue<T> {
    pub value: T,
    pub origin: ValueOrigin,
}

/// One precedence primitive for every ordinary control: environment > persisted config > selected runtime
/// profile > engine default. Special adaptive controls may transform the selected value afterwards, but may
/// not invent a different ordering.
pub(crate) fn resolve_control_precedence<T>(
    environment: Option<T>,
    config: Option<T>,
    profile: Option<T>,
    default: T,
) -> ResolvedValue<T> {
    if let Some(value) = environment {
        return ResolvedValue {
            value,
            origin: ValueOrigin::Environment,
        };
    }
    if let Some(value) = config {
        return ResolvedValue {
            value,
            origin: ValueOrigin::Config,
        };
    }
    if let Some(value) = profile {
        return ResolvedValue {
            value,
            origin: ValueOrigin::Profile,
        };
    }
    ResolvedValue {
        value: default,
        origin: ValueOrigin::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn struct_fields(source: &str) -> Vec<String> {
        let body = source
            .split_once("pub struct SwarmConfig {")
            .expect("SwarmConfig exists")
            .1
            .split_once("\n}")
            .expect("SwarmConfig closes")
            .0;
        body.lines()
            .filter_map(|line| {
                let line = line.trim();
                line.strip_prefix("pub ")?
                    .split_once(':')
                    .map(|(name, _)| name.trim().to_string())
            })
            .collect()
    }

    fn literal_readers(source: &str) -> BTreeSet<String> {
        const READERS: &[&str] = &[
            "std::env::var(",
            "std::env::var_os(",
            "env::var(",
            "env::var_os(",
            "swarm_gate(",
            "swarm_gate_cfg(",
            "swarm_gate_cfg_bundle(",
            "env_f32_clamped(",
            "default_on_environment_gate(",
        ];
        let mut controls = BTreeSet::new();
        for reader in READERS {
            let mut rest = source;
            while let Some((_, after_reader)) = rest.split_once(reader) {
                let after_reader = after_reader.trim_start();
                if let Some(after_quote) = after_reader.strip_prefix('"') {
                    if let Some((environment, _)) = after_quote.split_once('"') {
                        if environment.starts_with("GOOSE_SWARM_") {
                            controls.insert(environment.to_string());
                        }
                    }
                }
                rest = after_reader;
            }
        }
        controls
    }

    fn explicit_levers_event_keys(source: &str) -> BTreeSet<String> {
        let event = source
            .split_once("let mut levers_event = serde_json::json!")
            .expect("levers_resolved event exists")
            .1
            .split_once("sink.write_value(levers_event)")
            .expect("levers_resolved event is emitted")
            .0;
        let mut keys = BTreeSet::new();
        let mut rest = event;
        while let Some((_, after_quote)) = rest.split_once('"') {
            if let Some((candidate, after)) = after_quote.split_once('"') {
                if after.trim_start().starts_with(':')
                    && candidate
                        .chars()
                        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
                {
                    keys.insert(candidate.to_string());
                }
                rest = after;
            } else {
                break;
            }
        }
        keys
    }

    #[test]
    fn every_swarm_config_field_has_exactly_one_disposition() {
        let source = include_str!("swarm.rs");
        let fields = struct_fields(source);
        assert_eq!(fields.len(), 116, "field parser or SwarmConfig changed");
        let mut counts = BTreeMap::new();
        for spec in CONFIG_CONTROLS {
            *counts.entry(spec.canonical).or_insert(0usize) += 1;
        }
        for field in &fields {
            assert_eq!(
                counts.get(field.as_str()),
                Some(&1),
                "{field} registry coverage"
            );
        }
        assert_eq!(
            counts.len(),
            fields.len(),
            "registry contains a non-field name"
        );
    }

    #[test]
    fn audited_disposition_totals_cannot_drift_silently() {
        let count = |wanted| {
            CONFIG_CONTROLS
                .iter()
                .filter(|spec| spec.disposition == wanted)
                .count()
        };
        assert_eq!(count(ControlDisposition::RetainEnabled), 27);
        assert_eq!(count(ControlDisposition::RetainDisabled), 8);
        assert_eq!(count(ControlDisposition::Modify), 31);
        assert_eq!(count(ControlDisposition::RemoveMerge), 16);
        assert_eq!(count(ControlDisposition::RuntimeProfile), 34);

        let env_count = |wanted| {
            ENVIRONMENT_ONLY_CONTROLS
                .iter()
                .filter(|spec| spec.disposition == wanted)
                .count()
        };
        assert_eq!(env_count(ControlDisposition::RetainEnabled), 14);
        assert_eq!(env_count(ControlDisposition::RetainDisabled), 3);
        assert_eq!(env_count(ControlDisposition::Modify), 9);
        assert_eq!(env_count(ControlDisposition::RemoveMerge), 8);
        assert_eq!(env_count(ControlDisposition::RuntimeProfile), 15);
    }

    #[test]
    fn every_literal_swarm_environment_reader_is_registered_and_no_catalog_name_is_inert() {
        let swarm_source = include_str!("swarm.rs");
        let (before_echo, echo_and_after) = swarm_source
            .split_once("let mut levers_event = serde_json::json!")
            .expect("levers_resolved event exists");
        let (_, after_echo) = echo_and_after
            .split_once("sink.write_value(levers_event)")
            .expect("levers_resolved event is emitted");
        let swarm_behavior = format!("{before_echo}{after_echo}");
        let mut actual = BTreeSet::new();
        for source in [
            swarm_behavior.as_str(),
            include_str!("../../../goose-swarm/src/scheduler.rs"),
            include_str!("../../../goose-swarm/src/dag.rs"),
            include_str!("../../../goose/src/providers/swarm.rs"),
        ] {
            actual.extend(literal_readers(source));
        }
        let registered: BTreeSet<_> = SWARM_ENV_READERS.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            actual, registered,
            "reader registry must be bidirectionally exact"
        );
        assert_eq!(
            registered.len(),
            SWARM_ENV_READERS.len(),
            "duplicate reader metadata"
        );
        for environment in SWARM_ENV_READERS {
            assert!(
                canonical_control_name(environment).is_some(),
                "{environment} has no canonical config or environment-only control"
            );
        }
    }

    #[test]
    fn every_environment_overridable_config_control_has_an_explicit_effective_echo() {
        let event_keys = explicit_levers_event_keys(include_str!("swarm.rs"));
        for environment in SWARM_ENV_READERS {
            let canonical = canonical_control_name(environment).unwrap();
            if CONFIG_CONTROLS
                .iter()
                .any(|spec| spec.canonical == canonical)
            {
                assert!(
                    event_keys.contains(canonical),
                    "{environment} overrides {canonical}, but levers_resolved would retain its raw config value"
                );
            }
        }
    }

    #[test]
    fn default_on_environment_paths_are_effectively_echoed() {
        let source = include_str!("swarm.rs");
        let event_keys = explicit_levers_event_keys(source);
        for canonical in [
            "judge",
            "pillar_flow",
            "prereview",
            "qa",
            "tail_review",
            "spec_repair",
            "salvage_spin",
            "ship_best",
            "sink_shard",
        ] {
            assert!(
                event_keys.contains(canonical),
                "default-on {canonical} path is invisible in levers_resolved"
            );
        }
        assert!(source.contains("!pillar_flow_on && idle_judge_enabled()"));
        assert!(source.contains("!pillar_flow_on && prereview_enabled()"));
        assert!(source.contains("let ship_best = ship_best_enabled();"));
    }

    #[test]
    fn environment_only_effective_echo_metadata_is_bidirectionally_exact() {
        let event_keys = explicit_levers_event_keys(include_str!("swarm.rs"));
        let actual: BTreeSet<_> = ENVIRONMENT_ONLY_CONTROLS
            .iter()
            .filter(|spec| event_keys.contains(spec.canonical))
            .map(|spec| spec.canonical)
            .collect();
        let registered: BTreeSet<_> = EFFECTIVE_ENVIRONMENT_ONLY_ECHOES.iter().copied().collect();
        assert_eq!(actual, registered);
        assert_eq!(registered.len(), EFFECTIVE_ENVIRONMENT_ONLY_ECHOES.len());
    }

    #[test]
    fn aliases_are_unique_and_resolve_to_real_canonical_controls() {
        let mut aliases = BTreeSet::new();
        let canonical: BTreeSet<_> = CONFIG_CONTROLS
            .iter()
            .map(|spec| spec.canonical)
            .chain(ENVIRONMENT_ONLY_CONTROLS.iter().map(|spec| spec.canonical))
            .collect();
        for alias in CONTROL_ALIASES {
            assert!(
                aliases.insert(alias.alias),
                "duplicate alias {}",
                alias.alias
            );
            assert!(
                CONFIG_CONTROLS
                    .iter()
                    .any(|spec| spec.canonical == alias.canonical),
                "alias {} points at missing {}",
                alias.alias,
                alias.canonical
            );
            assert!(
                !canonical.contains(alias.alias) || alias.alias == alias.canonical,
                "alias {} shadows a different canonical control",
                alias.alias
            );
        }
    }

    #[test]
    fn obsolete_comment_only_control_names_do_not_reenter_production_source() {
        let source = include_str!("swarm.rs");
        for obsolete in ["GOOSE_SWARM_REVIEW_FANOUT", "GOOSE_SWARM_REVIEW_REPRO"] {
            assert!(!source.contains(obsolete), "obsolete control {obsolete}");
        }
    }

    #[test]
    fn manifest_is_the_machine_export_of_the_rust_registry() {
        let manifest = control_registry_manifest();
        assert_eq!(manifest["schema_version"], 2);
        assert_eq!(
            manifest["config"].as_array().unwrap().len(),
            CONFIG_CONTROLS.len()
        );
        assert!(manifest["config"].as_array().unwrap().iter().all(|row| row
            .get("default")
            .is_some()
            && row["source"] == "config"
            && row.get("value_type").is_some()
            && row.get("campaign_role").is_some()));
        assert_eq!(
            manifest["environment_only"].as_array().unwrap().len(),
            ENVIRONMENT_ONLY_CONTROLS.len()
        );
        assert_eq!(
            manifest["aliases"].as_array().unwrap().len(),
            CONTROL_ALIASES.len()
        );
        assert_eq!(
            manifest["environment_readers"].as_array().unwrap().len(),
            SWARM_ENV_READERS.len()
        );
    }

    #[test]
    fn campaign_roles_separate_behavior_profile_removal_and_telemetry() {
        let manifest = control_registry_manifest();
        for section in ["config", "environment_only"] {
            assert!(manifest[section].as_array().unwrap().iter().all(|row| {
                matches!(
                    row["campaign_role"].as_str(),
                    Some("behavior" | "runtime_profile" | "removal" | "telemetry")
                )
            }));
        }
        let occupancy = manifest["config"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["canonical"] == "occupancy")
            .unwrap();
        assert_eq!(occupancy["campaign_role"], "telemetry");
    }

    #[test]
    fn manifest_types_come_from_the_real_swarm_config_decoder() {
        let manifest = control_registry_manifest();
        let rows = manifest["config"].as_array().unwrap();
        let kind = |canonical: &str| {
            rows.iter()
                .find(|row| row["canonical"] == canonical)
                .unwrap()["value_type"]
                .as_str()
                .unwrap()
        };
        assert_eq!(kind("split"), "boolean");
        assert_eq!(kind("ask_max_q"), "integer");
        assert_eq!(kind("temperature"), "number");
        assert_eq!(kind("lm_extra_body"), "object");
        assert_eq!(kind("devices"), "array");
        assert_eq!(kind("research_planning"), "string");
        assert!(rows.iter().all(|row| row["value_type"] != "unknown"));
    }

    #[test]
    fn export_binds_build_identity_to_the_exact_registry() {
        let export = control_registry_export();
        assert_eq!(export["schema_version"], 1);
        assert!(export["engine"]["version"].is_string());
        assert!(export["engine"]["build_sha"].is_string());
        assert!(export["engine"]["crate_version"].is_string());
        assert_eq!(
            export["registry_sha256"],
            json_sha256(&export["control_registry"])
        );
        assert_eq!(
            export["control_environment_sha256"],
            control_environment_sha256()
        );
    }

    #[test]
    fn run_environment_seal_precedes_internal_environment_bridges() {
        let run = include_str!("swarm.rs")
            .split_once("pub async fn run_swarm")
            .unwrap()
            .1;
        let seal = run
            .find("let control_export = control_registry_export();")
            .unwrap();
        for bridge in [
            "std::env::set_var(\"GOOSE_SWARM_SINK_CAP_SECS\"",
            "std::env::set_var(\"GOOSE_SWARM_TELEMETRY_FILE\"",
        ] {
            assert!(
                seal < run.find(bridge).unwrap(),
                "operator-input seal must precede internal bridge {bridge}"
            );
        }
    }

    #[test]
    fn precedence_is_environment_then_config_then_profile_then_default() {
        assert_eq!(
            resolve_control_precedence(Some(1), Some(2), Some(3), 4),
            ResolvedValue {
                value: 1,
                origin: ValueOrigin::Environment
            }
        );
        assert_eq!(
            resolve_control_precedence(None, Some(2), Some(3), 4),
            ResolvedValue {
                value: 2,
                origin: ValueOrigin::Config
            }
        );
        assert_eq!(
            resolve_control_precedence(None, None, Some(3), 4),
            ResolvedValue {
                value: 3,
                origin: ValueOrigin::Profile
            }
        );
        assert_eq!(
            resolve_control_precedence(None, None, None, 4),
            ResolvedValue {
                value: 4,
                origin: ValueOrigin::Default
            }
        );
    }

    #[test]
    fn serialized_echo_starts_with_every_config_field_once() {
        let values = serialized_config_controls(&SwarmConfig::default());
        assert_eq!(values.len(), CONFIG_CONTROLS.len());
        for spec in CONFIG_CONTROLS {
            assert!(
                values.contains_key(spec.canonical),
                "missing {}",
                spec.canonical
            );
        }
        assert!(!values.contains_key("dynamic_replan_cfg"));
    }

    #[test]
    fn effective_values_replace_serialized_inputs_without_losing_schema_coverage() {
        let mut effective = Map::new();
        effective.insert("worker_max_turns".into(), Value::from(100_000));
        merge_effective_config_controls(&SwarmConfig::default(), &mut effective);
        assert_eq!(effective.len(), CONFIG_CONTROLS.len());
        assert_eq!(effective["worker_max_turns"], 100_000);
    }

    #[test]
    fn uncapped_echo_reports_execution_values_not_persisted_caps() {
        let mut values = serialized_config_controls(&SwarmConfig::default());
        apply_uncapped_effective_values(&mut values, true, 604_800, 100_000);
        assert_eq!(values["uncapped"], true);
        assert_eq!(values["sink_cap_secs"], 0);
        assert_eq!(values["progress_watchdog_secs"], 0);
        assert_eq!(values["spiral_break_chars"], 0);
        assert_eq!(values["spiral_thinking_chars"], 0);
        assert_eq!(values["straggler_stop_degrade"], false);
        assert_eq!(values["split"], false);
        assert_eq!(values["planner_timeout_secs"], 604_800);
        assert_eq!(values["scout_budget_secs"], 604_800);
        assert_eq!(values["complete_cap_secs"], 604_800);
        assert_eq!(values["draft_timeout_secs"], 604_800);
        assert_eq!(values["clarity_probe_secs"], 604_800);
        assert_eq!(values["worker_max_turns"], 100_000);
        assert_eq!(values["sink_max_turns"], 100_000);
    }

    #[test]
    fn split_inherit_echo_calls_the_scheduler_resolution() {
        let source = include_str!("swarm.rs");
        assert!(source.contains("\"split_inherit_spec\": split_inherit_spec_enabled()"));
    }

    #[test]
    fn runtime_profile_echoes_use_resolved_execution_inputs() {
        let source = include_str!("swarm.rs");
        for expression in [
            "\"context_cap\": local_context_cap()",
            "\"max_tool_response_chars\": large_text_threshold()",
            "\"parallel_planning\": use_parallel",
            "\"worker_extensions\": ext_names",
            "\"worker_max_turns\": worker_max_turns",
            "\"planner_timeout_secs\": planner_wall(cfg.planner_timeout_secs)",
            "\"best_of_n_skeletons\": best_of_n",
        ] {
            assert!(
                source.contains(expression),
                "raw runtime echo: {expression}"
            );
        }
    }
}
