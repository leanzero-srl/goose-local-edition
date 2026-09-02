//! VA-144 — THE WRITE-ALONE FIRST TURN (r6j ticks 24/25, read from the words): the write-first
//! opening (VA-102) was NOT the first action on any lane — the skeleton wrote at 16 minutes after
//! three spec reads; web-page and scene-stream made 0 calls at 17 minutes with 36k/34k reasoning
//! chars over 40–78k-char briefs. The stateless-models doctrine, as VA-128 applied it to research:
//! the harness forms the message, one thing per turn. So a build lane's FIRST message is the
//! WRITE ALONE — the rules it needs to make the call, the file manifest, the brief that names the
//! file and its interface — and the REST (the dependency sources, the completed-dependency
//! context, pitfalls, notes, pillars, what earlier builds learned) arrives on the SECOND turn:
//! the lane's own loop delivers it through `Agent::steer` the moment its first write lands
//! (`drain_brief_rest!` in swarm.rs, the seam E7's research relay already uses — a steer lands at
//! the next turn boundary and a landed tool call IS one). ONE arm, one `if`: `write_alone` false
//! puts every block back in the system prompt byte for byte.
//!
//! r6k measures minutes-to-first-write per lane against r6j's ≥ 16 / 17 / 17.

use std::collections::HashMap;
use std::path::Path;

use super::{swarm_gate_cfg, write_first_on, DispatchRequest, EventSink, GooseAgentDispatcher};

/// The lever, on with the write-first mold it sharpens; `GOOSE_SWARM_FIRST_TURN=0` reverts.
pub(super) fn first_turn_on() -> bool {
    write_first_on() && swarm_gate_cfg("GOOSE_SWARM_FIRST_TURN", true)
}

/// THE ONE `if`: a first-attempt file author that is not the join, not a repair shard, not the
/// merger and not a speculative twin. Every other dispatch keeps today's single-turn prompt.
pub(super) fn write_alone(req: &DispatchRequest, repairing: bool, is_sink: bool) -> bool {
    first_turn_on()
        && !repairing
        && !is_sink
        && req.merger_of.is_none()
        && req.attempt == 0
        && !req.speculative
        && !req.owned_files.is_empty()
}

/// Where the fact blocks go: `(before the layout, after the layout)` in the system prompt when
/// the arm is off — the pre-VA-144 order, byte for byte — and both empty with the blocks
/// concatenated for the second turn when it is on.
pub(super) struct Placed {
    pub(super) pre_layout: String,
    pub(super) post_layout: String,
    pub(super) rest: String,
}

pub(super) fn place(
    write_alone: bool,
    pitfalls: &str,
    notes: &str,
    pillars: &str,
    deps: &str,
    context: &str,
) -> Placed {
    let pre = format!("{pitfalls}{notes}{pillars}");
    let post = format!("{deps}{context}");
    if write_alone {
        Placed {
            pre_layout: String::new(),
            post_layout: String::new(),
            rest: format!("{pre}{post}"),
        }
    } else {
        Placed {
            pre_layout: pre,
            post_layout: post,
            rest: String::new(),
        }
    }
}

/// The second turn's text: what just happened, what this is, then the blocks and the LEARNED
/// block — every clause a fact of this dispatch (the file, the task), never a template.
pub(super) fn rest_text(task: &str, file: &str, blocks: &str, learned: &str) -> String {
    let learned_part = if learned.is_empty() {
        String::new()
    } else {
        format!("\n\n{learned}")
    };
    format!(
        "YOUR FIRST WRITE LANDED (`{file}`). The rest of task `{task}`'s brief follows — the \
         dependency sources under 'API of …', the context from completed dependencies, the \
         pitfalls, the notes, the pillars and what earlier builds learned. Read what `{file}` and \
         your next owned file need from it, then continue with the next `write` or `edit`; do \
         not re-read `{file}` to check it against this — edit it where a fact below contradicts \
         it.\n\n{blocks}{learned_part}"
    )
}

/// The event at dispatch: what the lane's first turn holds and how much waits for its write.
pub(super) fn first_turn_event(
    task: &str,
    file: &str,
    first_chars: usize,
    rest_chars: usize,
) -> serde_json::Value {
    serde_json::json!({
        "event": "brief_first_turn",
        "task": task,
        "chars": first_chars,
        "file": file,
        "rest_chars": rest_chars,
    })
}

/// Did this landed tool call WRITE a file? The developer extension's `write` (`developer__write`)
/// or the older `text_editor` with a `write` command — read from the call's name and the
/// engine's own argument summary (`summarize_tool_call`: "write <path>").
pub(super) fn is_write_call(name: &str, summary: &str) -> bool {
    name.rsplit("__").next() == Some("write")
        || (name.ends_with("text_editor") && summary.starts_with("write "))
}

