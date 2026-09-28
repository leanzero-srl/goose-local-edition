//! The pure rules of a session loop (design §4.3–§4.7, §7.2, §7.4). Every function here is a pure
//! function of its arguments — time arrives as a value, never read from a clock — and the desktop's
//! `components/loops/model.ts` mirrors each one the UI needs. Both suites run
//! `loops.fixture.json`, whose expectations an independent Ruby encoding of the design computed
//! (`loops.fixture.gen.rb`), so neither implementation grades itself.
//!
//! Gate 1: no rule fills a missing input with a default. A self-paced tick that names no delay is
//! `waiting_you`, a check that could not run pauses with its error, a tick without a report is
//! `no_report`, an unreadable time or cadence in the record is an `Err` with its words.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, Timelike, Utc};
use goose_sdk_types::custom_requests::{
    LoopCadence, LoopCheckRun, LoopEdit, LoopNextReason, LoopNextTick, LoopRecord, LoopRefusal,
    LoopRefusalCode, LoopRefuseReason, LoopReport, LoopStatus, LoopStatusReason, LoopTemplateId,
    LoopTickOutcome, LoopTickRecord, LoopVerdict,
};
use serde::{Deserialize, Serialize};

use super::record::{fmt_time, parse_time};
use super::templates::{
    SLOTS, SLOT_CHECK, SLOT_GOAL_FIRST_LINE, SLOT_LAST_NEXT_STEP, SLOT_STATE_FILE, SLOT_WORKING_DIR,
};
use super::{LOOP_ID_PREFIX, TICK_ID_PREFIX};
use crate::loop_clock::{parse_cadence, DeskClock, WorkWindow};

// ---------------------------------------------------------------------------------------------
// Cadence
// ---------------------------------------------------------------------------------------------

/// `"10m"` → `(10, 'm')`, by the one cadence grammar (`loop_clock::parse_cadence`).
fn cadence_parts(text: &str) -> Option<(i64, char)> {
    parse_cadence(text)?;
    let text = text.trim();
    let unit = text.chars().last()?;
    let number = text.strip_suffix(unit)?.trim().parse().ok()?;
    Some((number, unit))
}

/// "every 10 min", "goose decides when", "back to back". A cadence outside the grammar is shown
/// as the user typed it, never as a guess.
pub fn cadence_label(cadence: &LoopCadence) -> String {
    match cadence {
        LoopCadence::Every { every } => match cadence_parts(every) {
            Some((n, 's')) => format!("every {n} s"),
            Some((n, 'm')) => format!("every {n} min"),
            Some((n, _)) => format!("every {n} h"),
            None => format!("every {}", every.trim()),
        },
        LoopCadence::SelfPaced => "goose decides when".to_string(),
        LoopCadence::BackToBack => "back to back".to_string(),
    }
}

/// Seconds, as a person reads a duration: "45s", "6m 12s", "41m", "2h", "1h 5m".
pub fn duration_words(seconds: i64) -> String {
    let s = seconds.max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        if m > 0 {
            format!("{h}h {m}m")
        } else {
            format!("{h}h")
        }
    } else if m > 0 {
        if sec > 0 {
            format!("{m}m {sec}s")
        } else {
            format!("{m}m")
        }
    } else {
        format!("{sec}s")
    }
}

/// `HH:MM` at the viewer's UTC offset (the fixture pins offset 0; the desktop passes its own).
pub fn clock_time(at: DateTime<Utc>, utc_offset_minutes: i32) -> Result<String, String> {
    let offset = FixedOffset::east_opt(utc_offset_minutes * 60)
        .ok_or_else(|| format!("{utc_offset_minutes} minutes is not a UTC offset"))?;
    let local = at.with_timezone(&offset);
    Ok(format!("{:02}:{:02}", local.hour(), local.minute()))
}

// ---------------------------------------------------------------------------------------------
// The next tick
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum NextTickDecision {
    /// The next tick starts at `at` (at or before now = due now).
    At { next: LoopNextTick },
    /// Due, but tick `n`'s end-of-turn reviewers still run on the same model (§4.3).
    AfterReviewers { n: u32 },
    /// Self-paced, and the tick just ended named no delay goose can read (§4.3).
    WaitingYou { reason: LoopStatusReason },
}

fn always_open(every: &str) -> Option<DeskClock> {
    DeskClock::new(
        "UTC",
        &WorkWindow {
            always: true,
            ..WorkWindow::default()
        },
        every,
    )
}

fn due(
    at: DateTime<Utc>,
    reason: LoopNextReason,
    now: DateTime<Utc>,
    last_n: u32,
    reviewers_pending: bool,
) -> NextTickDecision {
    if at <= now && reviewers_pending {
        NextTickDecision::AfterReviewers { n: last_n }
    } else {
        NextTickDecision::At {
            next: LoopNextTick {
                at: fmt_time(at),
                reason,
            },
        }
    }
}

