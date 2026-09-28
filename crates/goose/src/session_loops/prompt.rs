//! The tick prompt (design §4.4), assembled from THIS loop's facts. Every line is a fact of the
//! loop or the user's own words, or an instructional constant branched on a measured predicate
//! (self-paced or not, a check ran or not, a yield or not). No line asserts context that may not
//! exist: the last-tick lines appear only when a tick n−1 exists, the check line only when a check
//! ran after it, and so on (gate 2; GEN-4). A tick cut short (a yield, the user's stop) is never
//! "the last tick": the last FINISHED tick's report and check are carried, and each unfinished
//! tick after it is named with what its record says it did (Q-278).

use std::collections::BTreeMap;

use goose_sdk_types::custom_requests::{
    LoopCadence, LoopRecord, LoopTickOutcome, LoopTickRecord, LoopVerdict,
};
use serde::{Deserialize, Serialize};

use super::record::parse_time;
use super::rules::{
    cadence_label, clock_time, goal_first_line, render_steps, LastNextStep, StepFacts,
};

/// How the question the last finished tick asked was resolved (read by the runner from
/// `needs_you.v0`). The answer is quoted from the item itself, so the prompt never depends on a
/// transcript compaction may have folded away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AskedResolution {
    Answered {
        answer: String,
    },
    Dismissed,
    /// The person wrote a message instead of answering (Q-298), quoted from the item.
    Superseded {
        message: String,
    },
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
    /// The name each chat a tick yielded to carries NOW, by session id, read when the prompt is
    /// written: a chat renamed since the yield (a new chat's first-turn auto-name) is named as it
    /// is. A chat that could not be read keeps the name the record holds from the yield.
    #[serde(default)]
    pub chat_names: BTreeMap<String, String>,
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

/// A tick cancelled before its turn ended — a yield to the user, or the user's stop — did not
/// finish its work, so the next tick must not read it as "the last tick".
pub fn unfinished(tick: &LoopTickRecord) -> bool {
    matches!(
        tick.outcome,
        Some(LoopTickOutcome::Yielded { .. } | LoopTickOutcome::StoppedByYou)
    )
}

/// The newest tick that ran to its end (`None` when none has), and the unfinished ticks after it,
/// oldest first.
pub fn last_finished(record: &LoopRecord) -> (Option<&LoopTickRecord>, &[LoopTickRecord]) {
    let split = record
        .ticks
        .iter()
        .rposition(|t| !unfinished(t))
        .map_or(0, |i| i + 1);
    let finished = split.checked_sub(1).and_then(|i| record.ticks.get(i));
    (finished, &record.ticks[split..])
}

/// What `{last_next_step}` says at the loop's next tick: the next step named by the newest tick
/// that reported, looking no further back than the last finished tick — an unfinished tick that
/// never reported does not erase the step the finished tick before it named.
pub fn last_next_step(record: &LoopRecord) -> LastNextStep {
    let (finished, cut_short) = last_finished(record);
    let reported = cut_short
        .iter()
        .rev()
        .chain(finished)
        .find_map(|t| t.report.as_ref().map(|r| (t.n, r.next_step.trim())));
    match (reported, finished.or(record.ticks.last())) {
        (Some((_, step)), _) if !step.is_empty() => LastNextStep::Named {
            text: step.to_string(),
        },
        (Some((n, _)), _) => LastNextStep::NamedNone { prev: n },
        (None, Some(tick)) => LastNextStep::NamedNone { prev: tick.n },
        (None, None) => LastNextStep::First,
    }
}

/// The step facts of the loop's next tick, from the record.
pub fn step_facts(record: &LoopRecord, working_dir: &str) -> StepFacts {
    StepFacts {
        state_file: record.state_file.clone(),
        check: record.check.clone(),
        goal_first_line: goal_first_line(&record.goal),
        last_next_step: last_next_step(record),
        working_dir: working_dir.to_string(),
    }
}

