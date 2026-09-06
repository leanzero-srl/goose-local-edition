//! The four prompts of a tick and the shapes they return. Every prompt is assembled from THIS
//! tick's facts (the charter, the poll output, the ledger block, the human's notes) — never a
//! template about work in general — and every output is a HANDOFF: exact identifiers, the
//! concrete next step, a confidence the reviewer can hold it to (gate 2).

use serde::Deserialize;
use serde_json::{json, Value};

use super::manifest::{AgentManifest, Surgeon};

// ---------------------------------------------------------------- ORIENT

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OrientOut {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub lanes: Vec<LanePlan>,
    #[serde(default)]
    pub asks: Vec<AskPlan>,
    #[serde(default)]
    pub drop: Vec<DropPlan>,
    #[serde(default)]
    pub scratchpad: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LanePlan {
    pub id: String,
    #[serde(default)]
    pub surgeon: String,
    #[serde(default)]
    pub item: String,
    #[serde(default)]
    pub objective: String,
    /// research | draft | verify
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AskPlan {
    pub question: String,
    #[serde(default)]
    pub why: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DropPlan {
    pub item: String,
    #[serde(default)]
    pub why: String,
}

pub fn orient_schema() -> Value {
    json!({
        "type": "object",
        "required": ["summary", "lanes", "asks", "drop"],
        "properties": {
            "summary": {"type": "string"},
            "lanes": {"type": "array", "items": {
                "type": "object",
                "required": ["id", "surgeon", "item", "objective", "kind"],
                "properties": {
                    "id": {"type": "string"},
                    "surgeon": {"type": "string"},
                    "item": {"type": "string"},
                    "objective": {"type": "string"},
                    "kind": {"type": "string"}
                }
            }},
            "asks": {"type": "array", "items": {
                "type": "object", "required": ["question", "why"],
                "properties": {"question": {"type": "string"}, "why": {"type": "string"}}
            }},
            "drop": {"type": "array", "items": {
                "type": "object", "required": ["item", "why"],
                "properties": {"item": {"type": "string"}, "why": {"type": "string"}}
            }},
            "scratchpad": {"type": "string"}
        }
    })
}

pub fn orient_system(m: &AgentManifest, tick: u64) -> String {
    let roster = if m.surgeons.is_empty() {
        "  (no surgeons declared — every lane runs on the general charter)".to_string()
    } else {
        m.surgeons
            .iter()
            .map(|s| {
                format!(
                    "  - {}{}{}",
                    s.name,
                    if s.match_words.is_empty() {
                        String::new()
                    } else {
                        format!(" (routes on: {})", s.match_words.join(", "))
                    },
                    if s.read_only {
                        " — read-only"
                    } else {
                        " — may write files"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "You are the ORCHESTRATOR of the desk `{name}` ({title}), tick #{tick}. You do not do the \
         work yourself: you read what this tick polled, what the ledger already knows and what the \
         human said, and you decide the LANES — one per item that needs a surgeon — then hand each \
         lane a sharp objective. Every lane costs a node's time, so a lane exists only when its \
         result will be consumed: a draft the desk will post, a fact the ledger lacks, a verification \
         a staged draft needs. Restating what the poll already says is not a lane.\n\n\
         Surgeons available (route by the item, the match words are hints):\n{roster}\n\n\
         RULES OF THE DESK (the charter, verbatim below, is binding). Never plan a post, a config \
         change or any write — a surgeon returns a DRAFT, the review attacks it, the human or the \
         next tick posts it through the desk's one write path. When an item needs a decision only \
         the human can make, raise an ASK with the exact question and why, and do not lane it. An \
         item already handled on the ledger (a staged or posted draft, an open ask) is DROPPED with \
         that reason, not re-laned.\n\n\
         Output JSON only: {{summary, lanes:[{{id, surgeon, item, objective, kind}}], asks:[{{question, why}}], \
         drop:[{{item, why}}], scratchpad}}. `id` is a short kebab-case handle unique in this tick \
         (e.g. `ithub-4821-access`). `item` names the exact object (ticket key, thread URL, page \
         id). `objective` states what the surgeon must find or draft and the concrete next step. \
         `kind` is research | draft | verify. `scratchpad` is the desk's short running note for the \
         NEXT tick (goal, in flight, facts) — rewrite it whole.",
        name = m.name,
        title = m.display_title(),
    )
}

pub fn orient_user(
    charter: &str,
    scratchpad: &str,
    ledger_block: &str,
    notes: &[String],
    poll_text: &str,
    guard_notes: &[String],
) -> String {
    let notes_block = if notes.is_empty() {
        "(none this tick)".to_string()
    } else {
        notes
            .iter()
            .map(|n| format!("- {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let guards = if guard_notes.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nGUARD NOTES (scripts that warned this tick):\n{}",
            guard_notes.join("\n")
        )
    };
    format!(
        "THE CHARTER:\n{charter}\n\n---\nSCRATCHPAD (your own note from the previous tick):\n{scratch}\n\n---\nTHE LEDGER (snowballed across ticks):\n{ledger}\n\n---\nNOTES FROM THE HUMAN since the last tick:\n{notes_block}{guards}\n\n---\nTHE POLL — what the desk's read-only scripts returned this tick (this is the inbox; identifiers in it are the ones to use):\n{poll}\n\n---\nDecide the lanes now. JSON only.",
        scratch = if scratchpad.trim().is_empty() { "(empty)" } else { scratchpad.trim() },
        ledger = ledger_block.trim(),
        poll = if poll_text.trim().is_empty() {
            "(the poll returned nothing — say so in the summary and plan no lanes unless a note or an answered ask needs one)"
        } else {
            poll_text.trim()
        },
    )
}

// ---------------------------------------------------------------- LANE

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LaneOut {
    #[serde(default)]
    pub homework: String,
    #[serde(default)]
    pub finding: String,
    #[serde(default)]
    pub draft: Option<Draft>,
    #[serde(default)]
    pub ask: Option<String>,
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub confidence: u8,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub next_step: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Draft {
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub body: String,
}

pub fn lane_schema() -> Value {
    json!({
        "type": "object",
        "required": ["homework", "finding", "confidence", "evidence", "next_step"],
        "properties": {
            "homework": {"type": "string"},
            "finding": {"type": "string"},
            "draft": {"type": "object", "properties": {
                "target": {"type": "string"}, "kind": {"type": "string"}, "body": {"type": "string"}
            }},
            "ask": {"type": "string"},
            "route": {"type": "string"},
            "confidence": {"type": "integer"},
            "evidence": {"type": "array", "items": {"type": "string"}},
            "next_step": {"type": "string"}
        }
    })
}

pub fn lane_system(m: &AgentManifest, surgeon: Option<&Surgeon>, surgeon_charter: &str) -> String {
    let (name, ro) = surgeon
        .map(|s| (s.name.as_str(), s.read_only))
        .unwrap_or(("general", true));
    format!(
        "You are the `{name}` surgeon of the desk `{desk}`. You handle ONE item this call, cold: you \
         carry no memory between calls, so everything you conclude must be in your answer. Do the \
         homework with the desk's read-only commands (run them; do not guess what they would say), \
         then hand back. {write_rule}\n\n\
         A NEGATIVE THAT LICENSES ACTION IS PROVEN ON THE SAME OBJECT: a zero, an empty list or a 404 \
         counts only after the same probe is shown to find something on that object. A number you did \
         not measure this call is not evidence.\n\n\
         YOUR CHARTER:\n{charter}\n\n\
         Output JSON only: {{homework, finding, draft:{{target, kind, body}}?, ask?, route?, confidence, evidence, next_step}}.\n\
         - homework: what you ran and what it returned, with the exact identifiers.\n\
         - finding: the conclusion, one paragraph, specific.\n\
         - draft: ONLY when the objective asked for one. `target` is the exact object (ticket key, \
         URL, page id), `kind` is comment | reply | page | message, `body` is the text in the \
         desk's voice: plain prose, first person, no bullets, no bold, no headings, no assistant-talk, \
         terse.\n\
         - ask: the one question only the human can answer, when the item is blocked on it.\n\
         - route: another surgeon's name when this item is theirs.\n\
         - confidence: 0-3 (0 = guessed, 1 = plausible, 2 = one source read, 3 = verified on the object).\n\
         - evidence: the commands/URLs/ids a reviewer can re-run.\n\
         - next_step: the concrete next action for whoever takes this over.",
        desk = m.name,
        charter = if surgeon_charter.trim().is_empty() {
            "(no surgeon charter: apply the desk charter you were given)"
        } else {
            surgeon_charter.trim()
        },
        write_rule = if ro {
            "You have NO file-writing tools and you never post: a draft is text in your answer."
        } else {
            "You may write files inside the desk directory when the objective says so; you never post."
        },
    )
}

pub fn lane_user(
    desk_charter: &str,
    ledger_block: &str,
    lane: &LanePlan,
    poll_excerpt: &str,
) -> String {
    format!(
        "THE DESK CHARTER (binding):\n{charter}\n\n---\nTHE LEDGER SO FAR:\n{ledger}\n\n---\nYOUR ITEM: {item}\nKIND: {kind}\nOBJECTIVE: {objective}\n\n---\nWHAT THE POLL SAID ABOUT IT (excerpt):\n{poll}\n\n---\nDo the homework now, then answer. JSON only.",
        charter = desk_charter.trim(),
        ledger = ledger_block.trim(),
        item = lane.item,
        kind = if lane.kind.is_empty() { "research" } else { &lane.kind },
        objective = lane.objective,
        poll = if poll_excerpt.trim().is_empty() { "(nothing matched this item in the poll; the identifiers above are what you have)" } else { poll_excerpt.trim() },
    )
}

/// The poll lines that mention the lane's item (its identifiers), with a little context — so a
/// lane reads about ITS object, not the whole inbox.
pub fn poll_excerpt_for(poll: &str, item: &str) -> String {
    let keys: Vec<String> = item
        .split(|c: char| {
            !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == ':' || c == '.')
        })
        .filter(|w| w.len() >= 4)
        .map(|w| w.to_lowercase())
        .collect();
    if keys.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = poll.lines().collect();
    let mut keep = vec![false; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        let low = l.to_lowercase();
        if keys.iter().any(|k| low.contains(k)) {
            for k in &mut keep[i.saturating_sub(2)..(i + 3).min(lines.len())] {
                *k = true;
            }
        }
    }
    lines
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(l, _)| *l)
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------- LENS (the red team)

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LensOut {
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub fixes: Vec<String>,
}

pub fn lens_schema() -> Value {
    json!({
        "type": "object",
        "required": ["verdict", "notes", "fixes"],
        "properties": {
            "verdict": {"type": "string"},
            "notes": {"type": "string"},
            "fixes": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub fn lens_charter(lens: &str) -> String {
    match lens {
        "factual" => "FACTUAL lens. Re-derive every claim in the draft from the source: run the same \
            read-only commands the surgeon cited, open the same objects. A claim with no evidence you \
            could reproduce is REFUTED. A number that differs from what you measure is REFUTED. Do not \
            trust the surgeon's framing — one lens must start from the object, and that is you."
            .to_string(),
        "duplication" => "DUPLICATION AND HOMEWORK lens. Read the live thread / object the draft targets \
            and the ledger. If the draft says what is already said there, or answers a question nobody \
            asked, or asks the human for homework the desk could do itself, REFUTE. If someone else \
            already replied since the poll, REFUTE and say so."
            .to_string(),
        "voice" => "VOICE AND AUDIENCE lens. The draft is posted under a real person's name and must read \
            as written by them: plain prose, first person, contractions, terse, no bullets, no bold, no \
            headings, no affirmation opener (\"Great question\"), no assistant-talk (\"I verified\", \
            \"as an AI\"), no meta-narration, no canned sign-off. Check the audience: is this public where \
            it should be internal, or addressed to the wrong person? Any of these is REFUTED, with the \
            offending words quoted."
            .to_string(),
        other => format!(
            "{} lens. Attack the draft from this angle only. Your job is to find the reason NOT to \
             post it; if you find none after actually reading the object, say PASS and what you checked.",
            other.to_uppercase()
        ),
    }
}

pub fn lens_system(m: &AgentManifest, lens: &str) -> String {
    format!(
        "You are one adversarial reviewer of the desk `{}`. Default to REFUTED when uncertain; a PASS \
         is earned by checking, not by reading. {}\n\nOutput JSON only: {{verdict: \"PASS\"|\"REFUTED\", \
         notes, fixes:[]}}. `notes` quotes what you checked and what you found. `fixes` are exact \
         replacements (find → replace) when a small edit would make the draft postable.",
        m.name,
        lens_charter(lens)
    )
}

pub fn lens_user(desk_charter: &str, lane: &LanePlan, lane_out: &LaneOut, draft: &Draft) -> String {
    format!(
        "THE DESK CHARTER (binding):\n{charter}\n\n---\nTHE ITEM: {item}\nOBJECTIVE THE SURGEON HAD: {objective}\n\nSURGEON'S HOMEWORK:\n{homework}\n\nSURGEON'S FINDING:\n{finding}\n\nEVIDENCE THE SURGEON CITED:\n{evidence}\nSURGEON'S CONFIDENCE: {conf}\n\n---\nTHE DRAFT ({kind} → {target}):\n\"\"\"\n{body}\n\"\"\"\n\n---\nAttack it now. JSON only.",
        charter = desk_charter.trim(),
        item = lane.item,
        objective = lane.objective,
        homework = lane_out.homework.trim(),
        finding = lane_out.finding.trim(),
        evidence = if lane_out.evidence.is_empty() { "(none cited)".to_string() } else { lane_out.evidence.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n") },
        conf = lane_out.confidence,
        kind = draft.kind,
        target = draft.target,
        body = draft.body.trim(),
    )
}

// ---------------------------------------------------------------- SYNTHESIS

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SynthOut {
    #[serde(default)]
    pub stage: Vec<StagePlan>,
    #[serde(default)]
    pub asks: Vec<AskPlan>,
    #[serde(default)]
    pub facts: Vec<String>,
    #[serde(default)]
    pub log_line: String,
    #[serde(default)]
    pub scratchpad: Option<String>,
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub handoff: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StagePlan {
    pub lane: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

pub fn synthesis_schema() -> Value {
    json!({
        "type": "object",
        "required": ["stage", "asks", "facts", "log_line", "handoff"],
        "properties": {
            "stage": {"type": "array", "items": {
                "type": "object", "required": ["lane", "target", "kind", "body"],
                "properties": {
                    "lane": {"type": "string"}, "target": {"type": "string"},
                    "kind": {"type": "string"}, "body": {"type": "string"},
                    "evidence": {"type": "array", "items": {"type": "string"}}
                }
            }},
            "asks": {"type": "array", "items": {
                "type": "object", "required": ["question", "why"],
                "properties": {"question": {"type": "string"}, "why": {"type": "string"}}
            }},
            "facts": {"type": "array", "items": {"type": "string"}},
            "log_line": {"type": "string"},
            "scratchpad": {"type": "string"},
            "pending": {"type": "array", "items": {"type": "string"}},
            "handoff": {"type": "string"}
        }
    })
}

pub fn synthesis_system(m: &AgentManifest, tick: u64) -> String {
    format!(
        "You are the ORCHESTRATOR of the desk `{name}` closing tick #{tick}. The lanes have returned and \
         the reviewers have attacked every draft. You decide what the desk KEEPS from this tick:\n\
         - stage: the drafts that survived review (every lens PASS, or a REFUTED whose fixes you applied \
         and can name) — with the final body. A draft any lens refuted on facts is NOT staged; its \
         finding goes to facts or to an ask instead. Nothing here posts now: a staged draft posts on a \
         later tick through the desk's one write path, after the human's approval when the desk requires it.\n\
         - asks: questions only the human can answer, exact and answerable in one line.\n\
         - facts: what the ledger should remember from this tick — measured things with their \
         identifiers, decisions taken, dead ends (so no later tick repeats them). Not restatements of the poll.\n\
         - log_line: ONE line for the daily log in the desk owner's voice, plain, what happened.\n\
         - scratchpad: the running note for the next tick, rewritten whole (goal, in flight, next, facts).\n\
         - pending: lines for the human's pending file (things only they can clear), if any new.\n\
         - handoff: the concrete next step for the next tick's orchestrator.\n\
         Output JSON only.",
        name = m.name,
    )
}

pub fn synthesis_user(
    orient_summary: &str,
    ledger_block: &str,
    lane_reports: &str,
    review_reports: &str,
    notes: &[String],
) -> String {
    format!(
        "WHAT THIS TICK SET OUT TO DO:\n{summary}\n\n---\nTHE LEDGER BEFORE THIS TICK:\n{ledger}\n\n---\nWHAT THE LANES RETURNED:\n{lanes}\n\n---\nWHAT THE REVIEWERS SAID:\n{review}\n\n---\nNOTES FROM THE HUMAN THIS TICK:\n{notes}\n\n---\nClose the tick. JSON only.",
        summary = orient_summary.trim(),
        ledger = ledger_block.trim(),
        lanes = if lane_reports.trim().is_empty() { "(no lanes ran)" } else { lane_reports.trim() },
        review = if review_reports.trim().is_empty() { "(no drafts, so no review)" } else { review_reports.trim() },
        notes = if notes.is_empty() { "(none)".to_string() } else { notes.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n") },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_excerpt_keeps_the_lines_about_the_item_with_context() {
        let poll = "a\nb\nITHUB-4821 needs access\nc\nd\ne\nf\nITHUB-9 other\n";
        let ex = poll_excerpt_for(poll, "ITHUB-4821 access");
        assert!(ex.contains("ITHUB-4821 needs access"));
        assert!(ex.contains("a\n"));
        assert!(ex.contains("d"));
        assert!(!ex.contains("ITHUB-9 other"));
        assert!(poll_excerpt_for(poll, "x").is_empty());
    }

    #[test]
    fn every_prompt_carries_this_ticks_facts_not_a_template() {
        let m: AgentManifest = serde_yaml::from_str(&AgentManifest::starter("axpo")).unwrap();
        let s = orient_system(&m, 7);
        assert!(s.contains("`axpo`"));
        assert!(s.contains("tick #7"));
        let u = orient_user(
            "CHARTER-TEXT",
            "",
            "LEDGER",
            &["note one".into()],
            "POLL-ROW",
            &[],
        );
        for needle in ["CHARTER-TEXT", "LEDGER", "note one", "POLL-ROW"] {
            assert!(u.contains(needle), "{needle}");
        }
        assert!(lens_system(&m, "voice").contains("VOICE"));
        assert!(lens_system(&m, "gdpr").contains("GDPR lens"));
    }
}
