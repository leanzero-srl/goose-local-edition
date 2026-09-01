//! The `levers_resolved` echo's RETIRED block: every lever here names a mechanism that is
//! `#[cfg(test)]` or unreachable in this build, so there is no value to RESOLVE — and a resolved
//! value WAS being printed: r6c's echo said `split_fat: true` while `split_fat_modules` had been
//! test-only since b0dd68eac (gate 1, suspect #2 — a levers echo that lies). The config fields
//! survive for the config round-trip with false/None defaults since r6e, the desktop pins neither
//! SPLIT_FAT nor FIX_SCHED since cceab86eb, golden.generated.json is regenerated from the struct.
//! Audited 2026-09-01 by asking, of each, which non-test caller consumes it.
//!
//! r6e E11 (the README overclaim the refuter caught): the block carried the REASON only, so a
//! stale pin — `split_fat: true` in an operator's config.yaml (the loop-state backups
//! `config.yaml.bak-pre-135:235` and `.pre-mtp-swap:216` carry exactly that), or
//! `GOOSE_SWARM_SPLIT_FAT=1` in an env — was silently ignored AND invisible, which is not "visible
//! rather than certified". Each row now carries `configured`: the value the lever WOULD have
//! resolved to, read exactly as the engine read it while it lived (env wins, parsed as the engine
//! parsed it; else the config field; `null` when neither names it — an honest absence, never a
//! default impersonating a choice). The reason says it is dead; `configured` says whether
//! someone still thinks it is alive. Sibling module under the incremental-split law.

use super::SwarmConfig;
use serde_json::{json, Value};

