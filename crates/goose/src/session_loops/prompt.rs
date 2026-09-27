//! The tick prompt (design §4.4), assembled from THIS loop's facts. Every line is a fact of the
//! loop or the user's own words, or an instructional constant branched on a measured predicate
//! (self-paced or not, a check ran or not, a yield or not). No line asserts context that may not
//! exist: the last-tick lines appear only when a tick n−1 exists, the check line only when a check
//! ran after it, and so on (gate 2; GEN-4).

use goose_sdk_types::custom_requests::{
    LoopCadence, LoopRecord, LoopTickOutcome, LoopTickRecord, LoopVerdict,
};
use serde::{Deserialize, Serialize};

use super::record::parse_time;
use super::rules::{
    cadence_label, clock_time, goal_first_line, render_steps, LastNextStep, StepFacts,
};

/// How the question tick n−1 asked was resolved (read by the runner from `needs_you.v0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskedResolution {
    Answered,
    Dismissed,
    Open,
}

/// The facts the prompt names beyond the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFacts {
    pub working_dir: String,
    #[serde(default)]
    pub asked_resolution: Option<AskedResolution>,
    pub utc_offset_minutes: i32,
}

fn outcome_words(outcome: Option<&LoopTickOutcome>) -> &'static str {
    match outcome {
        Some(LoopTickOutcome::Progress) => "progress",
        Some(LoopTickOutcome::Done) => "done",
        Some(LoopTickOutcome::Blocked) => "blocked",
        Some(LoopTickOutcome::Asked { .. }) => "asked the user",
        Some(LoopTickOutcome::Failed { .. }) => "failed",
        Some(LoopTickOutcome::NoReport) => "no report",
        Some(LoopTickOutcome::Yielded { .. }) => "yielded",
        Some(LoopTickOutcome::StoppedByYou) => "stopped by the user",
        None => "not ended",
    }
}

/// The step facts of tick `n`, from the record and the tick before it.
pub fn step_facts(
    record: &LoopRecord,
    prev: Option<&LoopTickRecord>,
    working_dir: &str,
) -> StepFacts {
    StepFacts {
        state_file: record.state_file.clone(),
        check: record.check.clone(),
        goal_first_line: goal_first_line(&record.goal),
        last_next_step: match prev {
            None => LastNextStep::First,
            Some(prev) => match &prev.report {
                Some(report) => LastNextStep::Named {
                    text: report.next_step.clone(),
                },
                None => LastNextStep::NamedNone { prev: prev.n },
            },
        },
        working_dir: working_dir.to_string(),
    }
}

/// The prompt of tick `n`, which follows the record's last tick (the tick before it, when there
/// is one).
pub fn tick_prompt(record: &LoopRecord, n: u32, facts: &PromptFacts) -> Result<String, String> {
    let hm = |t: &str| parse_time(t).and_then(|t| clock_time(t, facts.utc_offset_minutes));
    let prev = record.ticks.last();
    let mut lines: Vec<String> = Vec::new();

    let mut head = format!(
        "Loop tick {n} — \"{}\" · {}",
        goal_first_line(&record.goal),
        cadence_label(&record.cadence)
    );
    if let Some(k) = record.stop_after_ticks {
        head.push_str(&format!(" · stop after {k} ticks"));
    }
    lines.push(head);
    lines.push("Your goal (the user's words):".to_string());
    lines.push(record.goal.trim().to_string());
    lines.push(format!(
        "State file: {} — read it before anything else; rewrite it before you call loop_report",
        record.state_file
    ));
    lines.push("(Now · Next · Found · Done; keep it short enough to read in one go).".to_string());

    let steps = render_steps(&record.steps, &step_facts(record, prev, &facts.working_dir));
    if !steps.text.trim().is_empty() {
        lines.push(
            "What each tick does (the user's steps, as they left them in the dialog):".to_string(),
        );
        lines.push(steps.text.trim().to_string());
    }

    if let Some(prev) = prev {
        let p = prev.n;
        let mut last = format!(
            "Last tick ({p}, {}, {})",
            hm(&prev.started_at)?,
            outcome_words(prev.outcome.as_ref())
        );
        match (&prev.report, &prev.outcome) {
            (Some(report), _) => last.push_str(&format!(
                ": \"{}\" — next step it named: \"{}\"",
                report.summary.trim(),
                report.next_step.trim()
            )),
            (None, Some(LoopTickOutcome::Failed { error, .. })) => {
                last.push_str(&format!(": {}", error.trim()))
            }
            (None, Some(LoopTickOutcome::NoReport)) => {
                last.push_str(": it ended without calling loop_report.")
            }
            // A yield, a question or the user's stop ended it; the line after says which.
            (None, _) => last.push('.'),
        }
        lines.push(last);

        if let Some(report) = &prev.report {
            if report.verdict == LoopVerdict::Blocked {
                if let Some(blocked_on) = report.blocked_on.as_deref().map(str::trim) {
                    lines.push(format!("Tick {p} was blocked on: \"{blocked_on}\""));
                }
            }
        }

        if let Some(run) = &prev.check {
            let command = &run.command;
            if run.ran {
                if prev.report.as_ref().map(|r| r.verdict) == Some(LoopVerdict::Done)
                    && run.exit != Some(0)
                {
                    match run.exit {
                        Some(code) => lines.push(format!(
                            "You reported the goal done in tick {p}; `{command}` exited {code}."
                        )),
                        None => lines.push(format!(
                            "You reported the goal done in tick {p}; `{command}` ended without an exit status."
                        )),
                    }
                }
                let result = match run.exit {
                    Some(0) => "passed".to_string(),
                    Some(code) => format!("exited {code}"),
                    None => "ended without an exit status".to_string(),
                };
                let tail = run.output_tail.trim_end();
                if tail.is_empty() {
                    lines.push(format!(
                        "Check `{command}` after tick {p}: {result}. Its output was empty."
                    ));
                } else {
                    lines.push(format!(
                        "Check `{command}` after tick {p}: {result}. Its output ended with:"
                    ));
                    lines.push(tail.to_string());
                }
            } else {
                let error = run
                    .error
                    .as_deref()
                    .unwrap_or("it did not start and named no error");
                lines.push(format!(
                    "Check `{command}` could not run after tick {p}: {error}."
                ));
            }
        }

        match &prev.outcome {
            Some(LoopTickOutcome::Yielded { to_chat, .. }) => {
                let at = prev
                    .ended_at
                    .as_deref()
                    .ok_or_else(|| format!("tick {p} yielded but has no end time"))?;
                lines.push(format!(
                    "Tick {p} was stopped at {} for the user's turn in \"{to_chat}\"; its partial work is above.",
                    hm(at)?
                ));
            }
            Some(LoopTickOutcome::Asked { question, .. }) => {
                let resolution = match facts.asked_resolution {
                    Some(AskedResolution::Answered) => "their answer is above",
                    Some(AskedResolution::Dismissed) => "they dismissed it",
                    Some(AskedResolution::Open) | None => "it is still open",
                };
                lines.push(format!(
                    "Tick {p} asked the user \"{question}\"; {resolution}."
                ));
            }
            _ => {}
        }
    }

    if record.cadence == LoopCadence::SelfPaced {
        lines.push("Say when to come back: next_in (\"10m\", \"2h\") and why.".to_string());
    }
    lines.push("Finish by calling loop_report; calling it ends this tick.".to_string());
    Ok(lines.join("\n"))
}
