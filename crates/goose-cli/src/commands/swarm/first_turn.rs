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

impl GooseAgentDispatcher {
    /// Dispatch side: say what the first turn holds and park the rest for the lane's loop.
    pub(super) fn stash_brief_rest(
        &self,
        work_dir: &Path,
        task: &str,
        file: &str,
        first_chars: usize,
        rest: String,
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
            .insert(rest_key(work_dir, task), rest);
    }

    /// Loop side, at the boundary after the first write landed: the rest, once, with its event
    /// (`turn` = the tool calls landed so far — 1 when the write was the lane's first action).
    pub(super) fn deliver_brief_rest(
        &self,
        work_dir: &Path,
        task: &str,
        calls: usize,
    ) -> Option<String> {
        let rest = self
            .brief_rest
            .lock()
            .unwrap()
            .remove(&rest_key(work_dir, task))?;
        self.events.write_value(serde_json::json!({
            "event": "brief_rest_delivered",
            "task": task,
            "chars": rest.chars().count(),
            "turn": calls,
        }));
        Some(rest)
    }

    /// After the call: a rest the lane never earned (no write landed) is SAID, never dropped.
    pub(super) fn settle_brief_rest(&self, work_dir: &Path, task: &str) {
        let left = self
            .brief_rest
            .lock()
            .unwrap()
            .remove(&rest_key(work_dir, task));
        if let Some(rest) = left {
            self.events.write_value(serde_json::json!({
                "event": "brief_rest_undelivered",
                "task": task,
                "chars": rest.chars().count(),
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
