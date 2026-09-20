//! ONE TICK of a desk agent — the spine every desk skill already runs by hand, as engine phases:
//!
//!   GUARD   flags, the window, the desk's own guard scripts, the human's decisions folded in
//!   POLL    the read-only scripts; their stdout is the inbox
//!   ORIENT  the orchestrating node reads charter + scratchpad + ledger snowball + notes + inbox
//!           and decides the LANES (one per item), the ASKS, the DROPS
//!   LANES   surgeon calls fanned across the fleet's slots, queued when the fleet is full
//!   REVIEW  every draft attacked by the refuting lenses, in parallel
//!   SYNTH   the orchestrating node closes the tick: what to stage, ask, record, hand off
//!   POST    drafts staged on an EARLIER tick (and approved, when the desk requires it) go out
//!           through the desk's ONE write command — never a model, never this tick's draft
//!   CLOSE   close scripts, the daily log line, the commit
//!
//! Every phase writes state.json (the desk's phase clock), every unit of delivery is a ledger
//! mini, every lane is a keyed call so its words are readable live in `.swarm/activity/`.

use anyhow::Result;
use chrono::Utc;
use futures::future::join_all;
use goose::recipe::Response;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

use super::super::lenient_json::parse_json_lenient;
use super::super::planner_side_turns;
use super::super::supervision::said_kind_of;
use super::manifest::{AgentManifest, Approval};
use super::prompts::{self, Draft, LaneOut, LanePlan, LensOut, OrientOut, SynthOut};
use super::runtime::Fleet;
use super::scripts::{run_script, ScriptRun};
use super::store::{
    head_chars, now_rfc3339, sanitize_name, tail_chars, AskRow, DeskState, PreparedRow, RuntimeDir,
    TickSummary,
};
use super::window::DeskClock;
use goose_swarm::EventSink;

/// How many newest rows of each ledger kind the snowball block carries into a prompt. A count,
/// never a size: the whole ledger stays on disk and the block says how many it left out.
const LEDGER_NEWEST: usize = 24; // ratio: rows per kind the orchestrator re-reads each tick

pub struct TickCtx {
    pub manifest: Arc<AgentManifest>,
    pub dir: PathBuf,
    pub rt: RuntimeDir,
    pub fleet: Arc<Fleet>,
    pub sink: Arc<dyn EventSink>,
    pub env: Vec<(String, String)>,
    pub charter: Arc<String>,
    pub clock: DeskClock,
}

struct LaneResult {
    plan: LanePlan,
    key: String,
    model: String,
    secs: f64,
    out: Option<LaneOut>,
    raw: String,
    error: Option<String>,
    structured: bool,
}

struct LensResult {
    lane_key: String,
    lane_id: String,
    lens: String,
    model: String,
    secs: f64,
    out: LensOut,
    error: Option<String>,
}

fn set_phase(ctx: &TickCtx, st: &mut DeskState, phase: &str) {
    st.phase = phase.to_string();
    st.phase_started_at = Some(now_rfc3339());
    ctx.rt.write_state(st);
    ctx.sink
        .write_value(json!({"event": "tick_phase", "tick": st.tick, "phase": phase}));
}

/// The agent loop yields a provider/transport failure as the assistant's own TEXT and breaks
/// (agent.rs's error arms), so `run_agent_timed_at` returns Ok(text) for a dead node. The engine's
/// error-closer reader tells that text from anything a model said; a hit is NEVER parsed as an
/// answer — it is the failure, named.
fn transport_error(raw: &str) -> Option<String> {
    if said_kind_of(raw) == "error" {
        Some(tail_chars(raw.trim(), 400))
    } else {
        None
    }
}

/// End the tick before its work (a hold, or the orchestrator's call failing): the record, the
/// tick mini and the state carry the outcome and its reason, so the ledger shows the tick.
#[allow(clippy::too_many_arguments)]
fn close_early(
    ctx: &TickCtx,
    st: &mut DeskState,
    n: u64,
    record: &mut Value,
    started: std::time::Instant,
    started_at: &str,
    outcome: &str,
    summary: String,
    lane_secs: f64,
) -> TickSummary {
    let s = TickSummary {
        tick: n,
        started_at: started_at.to_string(),
        ended_at: now_rfc3339(),
        outcome: outcome.into(),
        summary,
        lanes: 0,
        staged: 0,
        posted: 0,
        asks: 0,
        lane_secs,
        wall_secs: started.elapsed().as_secs_f64(),
    };
    record["outcome"] = json!(outcome);
    record["summary"] = json!(s.summary);
    record["ended_at"] = json!(s.ended_at);
    record["lane_secs"] = json!(lane_secs);
    ctx.rt.write_tick_record(n, record);
    let _ = ctx.rt.write_mini(
        &format!("t{n}-tick"),
        &json!({"kind": "tick", "tick": n, "at": s.ended_at, "summary": s.summary, "handoff": "", "lanes": 0, "staged": 0, "posted": 0, "lane_secs": lane_secs, "outcome": outcome}),
    );
    finish(ctx, st, &s);
    s
}

fn script_value(r: &ScriptRun) -> Value {
    json!({
        "command": r.command,
        "exit": r.exit,
        "secs": r.secs,
        "stdout_chars": r.stdout.chars().count(),
        "stderr_tail": tail_chars(&r.stderr, 400),
    })
}