pub(super) fn retired_levers(cfg: &SwarmConfig, env: &dyn Fn(&str) -> Option<String>) -> Value {
    let on = |v: &str| {
        matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        )
    };
    // env > config, for a bool field (the `swarm_gate_cfg` reading).
    let gate = |name: &str, cfg_default: bool| -> Value {
        match env(name) {
            Some(v) => json!(on(&v)),
            None => json!(cfg_default),
        }
    };
    // env > config, for an Option<bool> field: null when neither names it.
    let opt_gate = |name: &str, cfg_val: Option<bool>| -> Value {
        match env(name) {
            Some(v) => json!(on(&v)),
            None => json!(cfg_val),
        }
    };
    // env > config for the grace seconds; an env value that is not a number is echoed RAW, so a
    // typo reads as the typo, not as the config value.
    let opt_secs = |name: &str, cfg_val: Option<u64>| -> Value {
        match env(name) {
            Some(v) => match v.trim().parse::<u64>() {
                Ok(n) => json!(n),
                Err(_) => json!(v),
            },
            None => json!(cfg_val),
        }
    };
    let row = |reason: &str, configured: Value| json!({"reason": reason, "configured": configured});
    json!({
        "split_fat": row(
            "split_fat_modules is #[cfg(test)] since b0dd68eac",
            gate("GOOSE_SWARM_SPLIT_FAT", cfg.split_fat),
        ),
        // Config-only while it lived (no env override ever read it): null when unset — the
        // fan cut found it printed as a live bound (`levers_resolved.max_research_questions:
        // 4`) on r6d while `research_fan` dispatched all 38 questions.
        "max_research_questions": row(
            "bounded v1's scout lenses (deleted with P1-5); consumed nowhere since — the v2 fan \
             dispatches every opener question, uncapped",
            json!(cfg.max_research_questions),
        ),
        // Config-only while they lived (the CLI flag `--dynamic-replan` died with the mechanism).
        "dynamic_replan": row(
            "the dynamic replanner is deleted (VA-015): r6c's replan-r0 ran 208 unsupervised minutes \
             for two bonus tasks nothing imported; r5's held two READY tasks 19 minutes",
            json!(cfg.dynamic_replan),
        ),
        "max_replans": row("same mechanism", json!(cfg.max_replans)),
        "persona": row(
            "LEARN/persona is deleted (VA-016): lessons were structurally 0 on both runs that wrote a \
             skill (persona.rs harvested `judge_verdict`, an event no run.jsonl carries) and the stack \
             key read `angular` for a Python + vanilla-JS app",
            gate("GOOSE_SWARM_PERSONA", cfg.persona),
        ),
        "fan_verify": row(
            "fan_verify_split is #[cfg(test)] since P1-4",
            gate("GOOSE_SWARM_FAN_VERIFY", cfg.fan_verify),
        ),
        "fan_e2e": row(
            "no consumer; it sharded the fan fan_verify no longer builds",
            gate("GOOSE_SWARM_FAN_E2E", cfg.fan_e2e),
        ),
        "straggler_stop": row(
            "collect_drafts_with_straggler_stop is #[cfg(test)] since P1-4",
            opt_gate("GOOSE_SWARM_STRAGGLER_STOP", cfg.straggler_stop),
        ),
        "straggler_stop_degrade": row(
            "same collector",
            opt_gate("GOOSE_SWARM_STRAGGLER_STOP_DEGRADE", cfg.straggler_stop_degrade),
        ),
        "straggler_grace_secs": row(
            "same collector",
            opt_secs("GOOSE_SWARM_STRAGGLER_GRACE_SECS", cfg.straggler_grace_secs),
        ),
        "split": row(
            "scheduler.rs pins `let is_split = false`; GOOSE_SWARM_SPLIT was read by nothing but this echo",
            opt_gate("GOOSE_SWARM_SPLIT", cfg.split),
        ),
        // env-only while it lived: no config field exists, so unset is null.
        "split_inherit_spec": row(
            "read only inside apply_split, which is_split = false never reaches",
            match env("GOOSE_SWARM_SPLIT_INHERIT_SPEC") {
                Some(v) => json!(on(&v)),
                None => Value::Null,
            },
        ),
        // 2c S6: the two idle-fill dimension reviews are deleted. sink_review kept a config field
        // (env > config); tail_review was env-only with a default-ON reading, echoed as the engine
        // read it (`0/off/false/no` = false) so main.ts's stale `GOOSE_SWARM_TAIL_REVIEW: '0'` pin
        // is visible as a pin on a dead lever.
        "sink_review": row(
            "the sink idle-fill (review_dimension, pick_sink_review) is deleted in 2c S6; its producer \
             defaulted OFF while its drain said ON, so it never ran once in any measured run",
            opt_gate("GOOSE_SWARM_SINK_REVIEW", cfg.sink_review),
        ),
        "tail_review": row(
            "the tail idle-fill (pick_tail_review, GOOSE_SWARM_TAIL_REVIEW) is deleted in 2c S6; r2 \
             measured 474 of 481 tail_review events in one hour, 477 with had_findings=false",
            match env("GOOSE_SWARM_TAIL_REVIEW") {
                Some(v) => json!(!matches!(
                    v.trim().to_lowercase().as_str(),
                    "0" | "off" | "false" | "no"
                )),
                None => Value::Null,
            },
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stale pin the README promised to make visible: `split_fat: true` in config.yaml (the
    /// loop-state backups' shape) reads `configured: true` beside the reason; an env pin reads
    /// the same; a run that names none of them reads false / null — r6d's shape, which the
    /// reason-only block rendered identically to a stale-pinned run.
    #[test]
    fn a_stale_pin_on_a_retired_lever_is_visible_beside_its_reason() {
        let none = |_: &str| None;
        let mut cfg = SwarmConfig::default();
        let clean = retired_levers(&cfg, &none);
        for k in [
            "split_fat",
            "fan_verify",
            "fan_e2e",
            "straggler_stop",
            "straggler_stop_degrade",
            "straggler_grace_secs",
            "split",
            "split_inherit_spec",
            "max_research_questions",
            "dynamic_replan",
            "max_replans",
            "persona",
            "sink_review",
            "tail_review",
        ] {
            assert!(
                clean[k]["reason"].as_str().is_some_and(|r| !r.is_empty()),
                "{k}"
            );
        }
        assert_eq!(
            clean["max_research_questions"]["configured"],
            Value::Null,
            "unset is an honest absence, never the old default 4 impersonating a choice"
        );
        assert_eq!(clean["split_fat"]["configured"], json!(false));
        assert_eq!(clean["fan_verify"]["configured"], json!(false));
        assert_eq!(clean["straggler_stop"]["configured"], Value::Null);
        assert_eq!(clean["straggler_grace_secs"]["configured"], Value::Null);
        assert_eq!(clean["split"]["configured"], Value::Null);
        assert_eq!(clean["split_inherit_spec"]["configured"], Value::Null);
        assert_eq!(clean["dynamic_replan"]["configured"], Value::Null);
        assert_eq!(clean["max_replans"]["configured"], Value::Null);
        assert_eq!(clean["persona"]["configured"], json!(false));
        assert_eq!(clean["sink_review"]["configured"], Value::Null);
        assert_eq!(clean["tail_review"]["configured"], Value::Null);

        cfg.split_fat = true;
        cfg.fan_verify = true;
        cfg.straggler_grace_secs = Some(45);
        cfg.max_research_questions = Some(4);
        let stale = retired_levers(&cfg, &none);
        assert_eq!(
            stale["max_research_questions"]["configured"],
            json!(4),
            "a config.yaml that still pins the retired bound is visible beside its reason"
        );
        assert_eq!(stale["split_fat"]["configured"], json!(true));
        assert_eq!(
            stale["split_fat"]["reason"],
            "split_fat_modules is #[cfg(test)] since b0dd68eac"
        );
        assert_eq!(stale["fan_verify"]["configured"], json!(true));
        assert_eq!(stale["straggler_grace_secs"]["configured"], json!(45));

        let env = |k: &str| match k {
            "GOOSE_SWARM_SPLIT_FAT" => Some("0".to_string()),
            "GOOSE_SWARM_SPLIT_INHERIT_SPEC" => Some("yes".to_string()),
            "GOOSE_SWARM_STRAGGLER_GRACE_SECS" => Some("soon".to_string()),
            // main.ts's Benchmark spawn still pins this (dead) lever off.
            "GOOSE_SWARM_TAIL_REVIEW" => Some("0".to_string()),
            "GOOSE_SWARM_SINK_REVIEW" => Some("1".to_string()),
            _ => None,
        };
        let pinned = retired_levers(&cfg, &env);
        assert_eq!(pinned["tail_review"]["configured"], json!(false));
        assert_eq!(pinned["sink_review"]["configured"], json!(true));
        assert_eq!(
            pinned["split_fat"]["configured"],
            json!(false),
            "env wins over config"
        );
        assert_eq!(pinned["split_inherit_spec"]["configured"], json!(true));
        assert_eq!(
            pinned["straggler_grace_secs"]["configured"],
            json!("soon"),
            "a non-numeric env value is echoed raw, never replaced by the config value"
        );
    }
}