/// The stash key: the lane's working tree AND its task, so a speculative twin's shadow never
/// takes the real lane's rest (both carry the task id as their activity key).
pub(super) fn rest_key(work_dir: &Path, task: &str) -> String {
    format!("{}\n{task}", work_dir.display())
}

/// One lane's rest: parked text until it lands, then the turn it landed on.
#[derive(Debug)]
enum Rest {
    Parked(String),
    Delivered { turn: usize },
}

/// A parked rest and the dispatch-seam events that describe its blocks (VA-148:
/// `pitfalls_delivered`, `user_notes_delivered`, `shard_siblings_delivered` / `_none`) — facts
/// that are true only once the rest has reached the model, so they wait for it.
#[derive(Debug)]
struct BriefRest {
    rest: Rest,
    events: Vec<serde_json::Value>,
}

/// What a judge look may assert about the lane's brief (GEN-4): nothing parked (the arm is off,
/// or the lane is not a write-alone author), the rest still parked, or the rest landed at a turn.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum RestState {
    NotParked,
    Parked { chars: usize },
    Delivered { turn: usize },
}

/// The dispatcher's stash, pure so the delivery order and the judge's reading of it are pinned
/// without a session: every method returns the events to write, the dispatcher writes them.
#[derive(Debug, Default)]
pub(super) struct BriefRestStash {
    lanes: HashMap<String, BriefRest>,
}

impl BriefRestStash {
    pub(super) fn park(&mut self, key: String, text: String, events: Vec<serde_json::Value>) {
        self.lanes.insert(
            key,
            BriefRest {
                rest: Rest::Parked(text),
                events,
            },
        );
    }

    /// The rest, once: `brief_rest_delivered{task, chars, turn}` first, then every parked event
    /// stamped with the same `turn` — a block is said to be delivered at the moment it is.
    pub(super) fn deliver(
        &mut self,
        key: &str,
        task: &str,
        turn: usize,
    ) -> Option<(String, Vec<serde_json::Value>)> {
        let lane = self.lanes.get_mut(key)?;
        let text = match std::mem::replace(&mut lane.rest, Rest::Delivered { turn }) {
            Rest::Parked(text) => text,
            already @ Rest::Delivered { .. } => {
                lane.rest = already;
                return None;
            }
        };
        let mut events = vec![serde_json::json!({
            "event": "brief_rest_delivered",
            "task": task,
            "chars": text.chars().count(),
            "turn": turn,
        })];
        for mut ev in std::mem::take(&mut lane.events) {
            ev["turn"] = serde_json::json!(turn);
            events.push(ev);
        }
        Some((text, events))
    }

    pub(super) fn state(&self, key: &str) -> RestState {
        match self.lanes.get(key) {
            None => RestState::NotParked,
            Some(BriefRest {
                rest: Rest::Parked(text),
                ..
            }) => RestState::Parked {
                chars: text.chars().count(),
            },
            Some(BriefRest {
                rest: Rest::Delivered { turn },
                ..
            }) => RestState::Delivered { turn: *turn },
        }
    }

    /// After the call the lane's entry goes. A rest that never landed is
    /// `brief_rest_undelivered{task, chars, withheld}`: `withheld` carries the parked events
    /// verbatim, so the facts they held (pitfall chars, note ids, sibling names) stay on the log
    /// under the one name that says they never reached the model — never as delivered.
    pub(super) fn settle(&mut self, key: &str, task: &str) -> Option<serde_json::Value> {
        let lane = self.lanes.remove(key)?;
        match lane.rest {
            Rest::Delivered { .. } => None,
            Rest::Parked(text) => Some(serde_json::json!({
                "event": "brief_rest_undelivered",
                "task": task,
                "chars": text.chars().count(),
                "withheld": lane.events,
            })),
        }
    }
}

/// The judge's fact block about the lane's brief (GEN-4: assert only what was delivered): empty
/// when nothing is parked, so the prompt reads exactly as before the arm; during the first turn
/// the blocks are named as NOT YET DELIVERED; afterwards the turn they landed on.
pub(super) fn rest_state_block(state: &RestState) -> String {
    match state {
        RestState::NotParked => String::new(),
        RestState::Parked { chars } => format!(
            "\n\nWHAT THIS CALL HAS BEEN HANDED SO FAR: its first message is the write alone — the \
             task, the file manifest and the reading rules. The rest of its brief ({chars} chars: \
             the dependency sources under 'API of …', the context from completed dependencies, \
             the pitfalls, the notes, the pillars) is NOT YET DELIVERED; it lands as its next user \
             message the moment its first `write` lands. Do not tell it to use an API excerpt, a \
             pitfall or a note it has not received, and do not read a first write made without \
             them as a defect — if it is stalled before that write, NEXT is the write."
        ),
        RestState::Delivered { turn } => format!(
            "\n\nWHAT THIS CALL HAS BEEN HANDED: its first message was the write alone; the rest \
             of its brief (the dependency sources under 'API of …', the context from completed \
             dependencies, the pitfalls, the notes, the pillars) was delivered as a user message \
             at turn {turn}, after its first write landed."
        ),
    }
}