/// When the tick after the last recorded one starts (§4.3). The last tick must have ended.
/// `reviewers_pending`: that tick's end-of-turn reviewers have not ended yet — no tick starts
/// beside them, whatever the cadence.
pub fn next_tick(
    record: &LoopRecord,
    now: DateTime<Utc>,
    reviewers_pending: bool,
) -> Result<NextTickDecision, String> {
    let Some(last) = record.ticks.last() else {
        return Ok(NextTickDecision::At {
            next: LoopNextTick {
                at: fmt_time(now),
                reason: LoopNextReason::First,
            },
        });
    };
    let ended_at = last
        .ended_at
        .as_deref()
        .ok_or_else(|| format!("tick {} has not ended", last.n))?;
    let ended_at = parse_time(ended_at)?;
    match &record.cadence {
        LoopCadence::Every { every } => {
            let clock = always_open(every).ok_or_else(|| {
                format!("the loop's cadence \"{every}\" is not <n>s, <n>m or <n>h")
            })?;
            let started = parse_time(&last.started_at)?;
            if started.checked_add_signed(clock.cadence).is_none() {
                return Err(format!(
                    "the cadence \"{every}\" reaches past the last date goose can hold"
                ));
            }
            let (at, why) = clock.next_tick(Some(started), now);
            let at = at.ok_or_else(|| format!("the clock named no next tick: {why}"))?;
            let reason = if why == "cadence" {
                LoopNextReason::Cadence
            } else if why.starts_with("overdue") {
                LoopNextReason::Overdue
            } else {
                return Err(format!("the clock's reason \"{why}\" is not a loop's"));
            };
            Ok(due(at, reason, now, last.n, reviewers_pending))
        }
        LoopCadence::SelfPaced => {
            let named = last.report.as_ref().and_then(|r| r.next_in.as_deref());
            let Some(given) = named.map(str::trim).filter(|g| !g.is_empty()) else {
                return Ok(NextTickDecision::WaitingYou {
                    reason: LoopStatusReason::NoDelay { n: last.n },
                });
            };
            let Some(delay) = parse_cadence(given) else {
                return Ok(NextTickDecision::WaitingYou {
                    reason: LoopStatusReason::BadDelay {
                        n: last.n,
                        given: given.to_string(),
                    },
                });
            };
            let Some(at) = ended_at.checked_add_signed(delay) else {
                return Ok(NextTickDecision::WaitingYou {
                    reason: LoopStatusReason::BadDelay {
                        n: last.n,
                        given: given.to_string(),
                    },
                });
            };
            let at = at.max(now);
            let reason = LoopNextReason::SelfPaced {
                interval: given.to_string(),
                reason: last.report.as_ref().and_then(|r| r.next_reason.clone()),
            };
            Ok(due(at, reason, now, last.n, reviewers_pending))
        }
        LoopCadence::BackToBack => Ok(due(
            now,
            LoopNextReason::BackToBack,
            now,
            last.n,
            reviewers_pending,
        )),
    }
}

// ---------------------------------------------------------------------------------------------
// After a tick
// ---------------------------------------------------------------------------------------------

/// Why a cancelled tick was cancelled. No cause = the user stopped it (the composer's Stop,
/// Escape, an interruption word).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CancelCause {
    /// A user reply contended with the tick (§5.3).
    Yield {
        to_session: String,
        to_chat: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        way: Option<String>,
    },
    /// Stop loop, from the rail or `/loop stop`.
    LoopStopped,
}

/// How the tick's turn ended (`tick_ended`, §5.2 step 6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TickEnd {
    Completed,
    Cancelled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause: Option<CancelCause>,
    },
    Errored {
        error_class: String,
        error: String,
    },
}

/// An open needs-you item the tick created (read by the runner from `needs_you.v0`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskedItem {
    pub item_id: String,
    pub question: String,
}

/// What the runner knows when a tick ends. The ended tick is the record's last, carrying its
/// report (written by `loop_report`) and its `wrote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickFacts {
    pub end: TickEnd,
    /// The check's run after this tick; `None` when no check is set, or the tick ended in a way the
    /// check does not run after (asked, cancelled, errored, no report, blocked).
    #[serde(default)]
    pub check: Option<LoopCheckRun>,
    #[serde(default)]
    pub asked: Option<AskedItem>,
    #[serde(default)]
    pub reviewers_pending: bool,
    pub now: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub outcome: LoopTickOutcome,
    pub status: LoopStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<LoopStatusReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_tick: Option<LoopNextTick>,
}

/// Whether the check must run after a tick that ended this way (§4.6: after a tick whose verdict
/// is done or progress, and only then).
pub fn check_runs_after(end: &TickEnd, report: Option<&LoopReport>, asked: bool) -> bool {
    matches!(end, TickEnd::Completed)
        && !asked
        && report.is_some_and(|r| matches!(r.verdict, LoopVerdict::Progress | LoopVerdict::Done))
}

fn outcome_of(tick: &LoopTickRecord, facts: &TickFacts) -> LoopTickOutcome {
    match &facts.end {
        TickEnd::Cancelled {
            cause:
                Some(CancelCause::Yield {
                    to_session,
                    to_chat,
                    way,
                }),
        } => LoopTickOutcome::Yielded {
            to_session: to_session.clone(),
            to_chat: to_chat.clone(),
            way: way.clone(),
        },
        TickEnd::Cancelled { .. } => LoopTickOutcome::StoppedByYou,
        _ if facts.asked.is_some() => match facts.asked.clone() {
            Some(AskedItem { item_id, question }) => LoopTickOutcome::Asked { item_id, question },
            None => unreachable!("the arm is guarded by facts.asked.is_some()"),
        },
        TickEnd::Errored { error_class, error } => LoopTickOutcome::Failed {
            error_class: error_class.clone(),
            error: error.clone(),
        },
        TickEnd::Completed => match &tick.report {
            None => LoopTickOutcome::NoReport,
            Some(report) => match report.verdict {
                LoopVerdict::Progress => LoopTickOutcome::Progress,
                LoopVerdict::Done => LoopTickOutcome::Done,
                LoopVerdict::Blocked => LoopTickOutcome::Blocked,
            },
        },
    }
}