/// "tick 2", "ticks 2 and 3", "ticks 2, 3 and 4".
fn tick_numbers(ticks: &[LoopTickRecord]) -> String {
    let ns: Vec<String> = ticks.iter().map(|t| t.n.to_string()).collect();
    match ns.split_last() {
        Some((last, [])) => format!("tick {last}"),
        Some((last, rest)) => format!("ticks {} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The prompt of tick `n`, which follows the record's last tick (the tick before it, when there
/// is one).
pub fn tick_prompt(record: &LoopRecord, n: u32, facts: &PromptFacts) -> Result<String, String> {
    let hm = |t: &str| parse_time(t).and_then(|t| clock_time(t, facts.utc_offset_minutes));
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

    let steps = render_steps(&record.steps, &step_facts(record, &facts.working_dir));
    if !steps.text.trim().is_empty() {
        lines.push(
            "What each tick does (the user's steps, as they left them in the dialog):".to_string(),
        );
        lines.push(steps.text.trim().to_string());
    }

    let (finished, cut_short) = last_finished(record);
    if let Some(prev) = finished {
        let p = prev.n;
        let which = if cut_short.is_empty() {
            "Last tick"
        } else {
            "Last finished tick"
        };
        let mut last = format!(
            "{which} ({p}, {}, {})",
            hm(&prev.started_at)?,
            outcome_words(prev.outcome.as_ref())
        );
        match (&prev.report, &prev.outcome) {
            (Some(report), _) => last.push_str(&format!(
                ": \"{}\" — next step it named: \"{}\"",
                report.summary.trim(),
                report.next_step.trim()
            )),
            (None, Some(LoopTickOutcome::Failed { error_class, error })) => {
                last.push_str(&format!(
                    ": the turn ended with a {error_class} error: {}",
                    error.trim()
                ))
            }
            (None, Some(LoopTickOutcome::NoReport)) => {
                last.push_str(": it ended without calling loop_report.")
            }
            // A question ended it; the line after says which.
            (None, _) => last.push('.'),
        }
        lines.push(last);

        if let Some(report) = &prev.report {
            if report.verdict == LoopVerdict::Blocked {
                if let Some(blocked_on) = report.blocked_on.as_deref().map(str::trim) {
                    lines.push(format!(
                        "Tick {p} was blocked on: \"{blocked_on}\". Nothing in this loop records an answer; if the state file and the conversation above do not settle it, report blocked again rather than choosing."
                    ));
                }
            }
        }

        if let Some(run) = &prev.check {
            let command = &run.command;
            if run.ran {
                let result = match run.exit {
                    Some(0) => "passed".to_string(),
                    Some(code) => format!("exited {code}"),
                    None => "ended without an exit status".to_string(),
                };
                let reported_done =
                    prev.report.as_ref().map(|r| r.verdict) == Some(LoopVerdict::Done);
                let lead = if reported_done && run.exit != Some(0) {
                    format!("You reported the goal done in tick {p}, but `{command}` {result}.")
                } else {
                    format!("Check `{command}` after tick {p}: {result}.")
                };
                let tail = run.output_tail.trim_end();
                if tail.is_empty() {
                    lines.push(format!("{lead} Its output was empty."));
                } else {
                    lines.push(format!("{lead} Its output ended with:"));
                    lines.push(tail.to_string());
                }
            } else {
                let error = run
                    .error
                    .as_deref()
                    .unwrap_or("it did not start and named no error");
                lines.push(format!(
                    "Check `{command}` could not run after tick {p}: {error}. No result can be quoted from it until it runs; fix what stops it, or report blocked on it."
                ));
            }
        }

        if let Some(LoopTickOutcome::Asked { question, .. }) = &prev.outcome {
            let resolution = match &facts.asked_resolution {
                Some(AskedResolution::Answered { answer }) => {
                    format!("they answered: \"{answer}\"")
                }
                Some(AskedResolution::Dismissed) => "they dismissed it".to_string(),
                Some(AskedResolution::Superseded { message }) => {
                    format!("they did not answer it and wrote instead: \"{message}\"")
                }
                Some(AskedResolution::Open) | None => "it is still open".to_string(),
            };
            lines.push(format!(
                "Tick {p} asked the user \"{question}\"; {resolution}."
            ));
        }
    } else if !cut_short.is_empty() {
        lines.push("No tick of this loop has finished yet.".to_string());
    }

    for tick in cut_short {
        let k = tick.n;
        let at = tick
            .ended_at
            .as_deref()
            .ok_or_else(|| format!("tick {k} was stopped but has no end time"))?;
        let mut line = format!("Tick {k} ({}) did not finish: ", hm(&tick.started_at)?);
        match &tick.outcome {
            Some(LoopTickOutcome::Yielded {
                to_session,
                to_chat,
                ..
            }) => {
                let chat = facts.chat_names.get(to_session).unwrap_or(to_chat);
                line.push_str(&format!(
                    "it was stopped at {} for the user's turn in \"{chat}\".",
                    hm(at)?
                ));
            }
            _ => line.push_str(&format!("the user stopped it at {}.", hm(at)?)),
        }
        if let Some(report) = &tick.report {
            line.push_str(&format!(
                " Before it stopped, it reported: \"{}\" — next step it named: \"{}\".",
                report.summary.trim(),
                report.next_step.trim()
            ));
        }
        if tick.wrote.is_empty() {
            line.push_str(" It wrote or edited no file outside the state file;");
        } else {
            let wrote: Vec<String> = tick.wrote.iter().map(|p| format!("`{p}`")).collect();
            line.push_str(&format!(" It wrote or edited {};", wrote.join(", ")));
        }
        line.push_str(" any command it ran is in the conversation above.");
        lines.push(line);
    }
    if !cut_short.is_empty() {
        let own = if cut_short.len() == 1 {
            "a tick of its own"
        } else {
            "ticks of their own"
        };
        lines.push(format!(
            "Tick {n} carries on from there: what {} left unfinished is part of this tick's work, not {own}.",
            tick_numbers(cut_short)
        ));
    }

    if record.cadence == LoopCadence::SelfPaced {
        lines.push(
            "In loop_report, set next_in (\"10m\", \"2h\") and next_reason: when to come back, and why."
                .to_string(),
        );
    }
    lines.push("Finish by calling loop_report; calling it ends this tick.".to_string());
    Ok(lines.join("\n"))
}