impl GooseAgentDispatcher {
    /// Dispatch side: an event about a block that rides the rest waits with it under the
    /// write-alone arm and fires at delivery; with the arm off it fires here, as it always did.
    pub(super) fn say_at_delivery(
        &self,
        write_alone: bool,
        rest_events: &mut Vec<serde_json::Value>,
        ev: serde_json::Value,
    ) {
        if write_alone {
            rest_events.push(ev);
        } else {
            self.events.write_value(ev);
        }
    }

    /// Dispatch side: say what the first turn holds and park the rest, with the events that
    /// describe it, for the lane's loop.
    pub(super) fn stash_brief_rest(
        &self,
        work_dir: &Path,
        task: &str,
        file: &str,
        first_chars: usize,
        rest: String,
        rest_events: Vec<serde_json::Value>,
    ) {
        self.events.write_value(first_turn_event(
            task,
            file,
            first_chars,
            rest.chars().count(),
        ));
        self.brief_rest
            .lock()
            .unwrap()
            .park(rest_key(work_dir, task), rest, rest_events);
    }

    /// Loop side, at the boundary after the first write landed: the rest, once, with its event
    /// (`turn` = the tool calls landed so far — 1 when the write was the lane's first action)
    /// and the parked block events behind it.
    pub(super) fn deliver_brief_rest(
        &self,
        work_dir: &Path,
        task: &str,
        calls: usize,
    ) -> Option<String> {
        let (rest, events) =
            self.brief_rest
                .lock()
                .unwrap()
                .deliver(&rest_key(work_dir, task), task, calls)?;
        for ev in events {
            self.events.write_value(ev);
        }
        Some(rest)
    }

    /// Look side: what the judge may assert about this lane's brief.
    pub(super) fn brief_rest_state(&self, work_dir: &Path, task: &str) -> RestState {
        self.brief_rest
            .lock()
            .unwrap()
            .state(&rest_key(work_dir, task))
    }