fn normalized_step(step: &str) -> String {
    step.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Stalled (§4.6): tick `cur` reported progress, made no write or edit outside the state file
/// (`wrote` excludes it by construction), and named the same next step as `prev` (whitespace- and
/// case-normalised). A repeat of the previous tick — never a count or a clock.
pub fn stalled(prev: &LoopTickRecord, cur: &LoopTickRecord) -> bool {
    let (Some(prev_report), Some(cur_report)) = (&prev.report, &cur.report) else {
        return false;
    };
    cur_report.verdict == LoopVerdict::Progress
        && cur.wrote.is_empty()
        && normalized_step(&prev_report.next_step) == normalized_step(&cur_report.next_step)
}

fn is_failed(outcome: Option<&LoopTickOutcome>) -> Option<&str> {
    match outcome {
        Some(LoopTickOutcome::Failed { error_class, .. }) => Some(error_class),
        _ => None,
    }
}

/// The outcome of the tick that just ended and what the loop does next (§4.6). The order is the
/// design's: the user's stop of the loop, then the goal met / reported done, then the user's tick
/// count, then the user's stop of the tick, a yield, a question, then the self-pause rules (each a
/// repeat of the previous tick), then the next tick by the cadence.
pub fn decide_after_tick(record: &LoopRecord, facts: &TickFacts) -> Result<Decision, String> {
    let now = parse_time(&facts.now)?;
    let tick = record
        .ticks
        .last()
        .ok_or_else(|| "the loop has no tick to decide on".to_string())?;
    let n = tick.n;
    let prev = record
        .ticks
        .len()
        .checked_sub(2)
        .and_then(|i| record.ticks.get(i));
    let outcome = outcome_of(tick, facts);
    let decided = |status: LoopStatus, reason: LoopStatusReason| {
        Ok(Decision {
            outcome: outcome.clone(),
            status,
            reason: Some(reason),
            next_tick: None,
        })
    };

    if matches!(
        facts.end,
        TickEnd::Cancelled {
            cause: Some(CancelCause::LoopStopped)
        }
    ) {
        return decided(LoopStatus::Ended, LoopStatusReason::StoppedByYou { n });
    }

    let checked = matches!(outcome, LoopTickOutcome::Progress | LoopTickOutcome::Done);
    let check_run =
        match (&record.check, checked) {
            (Some(command), true) => Some(facts.check.as_ref().ok_or_else(|| {
                format!("the check `{command}` has no run recorded after tick {n}")
            })?),
            _ => None,
        };
    if let (Some(command), Some(run)) = (&record.check, check_run) {
        if run.ran && run.exit == Some(0) {
            return decided(
                LoopStatus::Ended,
                LoopStatusReason::GoalMet {
                    n,
                    check: command.clone(),
                },
            );
        }
    }
    if record.check.is_none() && outcome == LoopTickOutcome::Done {
        return decided(LoopStatus::Ended, LoopStatusReason::ReportedDone { n });
    }
    if let Some(k) = record.stop_after_ticks {
        if n >= k {
            return decided(LoopStatus::Ended, LoopStatusReason::ReachedCount { k });
        }
    }

    match &outcome {
        LoopTickOutcome::StoppedByYou => {
            return decided(LoopStatus::Paused, LoopStatusReason::YouStoppedTick { n })
        }
        LoopTickOutcome::Yielded {
            to_session,
            to_chat,
            ..
        } => {
            return decided(
                LoopStatus::WaitingTurn,
                LoopStatusReason::UserTurn {
                    session_id: to_session.clone(),
                    chat: to_chat.clone(),
                },
            )
        }
        LoopTickOutcome::Asked { item_id, question } => {
            return decided(
                LoopStatus::NeedsYou,
                LoopStatusReason::Asked {
                    n,
                    item_id: item_id.clone(),
                    question: question.clone(),
                },
            )
        }
        LoopTickOutcome::Blocked => {
            let blocked_on = tick
                .report
                .as_ref()
                .and_then(|r| r.blocked_on.as_deref())
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("tick {n} did not say what it is blocked on"));
            return decided(
                LoopStatus::Paused,
                LoopStatusReason::Blocked { n, blocked_on },
            );
        }
        _ => {}
    }

    if let Some(run) = check_run {
        if !run.ran {
            let error = run
                .error
                .clone()
                .unwrap_or_else(|| "the check did not start and named no error".to_string());
            return decided(
                LoopStatus::Paused,
                LoopStatusReason::CheckCouldNotRun { n, error },
            );
        }
    }
    if let (Some(class), Some(prev)) = (is_failed(Some(&outcome)), prev) {
        if is_failed(prev.outcome.as_ref()) == Some(class) {
            let LoopTickOutcome::Failed { error, .. } = &outcome else {
                unreachable!("is_failed matched")
            };
            return decided(
                LoopStatus::Paused,
                LoopStatusReason::SameFailureTwice {
                    prev: prev.n,
                    n,
                    error: error.clone(),
                },
            );
        }
    }
    if let (LoopTickOutcome::NoReport, Some(prev)) = (&outcome, prev) {
        if prev.outcome == Some(LoopTickOutcome::NoReport) {
            return decided(
                LoopStatus::Paused,
                LoopStatusReason::NoReportTwice { prev: prev.n, n },
            );
        }
    }
    if let (LoopTickOutcome::Progress, Some(prev)) = (&outcome, prev) {
        if stalled(prev, tick) {
            return decided(
                LoopStatus::Paused,
                LoopStatusReason::Stalled { prev: prev.n, n },
            );
        }
    }

    // The next tick is computed over the record with this tick ended (its report and times are
    // already on it).
    Ok(match next_tick(record, now, facts.reviewers_pending)? {
        NextTickDecision::At { next } => Decision {
            outcome,
            status: LoopStatus::Waiting,
            reason: None,
            next_tick: Some(next),
        },
        NextTickDecision::AfterReviewers { n } => Decision {
            outcome,
            status: LoopStatus::WaitingTurn,
            reason: Some(LoopStatusReason::Reviewers { n }),
            next_tick: None,
        },
        NextTickDecision::WaitingYou { reason } => Decision {
            outcome,
            status: LoopStatus::WaitingYou,
            reason: Some(reason),
            next_tick: None,
        },
    })
}

