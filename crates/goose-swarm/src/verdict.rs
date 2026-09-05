//! The judge VOCABULARY the omni judge speaks (`goose-cli`'s `supervision::parse_judge_reply`,
//! `ladder::nudge_delivery`): `Verdict` and `JudgeOutcome`. Moved from `judge.rs` when the
//! scheduler-side idle-model judge was deleted (2c S6, 2026-09-01) — these two types outlived it
//! because the omni judge never depended on the rest.

use serde::Serialize;

/// What the omni judge concluded about an in-flight call (a NUDGE vocabulary — invariant 4: the
/// judge nudges, it never kills; the scheduler-side idle-model judge that also spoke it is deleted,
/// 2c S6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Progressing fine — no action.
    Ok,
    /// Reading/exploring far more than writing (over-read, structure-hunt, greenfield-explore).
    OverReading,
    /// A degenerate loop — repeated identical thinking, turns burned with no new output.
    Looping,
    /// A produced file does not compile (syntax error, bad characters, broken import).
    BrokenCode,
    /// The work is drifting from the subtask spec.
    SpecDrift,
    /// The worker has written none of its owned files AND has taken no action at all — it is stuck
    /// before its first byte, not over-reading. Distinct from `OverReading` because the remedies differ
    /// and because a log that calls a zero-tool-call worker "over_reading" misdirects every later reader.
    NoFirstWrite,
    /// The call is WORKING, but on the wrong thing — it has drifted off the goal. Redirect, never stop.
    Drifting,
    /// The call has produced nothing usable and a fresh session seeded with what it HAS established would
    /// beat continuing. The task is re-run on the SAME device with a new session — never handed to another
    /// node, because every node runs the same model, so moving work costs the session and buys nothing.
    ///
    /// Permitted only while the previous attempt produced SOMETHING (a tool call, a file byte, or new
    /// reasoning). Two consecutive attempts that produce nothing at all end the task instead, with the
    /// judge's notes attached — that is the liveness rule that stops a judge restarting forever.
    Restart,
    /// The deliverable is DONE: every owned file exists and none fails its syntax/compile check, but the
    /// worker is still running. Finish it rather than spending an attempt on a kill.
    ///
    /// MEASURED (F165): `test-meridian` was recorded a TERMINAL FAILURE with its file on disk carrying 8
    /// test functions and 12 assertions, all passing — 8 of the 72 tests the crunched app passes. The
    /// engine's own hint said so before killing it: "Nothing is reported failing, so the file is most
    /// likely already done and you are polishing." Every other verdict is a way to STOP a worker; without
    /// this one the judge's only lever is kill, and the third kill is terminal.
    Accept,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Ok => "ok",
            Verdict::OverReading => "over_reading",
            Verdict::Looping => "looping",
            Verdict::BrokenCode => "broken_code",
            Verdict::SpecDrift => "spec_drift",
            Verdict::Drifting => "drifting",
            Verdict::Restart => "restart",
            Verdict::Accept => "accept",
            Verdict::NoFirstWrite => "no_first_write",
        }
    }

    /// Whether this verdict means the worker is in trouble. `Accept` is a COMPLETION, not trouble — it
    /// must never reach the intervention path, or the verdict that exists to stop a task being failed
    /// would itself count toward failing it.
    pub fn is_problem(&self) -> bool {
        !matches!(self, Verdict::Ok | Verdict::Accept)
    }
}

/// The omni judge's conclusion about one call, as `supervision::parse_judge_reply` builds it.
#[derive(Clone, Debug)]
pub struct JudgeOutcome {
    pub verdict: Verdict,
    /// 0.0–1.0. Intervention requires a high bar because the judge is itself a weak model.
    pub confidence: f32,
    /// A one-line corrective hint, prepended to the task on re-dispatch.
    pub hint: String,
    /// What this call has ALREADY WORKED OUT that is worth keeping, in the judge's words, drawn from what
    /// the call actually said. This is the point of the whole mechanism: a nudge that throws away the
    /// useful half of a spiralling call is just a slower kill. Empty when nothing was established.
    pub established: String,
    /// The single most concrete next action toward the goal — a file, a command, a function. Never
    /// "continue" or "proceed", which is what the old one-line hint degenerated into.
    pub next_action: String,
    /// PROVENANCE. True only for a verdict an ENGINE FACT produced (the deleted idle-model judge's
    /// `deterministic_verdict` was the writer; the omni judge's parse always writes false) — a real engine fact
    /// (a compile error, an owned file never written, a measured char/tool count). False for anything the
    /// JUDGE MODEL authored.
    ///
    /// This exists because `confidence` cannot carry that distinction: the model produces its own confidence,
    /// so gating an irreversible action on `confidence >= threshold` lets a model opinion decide it. MEASURED:
    /// nf-ts-cadence's integrate-verify went `over_reading -> re_dispatch, re_dispatch, FAILED` at confidence
    /// 0.90 from the LLM path, and because integrate-verify depends on every verify::<M> under fan-verify, one
    /// model opinion took the whole run's verdict red. The standing rule is that only a DETERMINISTIC engine
    /// event may create or kill a verdict; `terminal` now requires this flag.
    pub deterministic: bool,
}

impl JudgeOutcome {
    pub fn ok() -> Self {
        Self {
            verdict: Verdict::Ok,
            confidence: 1.0,
            hint: String::new(),
            established: String::new(),
            next_action: String::new(),
            deterministic: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standing rule: only a DETERMINISTIC engine event may create or kill a verdict. The
    /// model-authored constructor must never claim provenance it does not have.
    #[test]
    fn model_authored_outcomes_are_never_marked_deterministic() {
        assert!(!JudgeOutcome::ok().deterministic);
        assert_eq!(JudgeOutcome::ok().verdict, Verdict::Ok);
        assert!(!Verdict::Ok.is_problem() && !Verdict::Accept.is_problem());
        assert!(Verdict::Drifting.is_problem() && Verdict::Restart.is_problem());
        assert_eq!(Verdict::NoFirstWrite.as_str(), "no_first_write");
    }
}