/// Run one tick. Never returns Err for model or script failures — those are outcomes the ledger
/// records; Err is reserved for the runtime dir itself being unusable.
pub async fn run_tick(ctx: &TickCtx, st: &mut DeskState, n: u64) -> Result<TickSummary> {
    ctx.rt.ensure()?;
    let started = std::time::Instant::now();
    let started_at = now_rfc3339();
    st.tick = n;
    st.status = "ticking".into();
    st.lanes_planned = 0;
    st.lanes_done = 0;
    st.hold_reason = None;
    ctx.sink
        .write_value(json!({"event": "tick_started", "tick": n, "at": started_at}));
    let mut record = json!({"tick": n, "started_at": started_at});
    let mut lane_secs = 0.0f64;

    // ---------------- GUARD
    set_phase(ctx, st, "guard");
    // An unreadable prepared/asks file STOPS the tick before anything could rewrite it empty.
    let mut hold: Option<String> = None;
    match ctx.rt.fold_decisions(st.decisions_applied) {
        Ok((applied, touched)) => {
            if applied != st.decisions_applied {
                ctx.sink.write_value(json!({
                    "event": "decisions_folded", "tick": n, "applied": applied - st.decisions_applied, "touched": touched,
                }));
                st.decisions_applied = applied;
            }
        }
        Err(e) => {
            ctx.sink
                .write_value(json!({"event": "store_unreadable", "tick": n, "error": e}));
            hold = Some(format!("a desk file cannot be read: {e}"));
        }
    }
    let window_open = ctx.clock.is_open(Utc::now());
    st.window_open = window_open;
    let mut guard_runs: Vec<ScriptRun> = Vec::new();
    if hold.is_none() && ctx.rt.is_paused() {
        hold = Some("paused by the human".into());
    }
    if hold.is_none() {
        hold = super::scripts::run_guards(&ctx.dir, &ctx.manifest.guard, &ctx.env, |r, reason| {
            ctx.sink
                .write_value(json!({"event": "guard_ran", "tick": n, "run": script_value(r)}));
            if !matches!(r.exit, Some(0) | Some(3)) {
                ctx.sink.write_value(json!({
                    "event": "guard_failed", "tick": n, "reason": reason,
                    "run": script_value(r),
                }));
            }
            guard_runs.push(r.clone());
        })
        .await;
    }
    record["guard"] = json!(guard_runs.iter().map(script_value).collect::<Vec<_>>());
    if let Some(why) = hold {
        st.hold_reason = Some(why.clone());
        ctx.sink
            .write_value(json!({"event": "tick_held", "tick": n, "reason": why}));
        return Ok(close_early(
            ctx,
            st,
            n,
            &mut record,
            started,
            &started_at,
            "held",
            format!("held: {why}"),
            0.0,
        ));
    }

    // ---------------- POLL
    set_phase(ctx, st, "poll");
    let (notes, inbox_err) = ctx.rt.take_inbox();
    if let Some(e) = inbox_err {
        ctx.sink
            .write_value(json!({"event": "inbox_unreadable", "tick": n, "error": e}));
    }
    if !notes.is_empty() {
        ctx.sink
            .write_value(json!({"event": "notes_taken", "tick": n, "count": notes.len()}));
    }
    let mut poll_runs: Vec<ScriptRun> = Vec::new();
    let mut poll_text = String::new();
    for cmd in &ctx.manifest.poll {
        let r = run_script(&ctx.dir, cmd, &ctx.env).await;
        ctx.sink
            .write_value(json!({"event": "poll_ran", "tick": n, "run": script_value(&r)}));
        poll_text.push_str(&format!("$ {cmd}\n"));
        match r.exit {
            Some(0) => poll_text.push_str(r.stdout.trim_end()),
            code => poll_text.push_str(&format!(
                "(exit {}) {}\n{}",
                code.map(|c| c.to_string()).unwrap_or_else(|| "none".into()),
                r.stdout.trim_end(),
                r.stderr.trim_end()
            )),
        }
        poll_text.push_str("\n\n");
        poll_runs.push(r);
    }
    record["poll"] = json!(poll_runs.iter().map(script_value).collect::<Vec<_>>());
    record["notes"] = json!(notes);
    if ctx.manifest.poll.is_empty() {
        ctx.sink.write_value(json!({"event": "poll_absent", "tick": n, "reason": "agent.yaml declares no poll command — the inbox is empty by construction"}));
    }

    // ---------------- ORIENT
    set_phase(ctx, st, "orient");
    let ledger_block = Arc::new(ctx.rt.render_ledger_block(LEDGER_NEWEST));
    let scratchpad = match ctx.rt.read_text(&ctx.manifest.scratchpad) {
        Ok(Some(s)) => s,
        Ok(None) => String::new(),
        Err(e) => {
            ctx.sink
                .write_value(json!({"event": "scratchpad_unreadable", "tick": n, "error": e}));
            format!("(the scratchpad file could not be read: {e})")
        }
    };
    let orient_key = format!("t{n}-orient");
    let orient_started = std::time::Instant::now();
    let orient_call = ctx
        .fleet
        .dispatcher
        .run_agent_timed_at(
            &ctx.fleet.planner_model,
            prompts::orient_system(&ctx.manifest, n),
            prompts::orient_user(
                &ctx.charter,
                &scratchpad,
                &ledger_block,
                &notes,
                &poll_text,
                &[],
            ),
            Some(Response {
                json_schema: Some(prompts::orient_schema()),
            }),
            planner_side_turns(),
            &[],
            None,
            Some(&orient_key),
            true,
            false,
        )
        .await;
    lane_secs += orient_started.elapsed().as_secs_f64();
    let orient: OrientOut = match orient_call {
        Ok(out) => {
            let raw = out.final_output.clone().unwrap_or_else(|| out.text.clone());
            record["orient_raw"] = json!(head_chars(&raw, 20_000));
            if let Some(err) = transport_error(&raw) {
                ctx.sink.write_value(json!({"event": "orient_failed", "tick": n, "kind": "transport", "model": ctx.fleet.planner_model, "error": err}));
                return Ok(close_early(
                    ctx,
                    st,
                    n,
                    &mut record,
                    started,
                    &started_at,
                    "failed",
                    format!(
                        "orchestrator call failed on {}: {err}",
                        ctx.fleet.planner_model
                    ),
                    lane_secs,
                ));
            }
            match parse_json_lenient::<OrientOut>(&raw) {
                Some(o) => o,
                None => {
                    ctx.sink.write_value(json!({"event": "orient_unparseable", "tick": n, "chars": raw.chars().count(), "tail": tail_chars(&raw, 400)}));
                    OrientOut {
                        summary: format!(
                            "orchestrator answered without a parseable plan: {}",
                            head_chars(raw.trim(), 400)
                        ),
                        ..Default::default()
                    }
                }
            }
        }
        Err(e) => {
            ctx.sink.write_value(json!({"event": "orient_failed", "tick": n, "kind": "call", "error": e.to_string()}));
            return Ok(close_early(
                ctx,
                st,
                n,
                &mut record,
                started,
                &started_at,
                "failed",
                format!("orchestrator call failed: {e}"),
                lane_secs,
            ));
        }
    };
    ctx.rt.mark_asks_consumed(n);
    // Lane ids must be unique keys on disk; sanitize and dedupe by suffix.
    let mut seen = std::collections::HashSet::new();
    let lanes: Vec<LanePlan> = orient
        .lanes
        .iter()
        .filter(|l| !l.item.trim().is_empty() || !l.objective.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let mut id = sanitize_name(l.id.trim().to_lowercase().as_str());
            if id.is_empty() {
                id = format!("lane-{}", i + 1);
            }
            while !seen.insert(id.clone()) {
                id = format!("{id}-{}", i + 1);
            }
            LanePlan {
                id,
                surgeon: l.surgeon.clone(),
                item: l.item.clone(),
                objective: l.objective.clone(),
                kind: l.kind.clone(),
            }
        })
        .collect();
    ctx.sink.write_value(json!({
        "event": "orient_done", "tick": n, "summary": orient.summary,
        "lanes": lanes.iter().map(|l| json!({"id": l.id, "surgeon": l.surgeon, "item": l.item, "kind": l.kind})).collect::<Vec<_>>(),
        "asks": orient.asks.len(), "drops": orient.drop.iter().map(|d| json!({"item": d.item, "why": d.why})).collect::<Vec<_>>(),
    }));
    if let Some(s) = &orient.scratchpad {
        if !s.trim().is_empty() {
            let _ = ctx.rt.write_text(&ctx.manifest.scratchpad, s.trim());
        }
    }
    record["orient"] = json!({
        "summary": orient.summary,
        "lanes": lanes.iter().map(|l| json!({"id": l.id, "surgeon": l.surgeon, "item": l.item, "objective": l.objective, "kind": l.kind})).collect::<Vec<_>>(),
        "asks": orient.asks.iter().map(|a| json!({"question": a.question, "why": a.why})).collect::<Vec<_>>(),
        "drop": orient.drop.iter().map(|d| json!({"item": d.item, "why": d.why})).collect::<Vec<_>>(),
    });
    st.lanes_planned = lanes.len() as u32;

    // ---------------- LANES (fanned over the slot pool; queued when full)
    set_phase(ctx, st, "lanes");
    let poll_arc = Arc::new(poll_text.clone());
    let lane_futs = lanes.clone().into_iter().map(|plan| {
        let ledger_block = ledger_block.clone();
        let poll = poll_arc.clone();
        async move { run_lane(ctx, n, plan, &ledger_block, &poll).await }
    });
    let lane_results: Vec<LaneResult> = join_all(lane_futs).await;
    st.lanes_done = lane_results.len() as u32;
    lane_secs += lane_results.iter().map(|r| r.secs).sum::<f64>();

    // ---------------- REVIEW (every draft, every lens, in parallel)
    let drafts: Vec<(&LaneResult, &LaneOut, &Draft)> = lane_results
        .iter()
        .filter_map(|r| {
            let o = r.out.as_ref()?;
            let d = o.draft.as_ref().filter(|d| !d.body.trim().is_empty())?;
            Some((r, o, d))
        })
        .collect();
    let mut lens_results: Vec<LensResult> = Vec::new();
    if ctx.manifest.review.enabled && !drafts.is_empty() && !ctx.manifest.review.lenses.is_empty() {
        set_phase(ctx, st, "review");
        let mut futs = Vec::new();
        for (r, o, d) in &drafts {
            for lens in &ctx.manifest.review.lenses {
                futs.push(run_lens(
                    ctx,
                    n,
                    &r.plan,
                    &r.key,
                    lens.clone(),
                    (*o).clone(),
                    (*d).clone(),
                ));
            }
        }
        lens_results = join_all(futs).await;
        lane_secs += lens_results.iter().map(|r| r.secs).sum::<f64>();
    }

    // ---------------- SYNTHESIS
    let pending_state_clear = match (ctx.rt.prepared(), ctx.rt.asks()) {
        (Ok(prepared), Ok(asks)) => prepared.is_empty() && asks.is_empty(),
        (Err(error), _) | (_, Err(error)) => {
            ctx.sink.write_value(
                json!({"event": "store_unreadable", "tick": n, "phase": "handoff", "error": error}),
            );
            false
        }
    };
    let mut tick_failure: Option<String> = None;
    let (synth, delivery_source) = if let Some((report, source)) = direct_read_only_report(
        &ctx.manifest,
        &orient,
        &lane_results,
        &lens_results,
        &notes,
        pending_state_clear,
    ) {
        set_phase(ctx, st, "handoff");
        ctx.sink.write_value(json!({"event": "synthesis_not_needed", "tick": n, "source": source,
            "reason": "one completed read-only research report; no drafts, questions, reviews or pending actions to reconcile"}));
        (report, source)
    } else {
        set_phase(ctx, st, "synthesis");
        let lane_reports = lane_results
            .iter()
            .map(render_lane_report)
            .collect::<Vec<_>>()
            .join("\n\n");
        let review_reports = lens_results
            .iter()
            .map(|l| {
                format!(
                    "REVIEW of {} by {} ({}, {:.0}s): {}\n  notes: {}{}{}",
                    l.lane_id,
                    l.lens,
                    l.model,
                    l.secs,
                    if l.out.verdict.is_empty() {
                        "(no verdict)"
                    } else {
                        &l.out.verdict
                    },
                    l.out.notes.trim(),
                    if l.out.fixes.is_empty() {
                        String::new()
                    } else {
                        format!("\n  fixes: {}", l.out.fixes.join(" | "))
                    },
                    l.error
                        .as_deref()
                        .map_or(String::new(), |e| format!("\n  reviewer error: {e}"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let synth_key = format!("t{n}-synthesis");
        let synth_started = std::time::Instant::now();
        let synth_call = ctx
            .fleet
            .dispatcher
            .run_agent_timed_at(
                &ctx.fleet.planner_model,
                prompts::synthesis_system(&ctx.manifest, n),
                prompts::synthesis_user(
                    &orient.summary,
                    &ledger_block,
                    &lane_reports,
                    &review_reports,
                    &notes,
                ),
                Some(Response {
                    json_schema: Some(prompts::synthesis_schema()),
                }),
                planner_side_turns(),
                &[],
                None,
                Some(&synth_key),
                true,
                false,
            )
            .await;
        lane_secs += synth_started.elapsed().as_secs_f64();
        let synth: SynthOut = match synth_call {
            Ok(out)
                if transport_error(
                    &out.final_output.clone().unwrap_or_else(|| out.text.clone()),
                )
                .is_some() =>
            {
                let raw = out.final_output.clone().unwrap_or_else(|| out.text.clone());
                let err = transport_error(&raw).unwrap_or_else(|| "(transport error)".to_string());
                ctx.sink.write_value(json!({"event": "synthesis_failed", "tick": n, "kind": "transport", "model": ctx.fleet.planner_model, "error": err}));
                let line = format!(
                    "tick {n}: synthesis call failed on {}: {err}",
                    ctx.fleet.planner_model
                );
                tick_failure = Some(line.clone());
                SynthOut {
                    log_line: line,
                    ..Default::default()
                }
            }
            Ok(out) => {
                let raw = out.final_output.clone().unwrap_or_else(|| out.text.clone());
                record["synthesis_raw"] = json!(head_chars(&raw, 20_000));
                parse_json_lenient::<SynthOut>(&raw).unwrap_or_else(|| {
                ctx.sink.write_value(json!({"event": "synthesis_unparseable", "tick": n, "tail": tail_chars(&raw, 400)}));
                SynthOut {
                    log_line: format!("tick {n}: synthesis answered without a parseable close ({} chars)", raw.chars().count()),
                    ..Default::default()
                }
            })
            }
            Err(e) => {
                ctx.sink.write_value(json!({"event": "synthesis_failed", "tick": n, "kind": "call", "error": e.to_string()}));
                let line = format!("tick {n}: synthesis call failed: {e}");
                tick_failure = Some(line.clone());
                SynthOut {
                    log_line: line,
                    ..Default::default()
                }
            }
        };

        (synth, json!({"mode": "model"}))
    };

    // Stage, ask, record — by CODE, from the synthesis's decisions or the source report.
    let now = now_rfc3339();
    // GUARD already refused the tick on an unreadable file; a file that broke between the two
    // reads is named here and nothing is staged over it.
    let mut prepared = match ctx.rt.prepared() {
        Ok(p) => p,
        Err(e) => {
            ctx.sink.write_value(
                json!({"event": "store_unreadable", "tick": n, "error": e, "phase": "synthesis"}),
            );
            Vec::new()
        }
    };
    let prepared_readable = !prepared.is_empty() || ctx.rt.prepared().is_ok();
    let mut staged_ids = Vec::new();
    for (k, s) in synth.stage.iter().enumerate() {
        if !prepared_readable {
            ctx.sink.write_value(json!({"event": "staging_skipped", "tick": n, "lane": s.lane, "reason": "prepared file unreadable"}));
            break;
        }
        if s.body.trim().is_empty() {
            continue;
        }
        let review: Vec<Value> = lens_results
            .iter()
            .filter(|l| l.lane_id == s.lane)
            .map(|l| json!({"lens": l.lens, "verdict": l.out.verdict, "notes": l.out.notes}))
            .collect();
        let row = match prepare_stage_row(s, &lane_results, &ctx.manifest.name, n, &now, review) {
            Ok(row) => row,
            Err(reason) => {
                ctx.sink.write_value(json!({"event": "staging_skipped", "tick": n, "lane": s.lane, "reason": reason}));
                continue;
            }
        };
        let id = row.id.clone();
        let _ = std::fs::write(ctx.rt.draft_path(&id), &row.body);
        ctx.sink.write_value(json!({"event": "draft_staged", "tick": n, "id": id, "lane": s.lane, "target": s.target, "kind": row.kind, "chars": row.body.chars().count()}));
        let _ = ctx.rt.write_mini(
            &format!("t{n}-staged-{}", k + 1),
            &json!({"kind": "staged", "tick": n, "at": now, "id": id, "target": s.target, "draft_kind": row.kind, "lane": s.lane}),
        );
        prepared.retain(|r| r.id != id);
        prepared.push(row);
        staged_ids.push(id);
    }
    if prepared_readable {
        ctx.rt.write_prepared(&prepared);
    }

    let (mut asks, asks_readable) = match ctx.rt.asks() {
        Ok(a) => (a, true),
        Err(e) => {
            ctx.sink.write_value(
                json!({"event": "store_unreadable", "tick": n, "error": e, "phase": "synthesis"}),
            );
            (Vec::new(), false)
        }
    };
    let mut ask_ids = Vec::new();
    for (k, a) in orient.asks.iter().chain(synth.asks.iter()).enumerate() {
        if a.question.trim().is_empty() || !asks_readable {
            continue;
        }
        if asks
            .iter()
            .any(|x| x.status == "open" && x.question == a.question)
        {
            continue;
        }
        let id = format!("ask-t{n}-{}", k + 1);
        asks.push(AskRow {
            id: id.clone(),
            tick: n,
            question: a.question.trim().to_string(),
            why: a.why.trim().to_string(),
            status: "open".into(),
            raised_at: now.clone(),
            answer: None,
            answered_at: None,
            consumed_tick: None,
        });
        ctx.sink.write_value(
            json!({"event": "ask_raised", "tick": n, "id": id, "question": a.question}),
        );
        let _ = ctx.rt.write_mini(
            &format!("t{n}-ask-{}", k + 1),
            &json!({"kind": "ask", "tick": n, "at": now, "id": id, "question": a.question, "why": a.why}),
        );
        let _ = ctx.rt.append_line(
            &ctx.manifest.pending,
            &format!("- [ ] {id}: {} — {}", a.question.trim(), a.why.trim()),
        );
        ask_ids.push(id);
    }
    if asks_readable {
        ctx.rt.write_asks(&asks);
    }
    for line in &synth.pending {
        if !line.trim().is_empty() {
            let _ = ctx.rt.append_line(
                &ctx.manifest.pending,
                &format!("- [ ] t{n}: {}", line.trim()),
            );
        }
    }
    for (k, f) in synth.facts.iter().enumerate() {
        if f.trim().is_empty() {
            continue;
        }
        let _ = ctx.rt.write_mini(
            &format!("t{n}-fact-{}", k + 1),
            &json!({"kind": "fact", "tick": n, "at": now, "fact": f.trim()}),
        );
    }
    if let Some(s) = &synth.scratchpad {
        if !s.trim().is_empty() {
            let _ = ctx.rt.write_text(&ctx.manifest.scratchpad, s.trim());
        }
    }
    ctx.sink.write_value(json!({
        "event": "synthesis_done", "tick": n, "staged": staged_ids, "asks": ask_ids,
        "facts": synth.facts.len(), "log_line": synth.log_line, "handoff": synth.handoff, "source": delivery_source,
    }));

    // ---------------- POST (earlier ticks' staged drafts, through the one write path)
    set_phase(ctx, st, "post");
    let mut posted_ids: Vec<String> = Vec::new();
    if let Ok(mut prepared) = ctx.rt.prepared().map_err(|e| {
        ctx.sink.write_value(
            json!({"event": "store_unreadable", "tick": n, "error": e, "phase": "post"}),
        );
    }) {
        let approval = ctx
            .manifest
            .post
            .as_ref()
            .map(|p| p.approval)
            .unwrap_or(Approval::Human);
        let eligible: Vec<usize> = prepared
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.tick < n
                    && (r.status == "approved"
                        || (r.status == "staged" && approval == Approval::None))
            })
            .map(|(i, _)| i)
            .collect();
        if !eligible.is_empty() {
            match (&ctx.manifest.post, window_open) {
                (None, _) => ctx.sink.write_value(json!({
                    "event": "post_skipped", "tick": n, "count": eligible.len(),
                    "reason": "agent.yaml declares no post command — staged drafts wait for the human to act on them",
                })),
                (Some(_), false) => ctx.sink.write_value(json!({
                    "event": "post_skipped", "tick": n, "count": eligible.len(),
                    "reason": format!("outside the desk's window ({})", ctx.clock.local_label(Utc::now())),
                })),
                (Some(post), true) => {
                    for i in eligible {
                        let row = &mut prepared[i];
                        let file = ctx.rt.draft_path(&row.id);
                        let _ = std::fs::write(&file, &row.body);
                        let cmd = post
                            .command
                            .replace("{id}", &row.id)
                            .replace("{file}", &file.display().to_string());
                        let mut env = ctx.env.clone();
                        env.push(("AGENT_TICK".into(), n.to_string()));
                        env.push(("AGENT_DRAFT_ID".into(), row.id.clone()));
                        env.push(("AGENT_DRAFT_FILE".into(), file.display().to_string()));
                        env.push(("AGENT_DRAFT_TARGET".into(), row.target.clone()));
                        env.push(("AGENT_DRAFT_KIND".into(), row.kind.clone()));
                        let r = run_script(&ctx.dir, &cmd, &env).await;
                        ctx.sink.write_value(json!({"event": "post_ran", "tick": n, "id": row.id, "run": script_value(&r)}));
                        if r.exit == Some(0) {
                            row.status = "posted".into();
                            row.posted_tick = Some(n);
                            row.result = Some(tail_chars(r.stdout.trim(), 2_000));
                            ctx.sink.write_value(json!({"event": "posted", "tick": n, "id": row.id, "target": row.target}));
                            let _ = ctx.rt.write_mini(
                                &format!("t{n}-posted-{}", sanitize_name(&row.id)),
                                &json!({"kind": "posted", "tick": n, "at": now_rfc3339(), "id": row.id, "target": row.target, "draft_kind": row.kind, "result": row.result}),
                            );
                            posted_ids.push(row.id.clone());
                        } else {
                            row.status = "failed".into();
                            row.result = Some(tail_chars(&format!("exit {:?}\n{}\n{}", r.exit, r.stdout.trim(), r.stderr.trim()), 2_000));
                            ctx.sink.write_value(json!({"event": "post_failed", "tick": n, "id": row.id, "exit": r.exit, "stderr_tail": tail_chars(&r.stderr, 600)}));
                        }
                    }
                }
            }
        }
        ctx.rt.write_prepared(&prepared);
    }

    // ---------------- CLOSE
    set_phase(ctx, st, "close");
    let mut close_runs = Vec::new();
    for cmd in &ctx.manifest.close {
        let r = run_script(&ctx.dir, cmd, &ctx.env).await;
        ctx.sink
            .write_value(json!({"event": "close_ran", "tick": n, "run": script_value(&r)}));
        close_runs.push(script_value(&r));
    }
    let log_line = if synth.log_line.trim().is_empty() {
        format!("tick {n}: {}", head_chars(orient.summary.trim(), 300))
    } else {
        synth.log_line.trim().to_string()
    };
    let _ = ctx.rt.append_line(
        &ctx.manifest.ledger,
        &format!(
            "{} tick {n} — {log_line}",
            ctx.clock.local_label(Utc::now())
        ),
    );
    if ctx.manifest.commit {
        let inside = run_script(&ctx.dir, "git rev-parse --is-inside-work-tree", &[]).await;
        if inside.exit == Some(0) {
            let msg = format!(
                "agent {} tick {n}: {}",
                ctx.manifest.name,
                head_chars(&log_line, 120)
            )
            .replace('"', "'");
            let r = run_script(&ctx.dir, &format!("git add -A && git -c commit.gpgsign=false commit -qm \"{msg}\" && git rev-parse --short HEAD"), &[]).await;
            if r.exit == Some(0) {
                ctx.sink
                    .write_value(json!({"event": "committed", "tick": n, "sha": r.stdout.trim()}));
            } else {
                ctx.sink.write_value(json!({"event": "commit_skipped", "tick": n, "reason": tail_chars(&format!("{}{}", r.stdout, r.stderr), 300).trim()}));
            }
        } else {
            ctx.sink.write_value(json!({"event": "commit_skipped", "tick": n, "reason": "the agent directory is not a git work tree"}));
        }
    }

    let ended_at = now_rfc3339();
    let outcome = if tick_failure.is_some() {
        "failed"
    } else {
        "done"
    };
    let summary = TickSummary {
        tick: n,
        started_at: started_at.clone(),
        ended_at: ended_at.clone(),
        outcome: outcome.into(),
        summary: log_line.clone(),
        lanes: lane_results.len() as u32,
        staged: staged_ids.len() as u32,
        posted: posted_ids.len() as u32,
        asks: ask_ids.len() as u32,
        lane_secs,
        wall_secs: started.elapsed().as_secs_f64(),
    };
    record["lanes"] = json!(lane_results.iter().map(lane_value).collect::<Vec<_>>());
    record["review"] = json!(lens_results.iter().map(|l| json!({
        "lane": l.lane_id, "key": format!("{}-lens-{}", l.lane_key, l.lens), "lens": l.lens, "model": l.model, "secs": l.secs,
        "verdict": l.out.verdict, "notes": l.out.notes, "fixes": l.out.fixes, "error": l.error,
    })).collect::<Vec<_>>());
    record["synthesis"] = json!({
        "staged": staged_ids, "asks": ask_ids, "facts": synth.facts, "log_line": log_line,
        "handoff": synth.handoff, "pending": synth.pending,
        "source": delivery_source,
    });
    record["posted"] = json!(posted_ids);
    record["close"] = json!(close_runs);
    record["outcome"] = json!(outcome);
    record["summary"] = json!(summary.summary);
    record["ended_at"] = json!(ended_at);
    record["lane_secs"] = json!(lane_secs);
    record["wall_secs"] = json!(summary.wall_secs);
    ctx.rt.write_tick_record(n, &record);
    let _ = ctx.rt.write_mini(
        &format!("t{n}-tick"),
        &json!({"kind": "tick", "tick": n, "at": ended_at, "summary": summary.summary, "handoff": synth.handoff,
                "lanes": summary.lanes, "staged": summary.staged, "posted": summary.posted, "lane_secs": lane_secs, "outcome": outcome}),
    );
    finish(ctx, st, &summary);
    Ok(summary)
}

fn finish(ctx: &TickCtx, st: &mut DeskState, summary: &TickSummary) {
    ctx.sink.write_value(json!({
        "event": "tick_done", "tick": summary.tick, "outcome": summary.outcome, "summary": summary.summary,
        "lanes": summary.lanes, "staged": summary.staged, "posted": summary.posted, "asks": summary.asks,
        "lane_secs": summary.lane_secs, "wall_secs": summary.wall_secs,
    }));
    st.last_tick = Some(summary.clone());
    st.phase = "idle".into();
    st.phase_started_at = None;
    ctx.rt.write_state(st);
}

// The two `match`es on an absent Option are spelled out on purpose: the fallback gate ratchets the
// default-on-absence call in the run path and wants each honest empty named where it is decided.
#[allow(clippy::manual_unwrap_or_default)]
async fn run_lane(
    ctx: &TickCtx,
    n: u64,
    plan: LanePlan,
    ledger_block: &str,
    poll: &str,
) -> LaneResult {
    let key = format!("t{n}-{}", plan.id);
    let surgeon = ctx.manifest.surgeon(&plan.surgeon);
    let surgeon_charter = surgeon.map(|s| {
        let mut t = String::new();
        if let Some(rel) = &s.charter {
            let p = ctx.dir.join(rel);
            match std::fs::read_to_string(&p) {
                Ok(x) => t.push_str(x.trim_end()),
                Err(e) => t.push_str(&format!(
                    "(surgeon charter {} could not be read: {e})",
                    p.display()
                )),
            }
        }
        if !s.brief.trim().is_empty() {
            if !t.is_empty() {
                t.push_str("\n\n");
            }
            t.push_str(s.brief.trim());
        }
        t
    });
    // No surgeon matched → the lane prompt says so in words (lane_system's absence line).
    let surgeon_charter = match surgeon_charter {
        Some(t) => t,
        None => String::new(),
    };
    if surgeon.is_none() && !plan.surgeon.is_empty() {
        ctx.sink.write_value(json!({"event": "surgeon_unknown", "tick": n, "lane": plan.id, "surgeon": plan.surgeon, "reason": "agent.yaml declares no surgeon by that name — the lane runs on the desk charter alone"}));
    }
    let read_only = surgeon.map(|s| s.read_only).unwrap_or(true);
    ctx.sink.write_value(json!({"event": "lane_queued", "tick": n, "key": key, "lane": plan.id, "inflight": ctx.fleet.slots.inflight(), "capacity": ctx.fleet.slots.capacity()}));
    let guard = ctx.fleet.slots.acquire().await;
    let model = guard.model_id.clone();
    ctx.sink.write_value(json!({"event": "lane_dispatched", "tick": n, "key": key, "lane": plan.id, "surgeon": plan.surgeon, "item": plan.item, "kind": plan.kind, "model": model, "read_only": read_only}));
    let started = std::time::Instant::now();
    let call = ctx
        .fleet
        .dispatcher
        .run_agent_timed_at(
            &model,
            prompts::lane_system(&ctx.manifest, surgeon, &surgeon_charter),
            prompts::lane_user(
                &ctx.charter,
                ledger_block,
                &plan,
                &prompts::poll_excerpt_for(poll, &plan.item),
            ),
            Some(Response {
                json_schema: Some(prompts::lane_schema()),
            }),
            planner_side_turns(),
            &ctx.fleet.extensions,
            None,
            Some(&key),
            read_only,
            false,
        )
        .await;
    drop(guard);
    let secs = started.elapsed().as_secs_f64();
    let (out, raw, error, structured) = match call {
        Ok(o) => {
            let collected = o.final_output.is_some();
            let raw = o.final_output.clone().unwrap_or_else(|| o.text.clone());
            if let Some(err) = transport_error(&raw) {
                (None, raw, Some(format!("transport: {err}")), false)
            } else {
                let parsed = parse_json_lenient::<LaneOut>(&raw);
                let structured = collected && parsed.is_some();
                let parsed = parsed.unwrap_or_else(|| LaneOut {
                    finding: raw.trim().to_string(),
                    ..Default::default()
                });
                (Some(parsed), raw, None, structured)
            }
        }
        Err(e) => (None, String::new(), Some(e.to_string()), false),
    };
    let res = LaneResult {
        plan,
        key,
        model,
        secs,
        out,
        raw,
        error,
        structured,
    };
    ctx.sink.write_value(json!({
        "event": if res.error.is_some() { "lane_failed" } else { "lane_done" },
        "tick": n, "key": res.key, "lane": res.plan.id, "model": res.model, "secs": secs,
        "has_draft": res.out.as_ref().and_then(|o| o.draft.as_ref()).is_some(),
        "confidence": res.out.as_ref().map(|o| o.confidence),
        "ask": res.out.as_ref().and_then(|o| o.ask.clone()),
        "route": res.out.as_ref().and_then(|o| o.route.clone()),
        "error": res.error,
    }));
    let _ = ctx
        .rt
        .write_mini(&format!("t{n}-lane-{}", res.plan.id), &lane_value(&res));
    res
}

async fn run_lens(
    ctx: &TickCtx,
    n: u64,
    plan: &LanePlan,
    lane_key: &str,
    lens: String,
    lane_out: LaneOut,
    draft: Draft,
) -> LensResult {
    let key = format!("{lane_key}-lens-{}", sanitize_name(&lens));
    let guard = ctx.fleet.slots.acquire().await;
    let model = guard.model_id.clone();
    ctx.sink.write_value(json!({"event": "lens_dispatched", "tick": n, "key": key, "lane": plan.id, "lens": lens, "model": model}));
    let started = std::time::Instant::now();
    let call = ctx
        .fleet
        .dispatcher
        .run_agent_timed_at(
            &model,
            prompts::lens_system(&ctx.manifest, &lens),
            prompts::lens_user(&ctx.charter, plan, &lane_out, &draft),
            Some(Response {
                json_schema: Some(prompts::lens_schema()),
            }),
            planner_side_turns(),
            &ctx.fleet.extensions,
            None,
            Some(&key),
            true,
            false,
        )
        .await;
    drop(guard);
    let secs = started.elapsed().as_secs_f64();
    let (out, error) = match call {
        Ok(o)
            if transport_error(&o.final_output.clone().unwrap_or_else(|| o.text.clone()))
                .is_some() =>
        {
            let raw = o.final_output.clone().unwrap_or_else(|| o.text.clone());
            let err = transport_error(&raw).unwrap_or_else(|| "(transport error)".to_string());
            (
                LensOut {
                    verdict: "REFUTED".into(),
                    notes: format!("reviewer transport error: {err}"),
                    fixes: vec![],
                },
                Some(format!("transport: {err}")),
            )
        }
        Ok(o) => {
            let raw = o.final_output.clone().unwrap_or_else(|| o.text.clone());
            (
                parse_json_lenient::<LensOut>(&raw).unwrap_or_else(|| LensOut {
                    verdict: "REFUTED".into(),
                    notes: format!(
                        "reviewer answered without a parseable verdict: {}",
                        head_chars(raw.trim(), 600)
                    ),
                    fixes: vec![],
                }),
                None,
            )
        }
        Err(e) => (
            LensOut {
                verdict: "REFUTED".into(),
                notes: format!("reviewer call failed: {e}"),
                fixes: vec![],
            },
            Some(e.to_string()),
        ),
    };
    let verdict = if out.verdict.to_uppercase().contains("PASS") {
        "PASS"
    } else {
        "REFUTED"
    }
    .to_string();
    let res = LensResult {
        lane_key: lane_key.to_string(),
        lane_id: plan.id.clone(),
        lens: lens.clone(),
        model,
        secs,
        out: LensOut { verdict, ..out },
        error,
    };
    ctx.sink.write_value(json!({"event": "lens_done", "tick": n, "key": key, "lane": plan.id, "lens": lens, "verdict": res.out.verdict, "secs": secs, "error": res.error}));
    let _ = ctx.rt.write_mini(
        &format!("t{n}-lens-{}-{}", plan.id, sanitize_name(&lens)),
        &json!({"kind": "lens", "tick": n, "at": now_rfc3339(), "lane": plan.id, "lens": lens, "verdict": res.out.verdict, "notes": res.out.notes, "fixes": res.out.fixes, "model": res.model, "secs": secs}),
    );
    res
}

#[allow(clippy::manual_unwrap_or_default)]
fn lane_value(r: &LaneResult) -> Value {
    // A lane with no parsed output carries its `error` (or `raw_tail`) — the empty fields are named by it.
    let o = match r.out.clone() {
        Some(o) => o,
        None => LaneOut::default(),
    };
    json!({
        "kind": "lane", "tick": r.key.split('-').next().and_then(|t| t.trim_start_matches('t').parse::<u64>().ok()).unwrap_or(0),
        "at": now_rfc3339(),
        "key": r.key, "lane": r.plan.id, "surgeon": r.plan.surgeon, "item": r.plan.item, "objective": r.plan.objective,
        "lane_kind": r.plan.kind, "model": r.model, "secs": r.secs,
        "homework": o.homework, "finding": o.finding,
        "draft": o.draft.as_ref().map(|d| json!({"target": d.target, "kind": d.kind, "body": d.body})),
        "ask": o.ask, "route": o.route, "confidence": o.confidence, "evidence": o.evidence, "next_step": o.next_step,
        "raw_tail": if r.out.is_some() { Value::Null } else { json!(tail_chars(&r.raw, 400)) },
        "error": r.error,
    })
}

fn direct_read_only_report(
    manifest: &AgentManifest,
    orient: &OrientOut,
    lanes: &[LaneResult],
    reviews: &[LensResult],
    notes: &[String],
    pending_state_clear: bool,
) -> Option<(SynthOut, Value)> {
    let [lane] = lanes else { return None };
    if !pending_state_clear
        || !notes.is_empty()
        || !reviews.is_empty()
        || !orient.asks.is_empty()
        || !orient.drop.is_empty()
        || manifest.post.is_some()
        || !manifest.close.is_empty()
        || lane.plan.kind != "research"
        || lane.error.is_some()
        || !manifest
            .surgeon(&lane.plan.surgeon)
            .is_some_and(|surgeon| surgeon.read_only)
        || !lane.structured
    {
        return None;
    }
    let output = lane.out.as_ref()?;
    if output.finding.trim().is_empty()
        || output.draft.is_some()
        || output.ask.is_some()
        || output.route.is_some()
    {
        return None;
    }
    let report = render_lane_report(lane);
    Some((
        SynthOut {
            facts: vec![report.clone()],
            log_line: format!(
                "Delivered the read-only report from lane {} without rewriting it.",
                lane.plan.id
            ),
            handoff: report,
            // The existing scratchpad retains standing constraints; the completed report is on the ledger.
            scratchpad: None,
            ..Default::default()
        },
        json!({"mode": "lane_report", "lane": lane.plan.id, "key": lane.key}),
    ))
}

fn prepare_stage_row(
    stage: &prompts::StagePlan,
    lanes: &[LaneResult],
    agent: &str,
    tick: u64,
    now: &str,
    review: Vec<Value>,
) -> Result<PreparedRow, &'static str> {
    let lane = lanes
        .iter()
        .find(|lane| lane.plan.id == stage.lane)
        .ok_or("source lane does not exist in this tick")?;
    let draft = lane
        .out
        .as_ref()
        .and_then(|out| out.draft.as_ref())
        .filter(|draft| !draft.body.trim().is_empty())
        .ok_or("source lane did not return a nonempty draft")?;
    if lane.error.is_some() {
        return Err("source lane did not return a nonempty draft");
    }
    if stage.target != draft.target || stage.kind != draft.kind {
        return Err("staged destination or kind differs from the source draft");
    }
    Ok(PreparedRow {
        id: format!("{agent}-t{tick}-{}", sanitize_name(&lane.plan.id)),
        tick,
        lane: lane.plan.id.clone(),
        surgeon: lane.plan.surgeon.clone(),
        target: stage.target.clone(),
        kind: draft.kind.clone(),
        body: stage.body.trim().to_string(),
        evidence: stage.evidence.clone(),
        status: "staged".into(),
        staged_at: now.to_string(),
        decided_at: None,
        decided_note: None,
        posted_tick: None,
        result: None,
        review,
    })
}

fn render_lane_report(r: &LaneResult) -> String {
    let head = format!(
        "LANE {} — surgeon {}, node {}, {:.0}s\n  item: {}\n  objective: {}",
        r.plan.id,
        if r.plan.surgeon.is_empty() {
            "general"
        } else {
            &r.plan.surgeon
        },
        r.model,
        r.secs,
        r.plan.item,
        r.plan.objective
    );
    match (&r.out, &r.error) {
        (_, Some(e)) => format!("{head}\n  FAILED: {e}"),
        (Some(o), None) => {
            let mut s = format!(
                "{head}\n  homework: {}\n  finding: {}\n  confidence: {}\n  evidence: {}\n  next_step: {}",
                o.homework.trim(),
                o.finding.trim(),
                o.confidence,
                if o.evidence.is_empty() { "(none)".to_string() } else { o.evidence.join(" | ") },
                o.next_step.trim()
            );
            if let Some(d) = &o.draft {
                s.push_str(&format!(
                    "\n  DRAFT ({} → {}):\n  \"\"\"\n{}\n  \"\"\"",
                    d.kind,
                    d.target,
                    d.body.trim()
                ));
            }
            if let Some(a) = &o.ask {
                s.push_str(&format!("\n  ASK: {a}"));
            }
            if let Some(rt) = &o.route {
                s.push_str(&format!("\n  ROUTE: {rt}"));
            }
            s
        }
        (None, None) => format!("{head}\n  (no output)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_delivery_preserves_the_report_and_keeps_decisions_on_synthesis() {
        let mut manifest: AgentManifest =
            serde_yaml::from_str(&AgentManifest::starter("demo")).unwrap();
        let orient = OrientOut::default();
        let raw = json!({"homework": "Fetched the assigned source", "finding": "Exact source finding <marker>",
            "confidence": 2, "evidence": ["https://example.com/source"], "next_step": "Exact title is unavailable; preserve this caveat."}).to_string();
        let mut lanes = vec![LaneResult {
            plan: serde_json::from_value(
                json!({"id": "source-read", "surgeon": "general", "kind": "research"}),
            )
            .unwrap(),
            key: "t1-source-read".into(),
            model: "test".into(),
            secs: 1.0,
            out: Some(serde_json::from_str(&raw).unwrap()),
            raw,
            error: None,
            structured: true,
        }];
        let (report, source) =
            direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).unwrap();
        assert_eq!(report.handoff, render_lane_report(&lanes[0]));
        assert_eq!(report.facts, vec![report.handoff.clone()]);
        assert!(report.handoff.contains("Exact source finding <marker>"));
        assert!(report
            .handoff
            .contains("Exact title is unavailable; preserve this caveat."));
        assert!(report.stage.is_empty() && report.asks.is_empty() && report.pending.is_empty());
        assert!(report.scratchpad.is_none());
        assert_eq!(
            source,
            json!({"mode": "lane_report", "lane": "source-read", "key": "t1-source-read"})
        );
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], false).is_none());
        assert!(direct_read_only_report(
            &manifest,
            &orient,
            &lanes,
            &[],
            &["new decision".into()],
            true
        )
        .is_none());
        lanes[0].structured = false;
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).is_none());
        lanes[0].structured = true;
        lanes[0].out.as_mut().unwrap().draft = Some(Draft {
            target: "DEMO-1".into(),
            kind: "comment".into(),
            body: "Review this".into(),
        });
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).is_none());
        lanes[0].out.as_mut().unwrap().draft = None;
        lanes[0].out.as_mut().unwrap().ask = Some("Need human decision".into());
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).is_none());
        lanes[0].out.as_mut().unwrap().ask = None;
        manifest.surgeons[0].read_only = false;
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).is_none());
        manifest.surgeons[0].read_only = true;
        lanes[0].error = Some("transport failed".into());
        assert!(direct_read_only_report(&manifest, &orient, &lanes, &[], &[], true).is_none());
    }

    #[test]
    fn staging_requires_this_ticks_draft_and_preserves_its_destination() {
        let mut lanes = vec![LaneResult {
            plan: serde_json::from_value(json!({"id": "page-read", "surgeon": "researcher"}))
                .unwrap(),
            key: "t1-page-read".into(),
            model: "test".into(),
            secs: 0.0,
            out: Some(LaneOut::default()),
            raw: String::new(),
            error: None,
            structured: true,
        }];
        let mut stage: prompts::StagePlan = serde_json::from_value(json!({
            "lane": "invented", "target": "DEMO-1", "kind": "comment", "body": "Reviewed text"
        }))
        .unwrap();
        assert_eq!(
            prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).unwrap_err(),
            "source lane does not exist in this tick"
        );
        stage.lane = "page-read".into();
        assert_eq!(
            prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).unwrap_err(),
            "source lane did not return a nonempty draft"
        );
        lanes[0].out.as_mut().unwrap().draft = Some(Draft {
            target: "DEMO-1".into(),
            kind: "comment".into(),
            body: "   ".into(),
        });
        assert!(prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).is_err());
        lanes[0].out.as_mut().unwrap().draft.as_mut().unwrap().body = "Original text".into();
        stage.target = "DEMO-2".into();
        assert_eq!(
            prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).unwrap_err(),
            "staged destination or kind differs from the source draft"
        );
        stage.target = "DEMO-1".into();
        stage.kind = "create".into();
        assert!(prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).is_err());
        stage.kind = "comment".into();
        let review = vec![json!({"verdict": "pass"})];
        let row = prepare_stage_row(&stage, &lanes, "demo", 1, "now", review.clone()).unwrap();
        assert_eq!(
            (
                row.lane.as_str(),
                row.surgeon.as_str(),
                row.target.as_str(),
                row.kind.as_str(),
                row.body.as_str()
            ),
            (
                "page-read",
                "researcher",
                "DEMO-1",
                "comment",
                "Reviewed text"
            )
        );
        assert_eq!(row.review, review);
        lanes[0].error = Some("failed after draft".into());
        assert!(prepare_stage_row(&stage, &lanes, "demo", 1, "now", vec![]).is_err());
    }

    #[test]
    fn a_dead_node_reads_as_a_transport_error_never_as_an_answer() {
        let dead = "Network error: Could not connect to 127.0.0.1:8090 — check your network connection and try again.\n\nPlease resend your message to try again.";
        assert!(transport_error(dead).unwrap().contains("127.0.0.1:8090"));
        assert!(transport_error("{\"summary\": \"two lanes\", \"lanes\": []}").is_none());
        assert!(
            transport_error("the ticket says: please resend your message to try again later")
                .is_none()
        );
    }
}