// ---------------------------------------------------------------------------------------------
// Who runs the clock
// ---------------------------------------------------------------------------------------------

/// What is known of the recorded owner (§5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OwnerProof {
    /// This goose process owns it.
    ThisProcess,
    /// Another goose process owns it and is proven live and attached.
    Live,
    /// Proven gone: the pid is dead, reused, or reparented away from its app.
    Gone { why: String },
    /// Nothing in this process can prove it either way (no runner installed). The written status
    /// stands — neither "closed" nor "elsewhere" is claimed without proof.
    Unproven { why: String },
}

/// The status as read now (§5.1): a pure read never writes; a loop whose owner is proven gone
/// reads `paused{closed}`, one another live goose runs reads `elsewhere`.
pub fn effective_status(
    record: &LoopRecord,
    proof: Option<&OwnerProof>,
) -> (LoopStatus, Option<LoopStatusReason>) {
    let written = (record.status, record.status_reason.clone());
    if matches!(record.status, LoopStatus::Paused | LoopStatus::Ended) {
        return written;
    }
    let closed = (
        LoopStatus::Paused,
        Some(LoopStatusReason::Closed { closed_at: None }),
    );
    match proof {
        None => closed,
        Some(OwnerProof::Gone { .. }) => closed,
        Some(OwnerProof::Live) => (LoopStatus::Elsewhere, None),
        Some(OwnerProof::ThisProcess) | Some(OwnerProof::Unproven { .. }) => written,
    }
}

/// How many ticks came due while no goose ran the clock: 0 before the next tick's time, one per
/// cadence interval since it for a fixed cadence, else one.
pub fn ticks_due(record: &LoopRecord, now: DateTime<Utc>) -> Result<u32, String> {
    let Some(next) = &record.next_tick else {
        return Ok(0);
    };
    let at = parse_time(&next.at)?;
    if at > now {
        return Ok(0);
    }
    Ok(match &record.cadence {
        LoopCadence::Every { every } => {
            let step = parse_cadence(every).ok_or_else(|| {
                format!("the loop's cadence \"{every}\" is not <n>s, <n>m or <n>h")
            })?;
            let missed = (now - at).num_seconds() / step.num_seconds();
            u32::try_from(missed + 1).map_err(|_| format!("{} ticks came due", missed + 1))?
        }
        LoopCadence::SelfPaced | LoopCadence::BackToBack => 1,
    })
}

// ---------------------------------------------------------------------------------------------
// The status sentence
// ---------------------------------------------------------------------------------------------

/// A sentence the user reads: its i18n key, its facts, and the English text the key's default
/// message renders to (the desktop's `defineMessages` use the same templates; the harness compares
/// the texts character for character).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub key: String,
    pub facts: BTreeMap<String, String>,
    pub text: String,
}

fn sentence(key: &str, facts: &[(&str, String)], text: String) -> Sentence {
    Sentence {
        key: key.to_string(),
        facts: facts
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        text,
    }
}

fn next_n(record: &LoopRecord) -> u32 {
    record.ticks.last().map(|t| t.n + 1).unwrap_or(1)
}

