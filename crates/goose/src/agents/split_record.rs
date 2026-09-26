//! THE SPLIT'S RECORD OF A FAILED TURN, SAVED WITH THE TURN (Q-121, Q-122).
//!
//! A tensor split supervised by this goosed keeps its events in memory only (`EVENT_LIMIT`, gone
//! when goosed restarts). E2E #3 (installed 3.0.47, 2026-09-26 10:07): the hang rule stopped the
//! split mid-answer; live, the chat read the split's events and said "The split across your Macs
//! stopped — the Macs stopped making progress". After the app relaunched the same history said
//! "No model is mounted … nothing answers at http://127.0.0.1:8090" five times, and the cut answer
//! itself only ever said "Network error: Stream decode error … no [DONE] after 9771 data frames",
//! because the event list the notice was derived from was empty ("Supervisor events 0").
//!
//! So when a turn fails, the supervisor's own events are written INTO the failure text, on one
//! line between the error and the closing sentence — the closer stays LAST, which the swarm's
//! error-text readers (`provider_failures.rs`) key on. The desktop strips the line and derives
//! the notice from it with the SAME rule it applies to live events (`splitStop.ts`); nothing here
//! decides what stopped the split. Facts only: the events since the split last became `ready`,
//! the state it was in when the turn failed, and when the failed provider call began.
//!
//! It is written whenever this goosed's split has served, whatever its state: a rank that dies on
//! its own breaks the stream up to one supervisor poll BEFORE the supervisor writes `rankDied`
//! (E2E #2), so at the failure the split can still read `serving` with no ending event yet. The
//! record then carries the turn's start, and the desktop reads the stop from the live events that
//! follow — or, when none are known (a relaunch), says what the error says, never a guessed cause.
//! A goosed that never ran a split (every swarm worker, the CLI) writes nothing.

use serde_json::json;

/// The line's prefix. Mirrored by `ui/desktop/src/components/chatServedBy/splitRecord.ts`.
pub const SPLIT_RECORD_MARKER: &str = "Split supervisor record: ";

/// The failure text a failed turn saves: the error, then the split's record when there is one,
/// then the closing sentence — always last.
pub fn failed_turn_text(error: &str, record: Option<&str>, closer: &str) -> String {
    match record {
        Some(record) => format!("{error}\n\n{record}\n\n{closer}"),
        None => format!("{error}\n\n{closer}"),
    }
}

/// The record line for a turn whose provider call began at `turn_started_ms` (ms since the
/// epoch), read from the split THIS goosed supervises.
#[cfg(unix)]
pub fn split_record_now(turn_started_ms: i64) -> Option<String> {
    split_record(
        &goose_sidecar::distributed::global_manager().status(),
        turn_started_ms,
    )
}

/// `goose_sidecar::distributed` compiles only on Unix: no goosed here supervises a split.
#[cfg(not(unix))]
pub fn split_record_now(_turn_started_ms: i64) -> Option<String> {
    None
}

#[cfg(unix)]
fn split_record(
    status: &goose_sidecar::distributed::DistributedStatus,
    turn_started_ms: i64,
) -> Option<String> {
    let nodes: Vec<&str> = status.nodes.iter().map(|n| n.name.as_str()).collect();
    let config_nodes: Option<Vec<&str>> = status
        .config
        .as_ref()
        .map(|c| c.nodes.iter().map(|n| n.name.as_str()).collect());
    record_line(
        status.state,
        &status.events,
        status.model_id.as_deref(),
        &nodes,
        config_nodes.as_deref(),
        turn_started_ms,
    )
}