    /// After the call: a rest the lane never earned (no write landed) is SAID, never dropped.
    pub(super) fn settle_brief_rest(&self, work_dir: &Path, task: &str) {
        let undelivered = self
            .brief_rest
            .lock()
            .unwrap()
            .settle(&rest_key(work_dir, task), task);
        if let Some(ev) = undelivered {
            self.events.write_value(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// OFF: the five blocks sit where they always did — pitfalls, notes, pillars before the
    /// layout, deps and context after it — and nothing waits. ON: the system prompt carries none
    /// of them and the second turn carries all five in that order.
    #[test]
    fn the_arm_moves_every_fact_block_to_the_second_turn_or_none() {
        let off = place(false, "P|", "N|", "L|", "D|", "C|");
        assert_eq!(off.pre_layout, "P|N|L|");
        assert_eq!(off.post_layout, "D|C|");
        assert_eq!(off.rest, "");
        let on = place(true, "P|", "N|", "L|", "D|", "C|");
        assert_eq!(on.pre_layout, "");
        assert_eq!(on.post_layout, "");
        assert_eq!(on.rest, "P|N|L|D|C|");
    }

    /// The second turn names the file that landed and the task, carries the blocks and the
    /// LEARNED block after them, and never a template line when nothing was learned.
    #[test]
    fn the_rest_opens_with_the_landed_file_and_carries_the_blocks() {
        let text = rest_text("web-page", "web/index.html", "## API of app/db.py\n…", "");
        assert!(text.starts_with("YOUR FIRST WRITE LANDED (`web/index.html`). The rest of task `web-page`'s brief follows"));
        assert!(text.ends_with("## API of app/db.py\n…"));
        assert!(!text.contains("LEARNED"));
        let learned = rest_text("t", "f.py", "B", "LEARNED FROM EARLIER BUILDS (x)\n- y");
        assert!(learned.ends_with("B\n\nLEARNED FROM EARLIER BUILDS (x)\n- y"));
    }

    #[test]
    fn a_write_is_recognised_by_its_tool_name_or_the_editors_write_command() {
        assert!(is_write_call("developer__write", "web/index.html"));
        assert!(is_write_call("write", "web/index.html"));
        assert!(is_write_call("developer__text_editor", "write src/main.py"));
        assert!(!is_write_call("developer__text_editor", "view src/main.py"));
        assert!(!is_write_call("developer__shell", "cat web/index.html"));
        assert!(!is_write_call("developer__edit", "web/index.html"));
    }

    #[test]
    fn the_stash_key_separates_a_twins_shadow_from_the_real_tree() {
        let real = rest_key(Path::new("/w/real"), "web-page");
        let twin = rest_key(Path::new("/tmp/shadow"), "web-page");
        assert_ne!(real, twin);
        assert_eq!(real, rest_key(Path::new("/w/real"), "web-page"));
    }

    #[test]
    fn the_first_turn_event_carries_the_file_and_both_sizes() {
        let ev = first_turn_event("skeleton", "app/__main__.py", 12_000, 30_000);
        assert_eq!(ev["event"], "brief_first_turn");
        assert_eq!(ev["task"], "skeleton");
        assert_eq!(ev["file"], "app/__main__.py");
        assert_eq!(ev["chars"], 12_000);
        assert_eq!(ev["rest_chars"], 30_000);
    }

    /// THE CONSUMER (VA-148, GEN-4): the judge's block reads the rest as NOT YET DELIVERED while it
    /// is parked and as delivered at its turn afterwards, and the parked dispatch-seam events
    /// (`pitfalls_delivered`, `shard_siblings_none`) fire only behind `brief_rest_delivered`,
    /// stamped with the same turn — never at dispatch.
    #[test]
    fn the_judge_reads_the_rest_as_undelivered_until_it_lands_and_the_parked_events_fire_then() {
        let mut stash = BriefRestStash::default();
        let key = rest_key(Path::new("/w/real"), "web-page");
        assert_eq!(stash.state(&key), RestState::NotParked);
        assert_eq!(rest_state_block(&stash.state(&key)), "");
        stash.park(
            key.clone(),
            "## API of app/db.py\n…".to_string(),
            vec![
                json!({"event": "pitfalls_delivered", "task_id": "web-page", "delivered": true, "chars": 812}),
                json!({"event": "shard_siblings_none", "task_id": "web-page", "module": "web/viz.js", "shard": "camera", "pending": ["labels"]}),
            ],
        );
        assert_eq!(stash.state(&key), RestState::Parked { chars: 21 });
        let parked = rest_state_block(&stash.state(&key));
        assert!(parked.contains("NOT YET DELIVERED"), "{parked}");
        assert!(parked.contains("(21 chars:"), "{parked}");
        assert!(parked.contains(
            "Do not tell it to use an API excerpt, a pitfall or a note it has not received"
        ));
        assert!(!parked.contains("was delivered"));
        let (text, events) = stash.deliver(&key, "web-page", 1).unwrap();
        assert_eq!(text, "## API of app/db.py\n…");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["event"], "brief_rest_delivered");
        assert_eq!(events[0]["task"], "web-page");
        assert_eq!(events[0]["chars"], 21);
        assert_eq!(events[0]["turn"], 1);
        assert_eq!(events[1]["event"], "pitfalls_delivered");
        assert_eq!(events[1]["delivered"], true);
        assert_eq!(events[1]["chars"], 812);
        assert_eq!(events[1]["turn"], 1);
        assert_eq!(events[2]["event"], "shard_siblings_none");
        assert_eq!(events[2]["pending"][0], "labels");
        assert_eq!(events[2]["turn"], 1);
        assert_eq!(stash.state(&key), RestState::Delivered { turn: 1 });
        let landed = rest_state_block(&stash.state(&key));
        assert!(
            landed.contains("was delivered as a user message at turn 1"),
            "{landed}"
        );
        assert!(!landed.contains("NOT YET DELIVERED"));
        assert!(
            stash.deliver(&key, "web-page", 2).is_none(),
            "the rest lands once"
        );
        assert!(
            stash.settle(&key, "web-page").is_none(),
            "a landed rest withholds nothing"
        );
        assert_eq!(stash.state(&key), RestState::NotParked);
    }

    /// A rest that never lands (no write) is said with the parked events it withheld, verbatim,
    /// under `brief_rest_undelivered` — the facts stay on the log, never as delivered.
    #[test]
    fn a_rest_that_never_lands_is_said_with_the_events_it_withheld() {
        let mut stash = BriefRestStash::default();
        let key = rest_key(Path::new("/w/real"), "skeleton");
        stash.park(
            key.clone(),
            "P|N|L|D|C|".to_string(),
            vec![json!({"event": "user_notes_delivered", "task_id": "skeleton", "notes": ["1756771200000-note.md"], "count": 1})],
        );
        let ev = stash.settle(&key, "skeleton").unwrap();
        assert_eq!(ev["event"], "brief_rest_undelivered");
        assert_eq!(ev["task"], "skeleton");
        assert_eq!(ev["chars"], 10);
        assert_eq!(ev["withheld"][0]["event"], "user_notes_delivered");
        assert_eq!(ev["withheld"][0]["count"], 1);
        assert_eq!(ev["withheld"][0].get("turn"), None);
        assert_eq!(stash.state(&key), RestState::NotParked);
        assert!(stash.settle(&key, "skeleton").is_none());
    }
}