/// The NOW line of the rail for the status as read now (§8.4), with every fact it names.
pub fn status_sentence(
    record: &LoopRecord,
    status: LoopStatus,
    reason: Option<&LoopStatusReason>,
    now: DateTime<Utc>,
    utc_offset_minutes: i32,
) -> Result<Sentence, String> {
    let hm = |t: &str| parse_time(t).and_then(|t| clock_time(t, utc_offset_minutes));
    let since = |t: &str| parse_time(t).map(|t| duration_words((now - t).num_seconds()));
    let wrong = || format!("a {status:?} loop cannot carry the reason {reason:?}");
    let next = next_n(record).to_string();
    use LoopStatusReason as R;
    Ok(match status {
        LoopStatus::Running => {
            let tick = record
                .ticks
                .last()
                .ok_or_else(|| "a running loop has no tick".to_string())?;
            let (n, time, elapsed) = (
                tick.n.to_string(),
                hm(&tick.started_at)?,
                since(&tick.started_at)?,
            );
            match &tick.served {
                Some(served) => sentence(
                    "loops.now.runningOn",
                    &[
                        ("n", n.clone()),
                        ("time", time.clone()),
                        ("elapsed", elapsed.clone()),
                        ("node", served.node.clone()),
                    ],
                    format!("Tick {n} · started {time} · {elapsed} · on {}", served.node),
                ),
                None => sentence(
                    "loops.now.running",
                    &[
                        ("n", n.clone()),
                        ("time", time.clone()),
                        ("elapsed", elapsed.clone()),
                    ],
                    format!("Tick {n} · started {time} · {elapsed}"),
                ),
            }
        }
        LoopStatus::Checking => {
            let run = record
                .ticks
                .last()
                .and_then(|t| t.check.as_ref())
                .ok_or_else(|| "a checking loop has no check run".to_string())?;
            let elapsed = since(&run.started_at)?;
            sentence(
                "loops.now.checking",
                &[("check", run.command.clone()), ("elapsed", elapsed.clone())],
                format!("Checking `{}` · {elapsed}", run.command),
            )
        }
        LoopStatus::Waiting => {
            let next_tick = record
                .next_tick
                .as_ref()
                .ok_or_else(|| "a waiting loop has no next tick".to_string())?;
            let at = parse_time(&next_tick.at)?;
            let time = clock_time(at, utc_offset_minutes)?;
            match &next_tick.reason {
                LoopNextReason::SelfPaced {
                    interval,
                    reason: Some(why),
                } => sentence(
                    "loops.now.selfPaced",
                    &[
                        ("time", time.clone()),
                        ("interval", interval.clone()),
                        ("reason", why.clone()),
                    ],
                    format!("Next tick {time} — goose chose {interval}: \"{why}\""),
                ),
                LoopNextReason::SelfPaced {
                    interval,
                    reason: None,
                } => sentence(
                    "loops.now.selfPacedNoReason",
                    &[("time", time.clone()), ("interval", interval.clone())],
                    format!("Next tick {time} — goose chose {interval} and gave no reason"),
                ),
                _ if at <= now => sentence("loops.now.startsNow", &[], "Next tick starts now".into()),
                _ => {
                    let rel = duration_words((at - now).num_seconds());
                    sentence(
                        "loops.now.nextAt",
                        &[("time", time.clone()), ("rel", rel.clone())],
                        format!("Next tick {time} · in {rel}"),
                    )
                }
            }
        }
        LoopStatus::WaitingTurn => match reason.ok_or_else(wrong)? {
            R::UserTurn { chat, .. } => sentence(
                "loops.now.dueAfterYourTurn",
                &[("next", next.clone()), ("chat", chat.clone())],
                format!("Tick {next} is due — it starts when your turn in \"{chat}\" ends"),
            ),
            R::Refused { refused } => match refused {
                LoopRefuseReason::TurnRunning | LoopRefuseReason::QueuedMessage => sentence(
                    "loops.now.dueAfterYourMessage",
                    &[("next", next.clone())],
                    format!("Tick {next} is due — it starts after your message here"),
                ),
                LoopRefuseReason::PendingCancel => sentence(
                    "loops.now.dueAfterStop",
                    &[("next", next.clone())],
                    format!(
                        "Tick {next} is due — it starts when the answer you stopped here has settled"
                    ),
                ),
                LoopRefuseReason::LoadFailed { error } => sentence(
                    "loops.now.dueLoadFailed",
                    &[("next", next.clone()), ("error", error.clone())],
                    format!("Tick {next} is due — this chat could not be opened in the window: {error}"),
                ),
                LoopRefuseReason::SubmitFailed { error } => sentence(
                    "loops.now.dueSubmitFailed",
                    &[("next", next.clone()), ("error", error.clone())],
                    format!("Tick {next} is due — goose refused its message: {error}"),
                ),
            },
            R::Reviewers { n } => sentence(
                "loops.now.dueAfterReviewers",
                &[("next", next.clone()), ("n", n.to_string())],
                format!("Tick {next} is due — it starts when goose's check of tick {n} ends"),
            ),
            R::WayHeld { node, chat, target } => sentence(
                "loops.now.dueWayHeld",
                &[
                    ("next", next.clone()),
                    ("node", node.clone()),
                    ("chat", chat.clone()),
                    ("target", target.clone()),
                ],
                format!(
                    "Tick {next} is due — {node} is answering you in \"{chat}\"; the tick loads {target} after"
                ),
            ),
            _ => return Err(wrong()),
        },
        LoopStatus::WaitingYou => match reason.ok_or_else(wrong)? {
            R::NoDelay { n } => sentence(
                "loops.now.noDelay",
                &[("n", n.to_string())],
                format!("Tick {n} didn't say when to come back."),
            ),
            R::BadDelay { n, given } => sentence(
                "loops.now.badDelay",
                &[("n", n.to_string()), ("given", given.clone())],
                format!("Tick {n} named a delay goose can't read: \"{given}\"."),
            ),
            _ => return Err(wrong()),
        },
        LoopStatus::NeedsYou => match reason.ok_or_else(wrong)? {
            R::Asked { n, question, .. } => sentence(
                "loops.now.asked",
                &[("n", n.to_string()), ("question", question.clone())],
                format!("Tick {n} asked you: \"{question}\""),
            ),
            R::AnswerRunning { n } => sentence(
                "loops.now.answerRunning",
                &[("n", n.to_string())],
                format!("Your answer to tick {n} is running — the next tick starts after it"),
            ),
            _ => return Err(wrong()),
        },
        LoopStatus::Paused => match reason.ok_or_else(wrong)? {
            R::ByYou { after_tick: 0 } => sentence(
                "loops.paused.byYouBeforeFirst",
                &[],
                "Paused by you before the first tick.".into(),
            ),
            R::ByYou { after_tick } => sentence(
                "loops.paused.byYou",
                &[("n", after_tick.to_string())],
                format!("Paused by you after tick {after_tick}."),
            ),
            R::YouStoppedTick { n } => sentence(
                "loops.paused.youStoppedTick",
                &[("n", n.to_string())],
                format!("You stopped tick {n}."),
            ),
            R::Blocked { blocked_on, .. } => sentence(
                "loops.paused.blocked",
                &[("blockedOn", blocked_on.clone())],
                format!("Blocked — {blocked_on}"),
            ),
            R::CheckCouldNotRun { error, .. } => sentence(
                "loops.paused.checkCouldNotRun",
                &[("error", error.clone())],
                format!("The check could not run: {error}"),
            ),
            R::SameFailureTwice { prev, n, error } => sentence(
                "loops.paused.sameFailureTwice",
                &[
                    ("prev", prev.to_string()),
                    ("n", n.to_string()),
                    ("error", error.clone()),
                ],
                format!("Ticks {prev} and {n} failed the same way: {error}"),
            ),
            R::NoReportTwice { prev, n } => sentence(
                "loops.paused.noReportTwice",
                &[("prev", prev.to_string()), ("n", n.to_string())],
                format!("Ticks {prev} and {n} ended without a loop report"),
            ),
            R::Stalled { prev, n } => sentence(
                "loops.paused.stalled",
                &[("prev", prev.to_string()), ("n", n.to_string())],
                format!(
                    "Stalled — tick {n} named the same next step as tick {prev} and made no write or edit outside the state file"
                ),
            ),
            R::Closed { closed_at } => {
                let due = ticks_due(record, now)?;
                let were = if due == 1 {
                    "1 tick was due".to_string()
                } else {
                    format!("{due} ticks were due")
                };
                match closed_at {
                    Some(at) => {
                        let time = hm(at)?;
                        sentence(
                            "loops.paused.closedAt",
                            &[("time", time.clone()), ("due", due.to_string())],
                            format!("goose was closed at {time}; {were}."),
                        )
                    }
                    None => sentence(
                        "loops.paused.closed",
                        &[("due", due.to_string())],
                        format!("goose was closed; {were}."),
                    ),
                }
            }
            R::FinishingElsewhere { n } => sentence(
                "loops.paused.finishingElsewhere",
                &[("n", n.to_string())],
                format!("Paused — tick {n} is finishing in the other window"),
            ),
            _ => return Err(wrong()),
        },
        LoopStatus::Ended => match reason.ok_or_else(wrong)? {
            R::GoalMet { n, check } => sentence(
                "loops.ended.goalMet",
                &[("n", n.to_string()), ("check", check.clone())],
                format!("Goal met — `{check}` passed after tick {n}"),
            ),
            R::ReportedDone { n } => sentence(
                "loops.ended.reportedDone",
                &[("n", n.to_string())],
                format!("goose reported the goal done after tick {n} — no check was set"),
            ),
            R::ReachedCount { k } => sentence(
                "loops.ended.reachedCount",
                &[("k", k.to_string())],
                format!("Reached {k} ticks, as you set"),
            ),
            R::StoppedByYou { n } => sentence(
                "loops.ended.stoppedByYou",
                &[("n", n.to_string())],
                format!("Stopped by you after tick {n}"),
            ),
            _ => return Err(wrong()),
        },
        LoopStatus::Elsewhere => sentence(
            "loops.now.elsewhere",
            &[],
            "This loop runs in another goose window.".into(),
        ),
    })
}