#[cfg(unix)]
fn record_line(
    state: goose_sidecar::distributed::RunState,
    events: &[goose_sidecar::distributed::EngineEvent],
    model_id: Option<&str>,
    nodes: &[&str],
    config_nodes: Option<&[&str]>,
    turn_started_ms: i64,
) -> Option<String> {
    use goose_sidecar::distributed::EventKind;
    let ready = events.iter().rposition(|e| e.kind == EventKind::Ready)?;
    let events: Vec<_> = events[ready..]
        .iter()
        .map(|e| {
            json!({
                "atMs": e.at_ms,
                "kind": e.kind.as_str(),
                "node": e.node,
                "message": e.message,
            })
        })
        .collect();
    let record = json!({
        "v": 1,
        "state": state.as_str(),
        "turnStartedMs": turn_started_ms,
        "modelId": model_id,
        "nodes": nodes,
        "configNodes": config_nodes,
        "events": events,
    });
    Some(format!("{SPLIT_RECORD_MARKER}{record}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use goose_sidecar::distributed::{EngineEvent, EventKind, RunState};

    const STUDIO: &str = "Work’s Mac Studio";
    const MACBOOK: &str = "Mihai Macbook";
    const READY_MS: u64 = 1_790_000_000_000;
    const TURN_MS: i64 = 1_790_000_600_000;
    const HANG_MS: u64 = 1_790_001_200_000;
    const HANG_WORDS: &str =
        "no progress for 20160 ms (10 × the 2016 ms median)\nrank ps stats [S, S]";

    fn event(at_ms: u64, kind: EventKind, message: &str) -> EngineEvent {
        EngineEvent {
            at_ms,
            kind,
            node: None,
            message: message.to_string(),
        }
    }

    /// E2E #3, 10:07: the hang rule stopped the split under a streaming answer.
    fn hung_events() -> Vec<EngineEvent> {
        vec![
            event(READY_MS - 7_000, EventKind::Launched, "2 ranks over jaccl"),
            event(
                READY_MS,
                EventKind::Ready,
                "the readiness completion ended with [DONE]",
            ),
            event(HANG_MS, EventKind::Hang, HANG_WORDS),
            event(HANG_MS + 100, EventKind::Stopped, "after hang: verified"),
        ]
    }

    fn line(state: RunState, events: &[EngineEvent]) -> Option<String> {
        record_line(
            state,
            events,
            Some("Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx"),
            &[MACBOOK, STUDIO],
            None,
            TURN_MS,
        )
    }

    #[test]
    fn a_stopped_split_is_recorded_from_its_last_ready_with_the_turns_start() {
        let line = line(RunState::Failed, &hung_events()).expect("the split had stopped");
        assert!(
            !line.contains('\n'),
            "one line, the supervisor's newlines escaped"
        );
        let record: serde_json::Value =
            serde_json::from_str(line.strip_prefix(SPLIT_RECORD_MARKER).unwrap()).unwrap();
        assert_eq!(record["state"], "failed");
        assert_eq!(record["turnStartedMs"], TURN_MS);
        assert_eq!(record["nodes"], json!([MACBOOK, STUDIO]));
        assert_eq!(record["configNodes"], serde_json::Value::Null);
        let kinds: Vec<_> = record["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            kinds,
            ["ready", "hang", "stopped"],
            "launched is before the ready"
        );
        assert_eq!(record["events"][1]["message"], HANG_WORDS);
        assert_eq!(record["events"][1]["atMs"], HANG_MS);
    }

    /// E2E #2's shape: the rank died and cut the stream before the supervisor's poll wrote it —
    /// the split still reads serving, and the record still says the turn ran on it.
    #[test]
    fn a_split_still_serving_at_the_cut_is_recorded_with_the_turns_start() {
        let serving = &hung_events()[..2];
        let record: serde_json::Value = serde_json::from_str(
            line(RunState::Serving, serving)
                .unwrap()
                .strip_prefix(SPLIT_RECORD_MARKER)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["state"], "serving");
        assert_eq!(record["turnStartedMs"], TURN_MS);
        assert_eq!(record["events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_split_that_never_served_writes_no_record() {
        let mut never_ready = hung_events();
        never_ready.retain(|e| e.kind != EventKind::Ready);
        assert_eq!(line(RunState::Failed, &never_ready), None);
        assert_eq!(line(RunState::Stopped, &[]), None);
    }

    #[test]
    fn a_goosed_that_never_ran_a_split_writes_no_record() {
        let idle = goose_sidecar::distributed::DistributedManager::default().status();
        assert_eq!(split_record(&idle, TURN_MS), None);
    }

    #[test]
    fn the_closer_stays_last_so_the_swarms_error_readers_still_match() {
        let record = line(RunState::Failed, &hung_events());
        let text = failed_turn_text(
            "Network error: Stream decode error: stream ended before completion",
            record.as_deref(),
            "Please resend your message to try again.",
        );
        assert!(text.ends_with("\n\nPlease resend your message to try again."));
        assert!(text.contains(&format!("\n\n{SPLIT_RECORD_MARKER}{{")));
        assert_eq!(
            failed_turn_text("Ran into this error: x.", None, "Please retry."),
            "Ran into this error: x.\n\nPlease retry."
        );
    }
}