// ---------------------------------------------------------------------------------------------
// Tick ids and ranges
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickId {
    pub loop_id: String,
    pub n: u32,
    pub uuid: String,
}

/// `looptick_<loopId>_<n>_<uuid>` — the id the runner mints with the offer and `on_prompt` stamps
/// on the tick's message.
pub fn tick_id(loop_id: &str, n: u32, uuid: &str) -> String {
    format!("{TICK_ID_PREFIX}{loop_id}_{n}_{uuid}")
}

fn is_loop_id(id: &str) -> bool {
    id.strip_prefix(LOOP_ID_PREFIX).is_some_and(|hex| {
        hex.len() == 8
            && hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    })
}

/// The parts of a tick id; `None` for any other id (a typed message is never read as a tick).
pub fn parse_tick_id(id: &str) -> Option<TickId> {
    let rest = id.strip_prefix(TICK_ID_PREFIX)?;
    let mut parts = rest.rsplitn(3, '_');
    let uuid = parts.next()?;
    let n = parts.next()?;
    let loop_id = parts.next()?;
    let uuid_ok = !uuid.is_empty()
        && uuid
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-');
    let n_ok = !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && !n.starts_with('0');
    if !(uuid_ok && n_ok && is_loop_id(loop_id)) {
        return None;
    }
    Some(TickId {
        loop_id: loop_id.to_string(),
        n: n.parse().ok()?,
        uuid: uuid.to_string(),
    })
}

/// One tick's messages: `[start, end)` from its marker to the next marker, or to the end. `None`
/// when an edit removed the tick's marker ("This tick's messages were removed by an edit"). A range
/// deliberately includes the user's steers and turns inside it: they happened during the tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickRange {
    pub n: u32,
    pub range: Option<(usize, usize)>,
}

pub fn tick_ranges(message_ids: &[Option<&str>], ticks: &[LoopTickRecord]) -> Vec<TickRange> {
    let marker = |tick: &LoopTickRecord| {
        message_ids
            .iter()
            .position(|id| *id == Some(tick.first_message_id.as_str()))
    };
    let starts: Vec<Option<usize>> = ticks.iter().map(marker).collect();
    ticks
        .iter()
        .zip(&starts)
        .map(|(tick, start)| TickRange {
            n: tick.n,
            range: start.map(|s| {
                let end = starts
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|other| *other > s)
                    .min()
                    .unwrap_or(message_ids.len());
                (s, end)
            }),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Steps and their slots
// ---------------------------------------------------------------------------------------------

/// What tick n−1 named as its next step, for `{last_next_step}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LastNextStep {
    /// This is the first tick.
    First,
    /// Tick `prev` named none (it ended without a report).
    NamedNone {
        prev: u32,
    },
    Named {
        text: String,
    },
}

/// The facts the slots are filled from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFacts {
    pub state_file: String,
    #[serde(default)]
    pub check: Option<String>,
    pub goal_first_line: String,
    pub last_next_step: LastNextStep,
    pub working_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedSteps {
    pub text: String,
    /// Slot names goose does not know, left literally in `text` ("{foo} is not a fact goose knows").
    pub unknown: Vec<String>,
}

enum Piece<'a> {
    Text(&'a str),
    Slot(&'a str),
}

/// `{name}` where name is `[a-z][a-z0-9_]*`; any other brace is text.
fn pieces(steps: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = steps;
    while let Some(open) = rest.find('{') {
        let (before, from_brace) = rest.split_at(open);
        if !before.is_empty() {
            out.push(Piece::Text(before));
        }
        let (brace, after) = from_brace.split_at('{'.len_utf8());
        let name_len = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(after.len());
        let (name, tail) = after.split_at(name_len);
        match (name.chars().next(), tail.strip_prefix('}')) {
            (Some(first), Some(tail)) if first.is_ascii_lowercase() => {
                out.push(Piece::Slot(name));
                rest = tail;
            }
            _ => {
                out.push(Piece::Text(brace));
                rest = after;
            }
        }
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

/// The slot names the steps use, each once, in first-use order.
pub fn step_slots(steps: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for piece in pieces(steps) {
        if let Piece::Slot(name) = piece {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

pub const NO_CHECK_SENTENCE: &str =
    "no check command is set; run the command that shows the change works and quote it";

/// The steps with every known slot filled from THIS loop's facts (§7.4); an absent fact renders
/// its own sentence, never a default value.
pub fn render_steps(steps: &str, facts: &StepFacts) -> RenderedSteps {
    let mut text = String::new();
    let mut unknown: Vec<String> = Vec::new();
    for piece in pieces(steps) {
        match piece {
            Piece::Text(t) => text.push_str(t),
            Piece::Slot(name) => match name {
                SLOT_STATE_FILE => text.push_str(&format!("`{}`", facts.state_file)),
                SLOT_WORKING_DIR => text.push_str(&format!("`{}`", facts.working_dir)),
                SLOT_GOAL_FIRST_LINE => text.push_str(&format!("\"{}\"", facts.goal_first_line)),
                SLOT_CHECK => match &facts.check {
                    Some(check) => text.push_str(&format!("`{check}`")),
                    None => text.push_str(NO_CHECK_SENTENCE),
                },
                SLOT_LAST_NEXT_STEP => match &facts.last_next_step {
                    LastNextStep::First => text.push_str("this is the first tick"),
                    LastNextStep::NamedNone { prev } => {
                        text.push_str(&format!("tick {prev} named no next step"))
                    }
                    LastNextStep::Named { text: step } => {
                        text.push_str(&format!("the last tick named \"{step}\""))
                    }
                },
                other => {
                    text.push('{');
                    text.push_str(other);
                    text.push('}');
                    if !unknown.iter().any(|u| u == other) {
                        unknown.push(other.to_string());
                    }
                }
            },
        }
    }
    RenderedSteps { text, unknown }
}

pub fn goal_first_line(goal: &str) -> String {
    goal.trim().lines().next().unwrap_or("").trim().to_string()
}

// ---------------------------------------------------------------------------------------------
// Starting and editing
// ---------------------------------------------------------------------------------------------

/// `.goose/loops/<slug>/NOW.md`, the slug from the goal's first words (the path is shown and
/// editable in the Start dialog; a goal with no letter or digit names the folder `loop`).
pub fn default_state_file(goal: &str) -> String {
    let words: Vec<String> = goal_first_line(goal)
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .map(str::to_string)
        .collect();
    let slug = if words.is_empty() {
        "loop".to_string()
    } else {
        words.join("-")
    };
    format!(".goose/loops/{slug}/NOW.md")
}

/// The state file as a path relative to the working dir, lexically: `.` and empty parts dropped,
/// `..` resolved; `Err(true)` = it leaves the working dir, `Err(false)` = it names nothing.
fn relative_state_file(state_file: &str, working_dir: &str) -> Result<String, bool> {
    let trimmed = state_file.trim();
    let relative = if trimmed.starts_with('/') {
        let dir = working_dir.trim_end_matches('/');
        match trimmed.strip_prefix(dir) {
            Some(rest) if !dir.is_empty() && (rest.is_empty() || rest.starts_with('/')) => rest,
            _ => return Err(true),
        }
    } else {
        trimmed
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(true);
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(false);
    }
    Ok(parts.join("/"))
}

/// What the start/edit validation needs to know about the chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatFacts {
    pub working_dir: String,
    /// The chat's model is `swarm-build` (or a `swarm-build:` strategy): every tick would start a
    /// full swarm build.
    #[serde(default)]
    pub swarm_build: bool,
}

fn refusal(code: LoopRefusalCode, reason: impl Into<String>) -> LoopRefusal {
    LoopRefusal {
        code,
        reason: reason.into(),
    }
}

/// Validate a start or an edit (§8.2's states): the loop as it will be stored — the state file
/// relative to the working dir, a blank check absent — or the one named refusal.
pub fn validate_loop(edit: &LoopEdit, chat: &ChatFacts) -> Result<LoopEdit, LoopRefusal> {
    if chat.swarm_build {
        return Err(refusal(
            LoopRefusalCode::SwarmBuild,
            "Loops run chat turns. This chat builds with the swarm, so every tick would start a full build. Use Agent Work for recurring builds.",
        ));
    }
    if edit.goal.trim().is_empty() {
        return Err(refusal(
            LoopRefusalCode::EmptyGoal,
            "Say what the loop should do.",
        ));
    }
    let check = edit
        .check
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    if edit.template == LoopTemplateId::UntilCheck && check.is_none() {
        return Err(refusal(
            LoopRefusalCode::CheckRequired,
            "This template needs a command to check.",
        ));
    }
    if let LoopCadence::Every { every } = &edit.cadence {
        if parse_cadence(every).is_none() {
            return Err(refusal(
                LoopRefusalCode::BadCadence,
                "Use a number and s, m or h — 90m, 2h",
            ));
        }
    }
    let state_file = match relative_state_file(&edit.state_file, &chat.working_dir) {
        Ok(path) => path,
        Err(true) => {
            return Err(refusal(
                LoopRefusalCode::StateFileOutside,
                format!("Keep the state file inside {}.", chat.working_dir),
            ))
        }
        Err(false) => {
            return Err(refusal(
                LoopRefusalCode::EmptyStateFile,
                "Name the state file — goose reads it first and rewrites it last.",
            ))
        }
    };
    if let Some(unknown) = step_slots(&edit.steps)
        .into_iter()
        .find(|slot| !SLOTS.contains(&slot.as_str()))
    {
        return Err(refusal(
            LoopRefusalCode::UnknownSlot,
            format!("{{{unknown}}} is not a fact goose knows"),
        ));
    }
    if edit.stop_after_ticks == Some(0) {
        return Err(refusal(
            LoopRefusalCode::BadStopAfter,
            "Stop after needs at least one tick — leave it empty to run until the goal is met or you stop it.",
        ));
    }
    Ok(LoopEdit {
        goal: edit.goal.clone(),
        template: edit.template,
        steps: edit.steps.clone(),
        cadence: match &edit.cadence {
            LoopCadence::Every { every } => LoopCadence::Every {
                every: every.trim().to_string(),
            },
            other => other.clone(),
        },
        state_file,
        check,
        stop_after_ticks: edit.stop_after_ticks,
    })
}

// ---------------------------------------------------------------------------------------------
// `/loop`
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopCommandRefusal {
    MissingGoal,
    MissingCadence,
    BadCadence,
    ControlTakesNoWords,
}

/// A `/loop` line (§7.2). The subcommand words are fixed, so "stop" is never read as a goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LoopCommand {
    Status,
    Now,
    Pause,
    Resume,
    Stop,
    Start {
        goal: String,
        cadence: LoopCadence,
    },
    Refused {
        code: LoopCommandRefusal,
        reason: String,
    },
}

impl LoopCommand {
    /// The forms the composer sends straight to `loops/get` / `loops/control` while a turn runs.
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            LoopCommand::Status
                | LoopCommand::Now
                | LoopCommand::Pause
                | LoopCommand::Resume
                | LoopCommand::Stop
        )
    }
}

fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(i) => {
            let (word, after) = text.split_at(i);
            (word, after.trim())
        }
        None => (text, ""),
    }
}

/// `None` = the line is not a `/loop` command.
pub fn parse_loop_command(line: &str) -> Option<LoopCommand> {
    let line = line.trim();
    let rest = line.strip_prefix("/loop")?;
    if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(LoopCommand::Status);
    }
    let (word, after) = split_word(rest);
    let lower = word.to_lowercase();
    let control = match lower.as_str() {
        "now" => Some(LoopCommand::Now),
        "pause" => Some(LoopCommand::Pause),
        "resume" => Some(LoopCommand::Resume),
        "stop" => Some(LoopCommand::Stop),
        _ => None,
    };
    if let Some(control) = control {
        if after.is_empty() {
            return Some(control);
        }
        return Some(LoopCommand::Refused {
            code: LoopCommandRefusal::ControlTakesNoWords,
            reason: format!(
                "/loop {lower} takes nothing after it — to loop on a goal that starts with \"{word}\", use the Loop button."
            ),
        });
    }
    if lower == "every" {
        let (every, goal) = split_word(after);
        if every.is_empty() {
            return Some(LoopCommand::Refused {
                code: LoopCommandRefusal::MissingCadence,
                reason: "Say how often and what: /loop every 10m <goal>.".to_string(),
            });
        }
        if parse_cadence(every).is_none() {
            return Some(LoopCommand::Refused {
                code: LoopCommandRefusal::BadCadence,
                reason: "Use a number and s, m or h — 90m, 2h".to_string(),
            });
        }
        if goal.is_empty() {
            return Some(LoopCommand::Refused {
                code: LoopCommandRefusal::MissingGoal,
                reason: format!("Say what the loop should do: /loop every {every} <goal>."),
            });
        }
        return Some(LoopCommand::Start {
            goal: goal.to_string(),
            cadence: LoopCadence::Every {
                every: every.to_string(),
            },
        });
    }
    Some(LoopCommand::Start {
        goal: rest.to_string(),
        cadence: LoopCadence::SelfPaced,
    })
}

// ---------------------------------------------------------------------------------------------
// The check's output tail
// ---------------------------------------------------------------------------------------------

pub const CHECK_TAIL_WINDOW_SHARE: usize = 64; // ratio: 1/64 of the session's context window

/// The token budget of a check's output tail in a tick prompt: 1/64 of the session's resolved
/// context window (§4.4) — 4,096 tokens on this fleet's 262,144-token window.
pub fn check_tail_budget(context_window_tokens: usize) -> usize {
    context_window_tokens / CHECK_TAIL_WINDOW_SHARE
}

/// The longest suffix of `output` whose token count is at most the budget, counted with the
/// caller's counter (goose's own `TokenCounter::count_tokens` in the runner). The search assumes a
/// suffix never counts more tokens than a longer suffix of the same text.
pub fn output_tail(
    output: &str,
    context_window_tokens: usize,
    count_tokens: impl Fn(&str) -> usize,
) -> &str {
    let budget = check_tail_budget(context_window_tokens);
    let mut boundaries: Vec<usize> = output.char_indices().map(|(i, _)| i).collect();
    boundaries.push(output.len());
    let suffix = |at: usize| output.split_at(boundaries[at]).1;
    let (mut lo, mut hi) = (0usize, boundaries.len() - 1);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if count_tokens(suffix(mid)) <= budget {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    suffix(lo)
}

// ---------------------------------------------------------------------------------------------
// What a tick wrote
// ---------------------------------------------------------------------------------------------

fn lexical_absolute(path: &str, working_dir: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", working_dir.trim_end_matches('/'), path)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The paths a tick's `write`/`edit` calls changed, from the diff each carries in its result
/// `_meta` (the same rule the ACP server forwards to the Changes rail), in first-touched order,
/// the state file excluded. The Rust half of the desktop's `sessionChanges`: a shell command that
/// changed files leaves no diff and is not listed, so every sentence built on this says
/// "write/edit", never "files changed".
pub fn wrote(
    messages: &[crate::conversation::message::Message],
    state_file: &str,
    working_dir: &str,
) -> Vec<String> {
    let state_file = lexical_absolute(state_file, working_dir);
    crate::agents::platform_extensions::developer::file_diff::written_files(messages)
        .into_iter()
        .map(|file| file.path)
        .filter(|path| lexical_absolute(path, working_dir) != state_file)
        .collect()
}
