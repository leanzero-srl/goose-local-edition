//! Automatic recall: every user turn, the user's own words are searched against the memory store and
//! the skill catalogue, and what matches rides into the turn context — so a model gets the facts it
//! already saved and the skill that fits without first having to decide to call a tool. Measured need:
//! a local 27B rarely calls `search_memories` or `load_skill` on its own.
//!
//! Only the request turn triggers it (the last message is the user's text, not a tool response), only
//! when the memory / skills extensions are enabled in the session, and it names its own absence: no
//! match, no block.

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::tool_execution::ToolCallContext;
use crate::config::paths::Paths;
use crate::conversation::effective_role;
use crate::conversation::message::{Message, MessageContent};
use crate::session::session_manager::SessionType;
use anyhow::Result;
use async_trait::async_trait;
use goose_memory_store::{
    headline, rarity_weight, said_together, search_terms, term_occurrences, tokenize, MemoryStore,
    SearchHit, STOPWORDS,
};
use goose_sdk_types::custom_requests::{SourceEntry, SourceType};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    ServerCapabilities,
};
use tokio_util::sync::CancellationToken;

pub static EXTENSION_NAME: &str = "recall";

// ratio: at most a fifth of the index budget — three entries at the importer's 2,500-char cap against
// the 39k-char index measured on the first machine.
const RECALL_MAX_MEMORIES: usize = 3;
// ratio: a skill line is a name and a description; three of them is the same budget as one memory.
const RECALL_MAX_SKILLS: usize = 3;
// ratio: a candidate rides along only while it scores at least half of the best candidate — measured
// on the 171-entry store, the slots below that line were filled by entries sharing six common words.
const RECALL_MIN_SHARE_OF_TOP: f64 = 0.5;

/// Words that open a correction. Read at the head of the message (the first REACTION_WINDOW tokens),
/// where a reaction lives; deeper in a long request they are ordinary words.
const CORRECTION_MARKERS: &[&str] = &[
    "no", "nope", "don't", "dont", "never", "stop", "wrong", "not", "instead", "again", "undo",
    "revert", "why",
];
const CORRECTION_PHRASES: &[&str] = &[
    "not what i",
    "i said",
    "i told you",
    "that's not",
    "thats not",
    "i didn't ask",
    "i did not ask",
    "please don't",
    "do not",
    "you should have",
    "should not have",
    "shouldn't have",
];
// ratio: a reaction is said in the first breath — a dozen words; a request that only mentions "no"
// or "instead" later is a request.
const REACTION_WINDOW: usize = 12;
// ratio: a skill body is loaded without a call only while it fits in a thirty-second of the context
// window — one extra page for a 262k model, nothing for a 32k one — and only on a name match.
const AUTOLOAD_WINDOW_SHARE: f64 = 1.0 / 32.0;
// measured: a token is about four characters of English or code across the providers goose runs.
const CHARS_PER_TOKEN: f64 = 4.0;

/// What the user is reacting to: everything the assistant did in its previous turn — the tool calls,
/// in order, then its closing words — gathered back to the previous user request. None when an
/// earlier user request sits between (the assistant did not speak last).
pub fn previous_assistant_text(messages: &[Message]) -> Option<String> {
    let mut seen_request = false;
    let mut parts: Vec<String> = Vec::new();
    for m in messages.iter().rev() {
        if !m.is_agent_visible() {
            continue;
        }
        if !seen_request {
            seen_request = true;
            continue;
        }
        match effective_role(m).as_str() {
            "assistant" => {
                let text: Vec<&str> = m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        MessageContent::Text(t) => Some(t.text.as_str()),
                        _ => None,
                    })
                    .collect();
                let text = text.join("\n");
                if !text.trim().is_empty() {
                    parts.push(text);
                }
                for c in m.content.iter().rev() {
                    if let MessageContent::ToolRequest(req) = c {
                        if let Ok(call) = req.tool_call.as_ref() {
                            let args = call
                                .arguments
                                .as_ref()
                                .map(|a| serde_json::Value::Object(a.clone()).to_string())
                                .unwrap_or_default();
                            parts.push(format!("tool {}({})", call.name, headline(&args)));
                        }
                    }
                }
            }
            "user"
                if m.content
                    .iter()
                    .any(|c| matches!(c, MessageContent::Text(_))) =>
            {
                break;
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join("\n"))
}

/// How surely the request is a correction of what was just done. Strong: an unambiguous phrase
/// ("don't", "never", "that's not what I", "stop") in the head — captured as a memory without waiting
/// for the model. Weak: a marker word that also opens ordinary requests ("why", "not", "again") —
/// the model is nudged, nothing is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    Strong,
    Weak,
}

const STRONG_MARKERS: &[&str] = &[
    "don't", "dont", "never", "stop", "wrong", "nope", "undo", "revert",
];

pub fn correction_strength(user_text: &str) -> Option<Correction> {
    let lower = user_text.to_lowercase();
    let head: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|t| !t.is_empty())
        .take(REACTION_WINDOW)
        .collect();
    let head_text = head.join(" ");
    if CORRECTION_PHRASES.iter().any(|p| head_text.contains(p))
        || head.iter().take(3).any(|t| STRONG_MARKERS.contains(t))
    {
        return Some(Correction::Strong);
    }
    if head.iter().take(3).any(|t| CORRECTION_MARKERS.contains(t)) {
        return Some(Correction::Weak);
    }
    None
}

/// Does the request open like a correction of what was just done?
pub fn is_correction(user_text: &str) -> bool {
    correction_strength(user_text).is_some()
}

/// The memory a strong correction becomes, written by recall itself: the user's words are the
/// headline, the corrected action the body, so the model can refine it in place (same headline).
pub fn correction_memory(user_text: &str, action: &str) -> (String, Vec<String>) {
    let content = format!(
        "Correction: {}\nSaid after goose did: {}\nRefine this into the rule and the reason if the words above are not already it.",
        headline(user_text),
        action
    );
    (
        content,
        vec!["feedback".to_string(), "correction".to_string()],
    )
}

/// The question the assistant left open, when its last message asked one.
pub fn open_question(assistant_text: &str) -> Option<String> {
    let last = assistant_text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())?;
    let lower = last.to_lowercase();
    let asks = last.ends_with('?')
        || [
            "which ",
            "should i",
            "do you want",
            "not sure",
            "unsure",
            "assume",
            "confirm",
        ]
        .iter()
        .any(|m| lower.contains(m));
    asks.then(|| headline(last))
}

/// Which skill to load without a call: the best suggestion, when the request names it (two name
/// terms) and its body fits the auto-load budget.
pub fn autoload_pick<'a>(
    ranked: &[(usize, &'a SourceEntry)],
    context_limit_tokens: usize,
) -> Option<&'a SourceEntry> {
    let budget = (context_limit_tokens as f64 * CHARS_PER_TOKEN * AUTOLOAD_WINDOW_SHARE) as usize;
    ranked
        .first()
        .filter(|(name_terms, skill)| *name_terms >= 2 && skill.content.chars().count() <= budget)
        .map(|(_, skill)| *skill)
}

pub struct RecallClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
}

impl RecallClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Recall"))
            .with_instructions(
                "When a request's words match saved memories or a skill's description, the turn context \
                 carries a <recalled-memories> block (those memories in full) and a <relevant-skills> block \
                 (skill names to load with load_skill). goose adds them; use them when they apply and ignore \
                 them when they do not."
                    .to_string(),
            );
        Ok(Self { info, context })
    }

    async fn extension_enabled(&self, name: &str) -> bool {
        match self
            .context
            .extension_manager
            .as_ref()
            .and_then(|weak| weak.upgrade())
        {
            Some(manager) => manager.is_extension_enabled(name).await,
            None => false,
        }
    }
}

/// The user's text when the last message is a fresh request; None during a tool loop, where the last
/// user-role message carries tool responses and the request has already been recalled.
pub fn last_user_text(messages: &[Message]) -> Option<String> {
    let last = messages.iter().rev().find(|m| m.is_agent_visible())?;
    if effective_role(last) != "user" {
        return None;
    }
    if last
        .content
        .iter()
        .any(|c| matches!(c, MessageContent::ToolResponse(_)))
    {
        return None;
    }
    let text: Vec<&str> = last
        .content
        .iter()
        .filter_map(|c| match c {
            MessageContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect();
    let text = text.join("\n");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// The query terms a request contributes: its words minus function words and one-letter tokens.
pub fn query_terms(text: &str) -> Vec<String> {
    search_terms(text)
        .into_iter()
        .filter(|t| t.chars().count() > 1 && !STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// Which hits are worth injecting: an entry the request NAMES (`SearchHit::named`), an entry whose
/// body carries EVERY request term, or an entry whose name carries the request's TOPIC WORD
/// (`SearchHit::topic_in_name`, its rarest term) with half of the terms and a MAJORITY of the request's
/// specific terms matched — each with at least one rare term, scoring at least half of the best such
/// hit; taken in the store's order, so the named entries fill the slots first. Once the topic word
/// names an entry, the entries named by the request's commoner words alone do not ride.
/// Read on the goose-native store (62 local + 171
/// global entries, 13 requests): true hits carry name terms and win by a wide margin (golden engine 14.8
/// vs 8.8, killpg 24.1, fleet names 31.3); every noise slot was a nameless entry matching half the
/// request ("list the files" → note-5e3df2), while the nameless entries
/// worth keeping matched it whole (note-93f4b2, 4/4, for "deploy the Forge app").
/// Measured (VA-181, same store): a single name term is not aboutness — "connect to the workhorse over
/// SSH" filled two slots on "workhorse" alone (a JACCL cluster note, a WindowServer-OOM note), "list the
/// files in this directory" one on "files" (a grep -I note), "write a blog post about local models"
/// three on "local"/"models" (swarm notes); none carried the request's topic word (ssh, directory,
/// blog) in its name. Requiring it emptied those three requests and dropped seven shared-vocabulary
/// slots elsewhere (a launchd-guard note for the e2e probe token, a signing-cert note for the git
/// identity); every named hit and every whole-request body stayed — 30 → 17 of 39 slots.
/// Measured (VA-183, 233 entries, 19 requests): the topic word in ANOTHER SENSE rode below the entry
/// it names. "Is swarm resume still broken?" (broken df 30, resume 8 | still 68, swarm 71; topic resume)
/// recalled `swarm-resume-works-now` (4/4, named, 14.3) and then two unnamed notes whose headlines carry
/// "resume" as continuing after an interruption — `remine-after-compaction` ("I have resumed on the
/// wrong thread", 3/4, 9.0) and `recovery-is-separate-from-detection` ("whether the loop can RESUME
/// afterwards", 2/4, 7.9) — both missing "broken", the request's other specific word; the one unnamed
/// topic rider worth keeping, `score-serially-hermetically-advertised-port` on the golden-score request,
/// matches both specific words (golden, score). "How do I release a notarized build of the desktop
/// app?" (notarized df 2, release 9, desktop 17 | app 58, build 79) recalled `macos-notarization-setup`
/// (5/5, named by notarized + release, 20.5) and then two notes NAMED by the commoner words —
/// `swarm-shipping-phases` (desktop + release, "Mihai's shipping roadmap", 14.1) and
/// `swarm-verify-in-the-running-app` (app + desktop, "verify in the running app", 12.2) — neither
/// carrying "notarized" anywhere; on the killpg request `launch-longlived-apps-via-launchd` stays,
/// named by a tied topic word ("reap"). After: the four slots empty, the other seventeen requests
/// identical — 26 → 22 of 57.
/// Measured (VA-185, 233 entries, 25 requests): a nameless body carrying EVERY request term also has to
/// say two of them TOGETHER (`SearchHit::together`). "How should I open a plan when I present it?"
/// (open, plan, present — 3/3) recalled its named note and then `do-all-of-it-never-defer` ("a window
/// opens in 20 minutes … not a phased plan … presenting my own scheduling caution", 5.8) and
/// `local-qwen-swarm-agent` ("a single OpenAI endpoint … Approved plan … Toolchain present", 5.8) —
/// three everyday words, each alone in its own sentence of a long body. The two nameless
/// whole-request rides worth keeping say the request's words side by side: `bank-agent-three-bucket-rule`
/// ("Forge deploy" in its bucket-2 list) and `goose-branch-map-main-vs-local-edition` ("r6h golden
/// 0.4616 … 14 engine commits … restored to the r6h golden"). After: the plan request keeps only its
/// named note; the other twenty-four requests identical.
/// VA-187 lives in the store: an IDENTIFIER (a term with a digit) in the name names the entry by
/// itself (`SearchHit::identifier_in_name`), a `key:value` tag is not a name word, and a topic
/// reached only through a stem yields to an entry named by the word itself
/// (`SearchHit::topic_word_in_name`) — "Why did the r2 run die in the middle of INTEGRATE?" recalls
/// `kill-pids-never-killpg`, "Can the Claude Code harness run the desk loops on its own?" keeps
/// `autonomous-loop-operating-mode` alone; 33 → 32 of 93 on 31 requests.
/// VA-187 (2): once a NAMED hit carries the WHOLE request, a named hit that carries part of it rides
/// only on the topic word in its name — the same law as the topic-named entry, triggered by the entry
/// that says everything the request says. Measured (233 entries, 37 requests): "Can I change the
/// workflow scheme on the client's production Jira myself, or ask first?" (scheme df 3, myself 15,
/// production 23, workflow 34, jira 40 | ask 55, client 55, change 68, first 95; topic scheme, in no
/// name) recalled `ask-before-client-prod-config` (9/9, named by ask + change + client + production,
/// 24.3) and then `cloud-sandbox-shares-groups-with-prod` (5/9, named by client + production, 15.3:
/// "an Atlassian Cloud sandbox is NOT isolated from production for GROUPS and USERS" — what a sandbox
/// shares with production, not whether to change the scheme). After: the rule alone; the killpg pair
/// stays (`launch-longlived-apps-via-launchd`, 5/6, carries the tied topic word "reap" in its name);
/// the thirty-five other requests identical.
pub fn select_hits(hits: Vec<SearchHit>, term_count: usize) -> Vec<SearchHit> {
    let topic_names_an_entry = hits.iter().any(|hit| hit.named && hit.topic_in_name);
    let whole_request_named = hits
        .iter()
        .any(|hit| hit.named && hit.matched_terms >= term_count);
    let covers = |hit: &SearchHit| {
        hit.rare_terms >= 1
            && if hit.named {
                hit.topic_in_name
                    || (!topic_names_an_entry
                        && (!whole_request_named || hit.matched_terms >= term_count))
            } else {
                (hit.matched_terms >= term_count && hit.together)
                    || (hit.topic_in_name
                        && hit.matched_terms * 2 >= term_count
                        && hit.matched_specific * 2 > hit.specific_terms)
            }
    };
    let top = hits
        .iter()
        .filter(|hit| covers(hit))
        .map(|hit| hit.score)
        .fold(0.0_f64, f64::max);
    hits.into_iter()
        .filter(|hit| covers(hit) && hit.score >= top * RECALL_MIN_SHARE_OF_TOP)
        .take(RECALL_MAX_MEMORIES)
        .collect()
}

/// A skill's `metadata.keywords` (a string or a list of strings in SKILL.md frontmatter) — words the
/// author wants requests to reach the skill by, matched with name strength.
pub fn skill_keywords(skill: &SourceEntry) -> String {
    match skill.properties.get("keywords") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// One skill scored against a request: the same numbers the memory hits carry, so `goose recall`
/// can say why a skill was or was not suggested.
#[derive(Debug, Clone)]
pub struct SkillHit<'a> {
    pub score: f64,
    pub matched_terms: usize,
    pub rare_terms: usize,
    pub name_terms: usize,
    /// Name terms that are this skill's OWN: in no other skill's name or keywords. A word several
    /// names share — goose, atlassian, api, skill — is a family word and names nothing.
    pub own_name_terms: usize,
    /// Two request words said together in the skill's name, keywords or description
    /// (`goose_memory_store::said_together`) — the description path's aboutness.
    pub together: bool,
    pub about: bool,
    pub skill: &'a SourceEntry,
}

/// Every skill sharing a term with the request, best first: terms weighted by their rarity across
/// the catalogue (a name or keyword match counts twice), and whether the skill is ABOUT the request.
pub fn skill_hits<'a>(skills: &'a [SourceEntry], terms: &[String]) -> Vec<SkillHit<'a>> {
    let catalogue: Vec<(&SourceEntry, Vec<String>, Vec<String>)> = skills
        .iter()
        .filter(|s| matches!(s.source_type, SourceType::Skill | SourceType::BuiltinSkill))
        .map(|skill| {
            let keywords = skill_keywords(skill);
            let name = tokenize(&format!("{} {}", skill.name, keywords));
            let text = tokenize(&format!(
                "{} {} {}",
                skill.name, keywords, skill.description
            ));
            (skill, name, text)
        })
        .collect();
    let n = catalogue.len();
    let document_frequency: Vec<usize> = terms
        .iter()
        .map(|term| {
            catalogue
                .iter()
                .filter(|(_, _, text)| term_occurrences(term, text) > 0)
                .count()
        })
        .collect();
    let name_frequency: Vec<usize> = terms
        .iter()
        .map(|term| {
            catalogue
                .iter()
                .filter(|(_, name, _)| term_occurrences(term, name) > 0)
                .count()
        })
        .collect();
    let mut hits: Vec<SkillHit<'a>> = catalogue
        .iter()
        .filter_map(|(skill, name, text)| {
            let mut score = 0.0;
            let mut rare_terms = 0;
            let mut matched_terms = 0;
            let mut name_terms = 0;
            let mut own_name_terms = 0;
            for (i, term) in terms.iter().enumerate() {
                if term_occurrences(term, text) == 0 {
                    continue;
                }
                matched_terms += 1;
                let weight = rarity_weight(n, document_frequency[i]);
                score += weight;
                if document_frequency[i] * 2 <= n {
                    rare_terms += 1;
                }
                if term_occurrences(term, name) > 0 {
                    name_terms += 1;
                    score += weight;
                    if name_frequency[i] == 1 {
                        own_name_terms += 1;
                    }
                }
            }
            if matched_terms == 0 {
                return None;
            }
            let together = said_together(text, terms);
            // a skill named by the request — a word of its name or keywords that no other skill's
            // carries — is suggested on one rare term; one matched only by its description, or only
            // by a family word its name shares with others, needs two rare terms and half the
            // request. The description rule: the suggestion line ran at 29 skills on 13 probe
            // requests before it, naming a tenant skill for "set up my scratchpad". The own-word
            // rule (VA-186, 30 skills, 25 requests): "Should I remind him to rotate the API key I was
            // just given?" suggested atlassian-organizations-api-skill, confluence-api-skill and
            // jira-api-skill on "api" alone — 1 of 5 terms, a word in 14 of the 30 descriptions and
            // 3 of the names, score 1.5 — while the request is about whether to nag. The
            // description path also has to say two of the request's words TOGETHER (VA-188, 30
            // skills, 31 requests): "Deploy the app to the sandbox first, then production." suggested
            // the tenant desks alterdomus ("(ET- on production; AHUB-, … on the sandbox) … the
            // 'Altomata' Forge automations app", 3/5) and bankofireland ("Forge app development, Forge
            // deployment/approval … every change to a production system", 3/5) — a request naming no
            // tenant, key or site, matched on the words every desk's description carries apart; the
            // description suggestions worth keeping say the words side by side: "blog post",
            // "weekly write-up", "LM Studio", "Jira issues", "Forge deployment", "run or benchmark".
            let about = if own_name_terms >= 1 {
                rare_terms >= 1
            } else {
                rare_terms >= 2 && matched_terms * 2 >= terms.len() && together
            };
            Some(SkillHit {
                score,
                matched_terms,
                rare_terms,
                name_terms,
                own_name_terms,
                together,
                about,
                skill,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.skill.name.cmp(&b.skill.name))
    });
    hits
}

/// Skills the request is about, by the same rule as memories: the hits that are ABOUT it, scoring
/// at least half the best of them, at most `RECALL_MAX_SKILLS`.
pub fn relevant_skills<'a>(skills: &'a [SourceEntry], terms: &[String]) -> Vec<&'a SourceEntry> {
    let hits = skill_hits(skills, terms);
    let top = hits
        .iter()
        .filter(|hit| hit.about)
        .map(|hit| hit.score)
        .fold(0.0_f64, f64::max);
    hits.into_iter()
        .filter(|hit| hit.about && hit.score >= top * RECALL_MIN_SHARE_OF_TOP)
        .take(RECALL_MAX_SKILLS)
        .map(|hit| hit.skill)
        .collect()
}

/// The suggested skills with how many request terms sit in their name or keywords, best first.
pub fn ranked_skills<'a>(
    skills: &'a [SourceEntry],
    terms: &[String],
) -> Vec<(usize, &'a SourceEntry)> {
    relevant_skills(skills, terms)
        .into_iter()
        .map(|skill| {
            let name = tokenize(&format!("{} {}", skill.name, skill_keywords(skill)));
            let name_terms = terms
                .iter()
                .filter(|term| term_occurrences(term, &name) > 0)
                .count();
            (name_terms, skill)
        })
        .collect()
}

/// The most recent earlier session whose words cover the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastSession {
    pub session_id: String,
    pub description: String,
    pub when: String,
    pub role: String,
    pub headline: String,
}

// ratio: the history search is an OR of LIKEs, so it returns every message sharing one word; twenty
// rows is enough to find one that covers the request when one exists, and cheap when none does.
const PAST_SESSION_ROWS: usize = 20;

/// From history-search rows (newest first), the first message in another session that covers the
/// request by the same rule memories use: at least half of the request's terms.
pub fn select_past_session(
    results: &[crate::session::chat_history_search::ChatRecallResult],
    terms: &[String],
) -> Option<PastSession> {
    let mut candidates: Vec<(chrono::DateTime<chrono::Utc>, PastSession)> = Vec::new();
    for result in results {
        for message in &result.messages {
            let tokens = tokenize(&message.content);
            let matched = terms
                .iter()
                .filter(|term| term_occurrences(term, &tokens) > 0)
                .count();
            if matched * 2 < terms.len() {
                continue;
            }
            candidates.push((
                message.timestamp,
                PastSession {
                    session_id: result.session_id.clone(),
                    description: result.session_description.clone(),
                    when: message.timestamp.format("%Y-%m-%d %H:%M").to_string(),
                    role: message.role.clone(),
                    headline: headline(&message.content),
                },
            ));
        }
    }
    candidates.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    candidates.into_iter().next().map(|(_, past)| past)
}

/// One line naming what rode along — shown to the person as a system notice and carried at the top of
/// the part so the same words reach the model.
pub fn recall_line(
    memories: &[SearchHit],
    skills: &[&SourceEntry],
    past: Option<&PastSession>,
) -> String {
    let mut parts = Vec::new();
    if !memories.is_empty() {
        let names: Vec<&str> = memories.iter().map(|h| h.entry.category.as_str()).collect();
        parts.push(format!("memories {}", names.join(", ")));
    }
    if !skills.is_empty() {
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        parts.push(format!("skills {}", names.join(", ")));
    }
    if let Some(past) = past {
        parts.push(format!("past session {}", past.session_id));
    }
    format!("recalled: {}", parts.join(" · "))
}

/// The `<recall-line>` text of a turn-context block, if the block carries one.
pub fn recall_line_of(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once("<recall-line>")?;
    let (line, _) = rest.split_once("</recall-line>")?;
    Some(line)
}

/// What else the turn carries besides matches: a loaded skill body, a correction to capture, an
/// answered question to capture.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Extras {
    pub autoloaded: Option<(String, String)>,
    pub correction_of: Option<String>,
    pub answered: Option<(String, String)>,
}

/// The turn-context part. None when there is nothing to say.
pub fn render(
    memories: &[SearchHit],
    skills: &[&SourceEntry],
    past: Option<&PastSession>,
) -> Option<String> {
    render_with(memories, skills, past, &Extras::default())
}

pub fn render_with(
    memories: &[SearchHit],
    skills: &[&SourceEntry],
    past: Option<&PastSession>,
    extras: &Extras,
) -> Option<String> {
    if memories.is_empty()
        && skills.is_empty()
        && past.is_none()
        && extras.autoloaded.is_none()
        && extras.correction_of.is_none()
        && extras.answered.is_none()
    {
        return None;
    }
    let mut line = recall_line(memories, skills, past);
    if let Some((name, _)) = &extras.autoloaded {
        line.push_str(&format!(" · loaded {name}"));
    }
    if extras.correction_of.is_some() {
        line.push_str(" · correction noticed");
    }
    if extras.answered.is_some() {
        line.push_str(" · answer noticed");
    }
    let mut sections = vec![format!("<recall-line>{line}</recall-line>")];
    if !memories.is_empty() {
        let mut block = String::from(
            "<recalled-memories>\nSaved memories whose words match this request — recalled by goose, use them if they apply:\n",
        );
        for hit in memories {
            block.push('\n');
            block.push_str(&hit.entry.render());
        }
        block.push_str("</recalled-memories>");
        sections.push(block);
    }
    if !skills.is_empty() {
        let mut block = String::from(
            "<relevant-skills>\nSkills whose description matches this request; load one with load_skill(name) before doing the work it covers:\n",
        );
        for skill in skills {
            block.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        block.push_str("</relevant-skills>");
        sections.push(block);
    }
    if let Some(past) = past {
        sections.push(format!(
            "<past-session>\nThis was discussed before — session {} (\"{}\", {}), the {} said: \"{}\". \
             chatrecall(session_id) loads it if the history matters.\n</past-session>",
            past.session_id, past.description, past.when, past.role, past.headline
        ));
    }
    if let Some((name, body)) = &extras.autoloaded {
        sections.push(format!(
            "<loaded-skill name=\"{name}\">\nThis skill matches the request by name, so goose loaded it for you — follow it as if you had called load_skill({name}):\n{body}\n</loaded-skill>"
        ));
    }
    if let Some(action) = &extras.correction_of {
        sections.push(format!(
            "<correction>\nThe user's message reads as a correction of what you just did (\"{action}\"). \
             A strong correction is already saved verbatim in local memory (category \"corrections\"); \
             restate it as the RULE and the REASON with remember_memory — same first line, tags feedback first — \
             so the saved memory says what to do, not only what was said. Then continue.\n</correction>"
        ));
    }
    if let Some((question, answer)) = &extras.answered {
        sections.push(format!(
            "<answered>\nYou asked \"{question}\" and the user answered \"{answer}\". \
             If that answer is a durable fact — a path, a host, a convention, a preference — save it with remember_memory (tags: project or user first) so you never ask again.\n</answered>"
        ));
    }
    Some(sections.join("\n"))
}

#[async_trait]
impl McpClientTrait for RecallClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Vec::new(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        _ctx: &ToolCallContext,
        name: &str,
        _arguments: Option<JsonObject>,
        _cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        Ok(CallToolResult::error(vec![Content::text(format!(
            "recall has no tools (asked for '{name}'); it works from the turn context"
        ))]))
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    async fn get_moim(&self, session_id: &str) -> Option<String> {
        let session = match self
            .context
            .session_manager
            .get_session(session_id, true)
            .await
        {
            Ok(session) => session,
            Err(err) => {
                tracing::warn!(session_id, %err, "recall: session unreadable, nothing recalled");
                return None;
            }
        };
        let text = last_user_text(session.conversation.as_ref()?.messages())?;
        let terms = query_terms(&text);
        if terms.is_empty() {
            return None;
        }
        let query = terms.join(" ");

        let memories = if self.extension_enabled("memory").await {
            let store = MemoryStore::new(Paths::config_dir().join("memory"), &session.working_dir);
            match store.search(&query, None) {
                Ok(hits) => select_hits(hits, terms.len()),
                Err(err) => {
                    tracing::warn!(%err, "recall: memory store unreadable, nothing recalled");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let catalogue = if self.extension_enabled("skills").await {
            crate::skills::discover_skills(Some(&session.working_dir))
        } else {
            Vec::new()
        };
        let ranked = ranked_skills(&catalogue, &terms);
        let skills: Vec<&SourceEntry> = ranked.iter().map(|(_, s)| *s).collect();

        let mut extras = Extras::default();
        let context_limit = match (
            self.context.model_config_for_session(session_id).await,
            self.context
                .extension_manager
                .as_ref()
                .and_then(|weak| weak.upgrade()),
        ) {
            (Ok(model_config), Some(manager)) => {
                let provider = manager.get_provider().lock().await.clone();
                match provider {
                    Some(provider) => Some(
                        crate::context_mgmt::effective_context_limit(
                            provider.as_ref(),
                            &model_config,
                        )
                        .await,
                    ),
                    None => None,
                }
            }
            _ => None,
        };
        if let Some(limit) = context_limit {
            if let Some(skill) = autoload_pick(&ranked, limit) {
                extras.autoloaded = Some((skill.name.clone(), skill.content.clone()));
            }
        }
        let messages = session.conversation.as_ref()?.messages();
        if self.extension_enabled("memory").await {
            if let Some(assistant) = previous_assistant_text(messages) {
                if let Some(strength) = correction_strength(&text) {
                    let action = headline(&assistant);
                    if strength == Correction::Strong {
                        let store = MemoryStore::new(
                            Paths::config_dir().join("memory"),
                            &session.working_dir,
                        );
                        let (content, tags) = correction_memory(&text, &action);
                        match store.remember("corrections", &content, &tags, false) {
                            Ok(outcome) => {
                                tracing::info!(?outcome, "correction captured as a memory")
                            }
                            Err(err) => tracing::warn!(%err, "correction not captured"),
                        }
                    }
                    extras.correction_of = Some(action);
                }
                if extras.correction_of.is_none() {
                    if let Some(question) = open_question(&assistant) {
                        extras.answered = Some((question, headline(&text)));
                    }
                }
            }
        }

        let session_types = match session.session_type {
            SessionType::Acp => vec![SessionType::Acp],
            _ => vec![SessionType::User, SessionType::Scheduled],
        };
        let history_started = std::time::Instant::now();
        let past = match self
            .context
            .session_manager
            .search_chat_history(
                &query,
                Some(PAST_SESSION_ROWS),
                None,
                None,
                Some(session_id.to_string()),
                session_types,
            )
            .await
        {
            Ok(results) => select_past_session(&results.results, &terms),
            Err(err) => {
                tracing::warn!(%err, "recall: chat history unreadable, no past session named");
                None
            }
        };

        let part = render_with(&memories, &skills, past.as_ref(), &extras);
        let recalled: Vec<String> = memories
            .iter()
            .map(|hit| format!("{}({:.1})", hit.entry.category, hit.score))
            .collect();
        let suggested: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
        tracing::info!(
            session_id,
            terms = terms.len(),
            memories = memories.len(),
            recalled = ?recalled,
            skills = skills.len(),
            suggested = ?suggested,
            past_session = past.as_ref().map(|p| p.session_id.as_str()),
            history_ms = history_started.elapsed().as_millis(),
            autoloaded = extras.autoloaded.as_ref().map(|(n, _)| n.as_str()),
            correction = extras.correction_of.is_some(),
            answered = extras.answered.is_some(),
            "recall"
        );
        part
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_memory_store::MemoryEntry;

    fn hit(category: &str, score: f64, rare_terms: usize) -> SearchHit {
        named_hit(category, score, rare_terms, 1)
    }

    /// A hit matching `matched` terms, all of them rare, one of them — the request's topic word — in
    /// the name.
    fn covering_hit(category: &str, score: f64, matched: usize) -> SearchHit {
        let mut hit = named_hit(category, score, matched, 1);
        hit.matched_terms = matched;
        hit.topic_in_name = true;
        hit
    }

    fn named_hit(category: &str, score: f64, rare_terms: usize, name_terms: usize) -> SearchHit {
        SearchHit {
            score,
            matched_terms: rare_terms.max(1),
            rare_terms,
            phrase: false,
            name_terms,
            specific_terms: 1,
            matched_specific: 1,
            named: false,
            topic_word_in_name: false,
            topic_in_name: false,
            identifier_in_name: false,
            identifier_in_body: false,
            together: true,
            occurrences: rare_terms.max(1),
            entry: MemoryEntry {
                is_global: true,
                category: category.to_string(),
                tags: vec!["project".to_string()],
                content: format!("{category} headline\nbody"),
            },
        }
    }

    fn skill(name: &str, description: &str) -> SourceEntry {
        SourceEntry {
            source_type: SourceType::Skill,
            name: name.to_string(),
            description: description.to_string(),
            content: String::new(),
            path: format!("/skills/{name}"),
            supporting_files: Vec::new(),
            global: true,
            writable: false,
            properties: Default::default(),
        }
    }

    #[test]
    fn last_user_text_is_the_fresh_request_only() {
        let request = vec![Message::user().with_text("fix the postgres docker port")];
        assert_eq!(
            last_user_text(&request).as_deref(),
            Some("fix the postgres docker port")
        );

        let mut tool_loop = request.clone();
        tool_loop.push(Message::assistant().with_text("looking"));
        tool_loop.push(Message::user().with_tool_response(
            "call-1",
            Ok(CallToolResult::success(vec![Content::text("ok")])),
        ));
        assert_eq!(last_user_text(&tool_loop), None);

        let answered = vec![
            Message::user().with_text("hello"),
            Message::assistant().with_text("hi"),
        ];
        assert_eq!(last_user_text(&answered), None);
    }

    #[test]
    fn query_terms_drop_function_words() {
        let terms = query_terms("Can you fix the Postgres port in docker, please?");
        assert_eq!(terms, vec!["docker", "fix", "port", "postgres"]);
        assert!(query_terms("do it").is_empty());
    }

    #[test]
    fn select_hits_needs_half_coverage_a_rare_term_and_half_the_top_score() {
        // a five-term request: coverage needs three matched terms
        let hits = vec![
            covering_hit("best", 4.0, 3),
            covering_hit("one-common-word", 5.0, 1),
            covering_hit("two-of-five", 4.5, 2),
            covering_hit("half", 2.0, 3),
            covering_hit("under-half", 1.9, 3),
            covering_hit("also-fine", 3.0, 4),
            covering_hit("fourth", 2.5, 3),
        ];
        let kept: Vec<String> = select_hits(hits, 5)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["best", "half", "also-fine"]);
        assert!(select_hits(vec![named_hit("nothing-rare", 9.0, 0, 0)], 1).is_empty());

        // a nameless entry must cover the whole request; a named one, half of it
        let mut nameless_half = covering_hit("nameless-half", 9.0, 3);
        nameless_half.name_terms = 0;
        nameless_half.topic_in_name = false;
        let mut nameless_full = covering_hit("nameless-full", 4.0, 5);
        nameless_full.name_terms = 0;
        nameless_full.topic_in_name = false;
        let kept: Vec<String> = select_hits(vec![nameless_half, nameless_full], 5)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["nameless-full"]);
    }

    /// The VA-181 shapes. "connect ssh workhorse": two notes named only by "workhorse" match two of
    /// three terms and share the request's words, not its topic ("ssh") — neither rides. "deploy forge
    /// app production": the note the request names rides, the nameless note carrying every term rides,
    /// the bank-desk note named only by "forge" (topic: "deploy") does not. "benchmark properly run
    /// start": the named notes ride without the topic word ("properly") in their names.
    #[test]
    fn an_unnamed_hit_rides_only_on_the_whole_request_or_the_topic_word_in_its_name() {
        let mut cluster = covering_hit("distributed-mlx-jaccl-cluster", 9.0, 2);
        cluster.topic_in_name = false;
        let mut oom = covering_hit("workhorse-oom-windowserver-kill", 8.2, 2);
        oom.topic_in_name = false;
        assert!(select_hits(vec![cluster, oom], 3).is_empty());

        let mut forge_facts = named_hit("forge-live-ui-testing-facts", 13.3, 3, 3);
        forge_facts.named = true;
        forge_facts.topic_in_name = true;
        let mut bank_rule = covering_hit("bank-agent-three-bucket-rule", 8.9, 4);
        bank_rule.name_terms = 0;
        bank_rule.topic_in_name = false;
        let mut bank_desk = covering_hit("bankofireland-desk", 7.3, 2);
        bank_desk.topic_in_name = false;
        let kept: Vec<String> = select_hits(vec![forge_facts, bank_rule, bank_desk], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec![
                "forge-live-ui-testing-facts",
                "bank-agent-three-bucket-rule"
            ]
        );

        let mut observe = named_hit("swarm-5min-observation-protocol", 8.2, 2, 2);
        observe.matched_terms = 3;
        observe.named = true;
        let mut fleet = named_hit("check-the-fleet-before-you-load-it", 7.6, 2, 2);
        fleet.matched_terms = 3;
        fleet.named = true;
        let kept: Vec<String> = select_hits(vec![observe, fleet], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec![
                "swarm-5min-observation-protocol",
                "check-the-fleet-before-you-load-it"
            ]
        );
    }

    /// The VA-183 shapes. "broken resume still swarm" (topic "resume", specific broken + resume): the
    /// note the request names rides; two unnamed notes with "resume" in their headlines in another
    /// sense (resuming after a compaction, a loop resuming after a rate limit) match the topic word and
    /// the common words but not "broken" — neither rides; on "commit engine golden score" the unnamed
    /// scoring note carrying "score" in its name matches both specific words and stays. "app build
    /// desktop notarized release" (topic "notarized"): once the topic word names the notarization
    /// note, the shipping-roadmap and verify-in-the-app notes named by desktop/release/app alone do
    /// not ride; on the killpg request the launchd note named by a tied topic word ("reap") stays,
    /// and on the e2e request — no entry named by "e2e" — the scoring note named by bench/port/vendor
    /// rides as before.
    #[test]
    fn the_topic_word_in_another_sense_does_not_ride_below_the_entry_it_names() {
        let mut resume_works = named_hit("swarm-resume-works-now", 14.3, 4, 3);
        resume_works.matched_terms = 4;
        resume_works.specific_terms = 2;
        resume_works.matched_specific = 2;
        resume_works.named = true;
        resume_works.topic_in_name = true;
        let mut remine = covering_hit("remine-after-compaction", 9.0, 3);
        remine.specific_terms = 2;
        remine.matched_specific = 1;
        let mut recovery = covering_hit("recovery-is-separate-from-detection", 7.9, 2);
        recovery.specific_terms = 2;
        recovery.matched_specific = 1;
        let kept: Vec<String> = select_hits(vec![resume_works, remine, recovery], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["swarm-resume-works-now"]);

        let mut golden = named_hit("golden-engine-is-the-law", 14.8, 4, 3);
        golden.matched_terms = 4;
        golden.specific_terms = 2;
        golden.matched_specific = 2;
        golden.named = true;
        golden.topic_in_name = true;
        let mut scoring = covering_hit("score-serially-hermetically-advertised-port", 8.6, 2);
        scoring.specific_terms = 2;
        scoring.matched_specific = 2;
        let kept: Vec<String> = select_hits(vec![golden, scoring], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec![
                "golden-engine-is-the-law",
                "score-serially-hermetically-advertised-port"
            ]
        );

        let mut notarization = named_hit("macos-notarization-setup", 20.5, 5, 2);
        notarization.matched_terms = 5;
        notarization.named = true;
        notarization.topic_in_name = true;
        let mut shipping = named_hit("swarm-shipping-phases", 14.1, 4, 2);
        shipping.matched_terms = 4;
        shipping.named = true;
        let mut verify = named_hit("swarm-verify-in-the-running-app", 12.2, 4, 2);
        verify.matched_terms = 4;
        verify.named = true;
        let kept: Vec<String> = select_hits(vec![notarization, shipping, verify], 5)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["macos-notarization-setup"]);

        let mut kill_pids = named_hit("kill-pids-never-killpg", 24.1, 5, 3);
        kill_pids.matched_terms = 6;
        kill_pids.named = true;
        kill_pids.topic_in_name = true;
        let mut launchd = named_hit("launch-longlived-apps-via-launchd", 18.1, 4, 4);
        launchd.matched_terms = 5;
        launchd.named = true;
        launchd.topic_in_name = true;
        let kept: Vec<String> = select_hits(vec![kill_pids, launchd], 6)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec![
                "kill-pids-never-killpg",
                "launch-longlived-apps-via-launchd"
            ]
        );

        let mut scoring = named_hit("score-serially-hermetically-advertised-port", 19.1, 5, 3);
        scoring.matched_terms = 5;
        scoring.named = true;
        let mut harness = covering_hit("forge-live-harness-project", 9.1, 1);
        harness.name_terms = 1;
        let kept: Vec<String> = select_hits(vec![scoring, harness], 9)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["score-serially-hermetically-advertised-port"]);
    }

    #[test]
    fn a_named_hit_beside_the_whole_request_s_entry_rides_only_on_the_topic_word() {
        let mut ask_first = named_hit("ask-before-client-prod-config", 24.3, 9, 4);
        ask_first.matched_terms = 9;
        ask_first.specific_terms = 5;
        ask_first.matched_specific = 5;
        ask_first.named = true;
        let mut sandbox_groups = named_hit("cloud-sandbox-shares-groups-with-prod", 15.3, 5, 2);
        sandbox_groups.matched_terms = 5;
        sandbox_groups.specific_terms = 5;
        sandbox_groups.matched_specific = 4;
        sandbox_groups.named = true;
        let kept: Vec<String> = select_hits(vec![ask_first, sandbox_groups], 9)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(kept, vec!["ask-before-client-prod-config"]);

        let mut fix_loop = named_hit("evolve-goose-test-loop", 8.0, 3, 2);
        fix_loop.matched_terms = 3;
        fix_loop.named = true;
        let mut test_sooner = named_hit("test-sooner-before-runs", 8.0, 3, 2);
        test_sooner.matched_terms = 3;
        test_sooner.named = true;
        let kept: Vec<String> = select_hits(vec![fix_loop, test_sooner], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec!["evolve-goose-test-loop", "test-sooner-before-runs"],
            "no entry carries the whole request: two symmetric names ride together"
        );

        let mut kill_pids = named_hit("kill-pids-never-killpg", 24.1, 5, 3);
        kill_pids.matched_terms = 6;
        kill_pids.named = true;
        kill_pids.topic_in_name = true;
        let mut launchd = named_hit("launch-longlived-apps-via-launchd", 18.1, 4, 4);
        launchd.matched_terms = 5;
        launchd.named = true;
        launchd.topic_in_name = true;
        let kept: Vec<String> = select_hits(vec![kill_pids, launchd], 6)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            kept,
            vec![
                "kill-pids-never-killpg",
                "launch-longlived-apps-via-launchd"
            ],
            "the partial name carries the topic word (a tied one): it rides"
        );
    }

    #[test]
    fn relevant_skills_need_a_rare_term_and_rank_name_matches_first() {
        let skills = vec![
            skill(
                "jira-api",
                "Jira Cloud REST API v3 integration from external apps",
            ),
            skill(
                "leanzero-newsroom",
                "Research and draft articles about local models and Atlassian tools",
            ),
            skill(
                "tenant-a",
                "Tenant A Atlassian operations: Jira issue triage over the REST API",
            ),
            skill("tenant-b", "Tenant B Atlassian operations (Jira + JSM)"),
            skill(
                "tenant-c",
                "Tenant C Atlassian operations (Jira Cloud and Data Center)",
            ),
        ];
        let terms = query_terms("create a jira issue through the rest api");
        let mut names: Vec<&str> = relevant_skills(&skills, &terms)
            .into_iter()
            .map(|s| s.name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["jira-api", "tenant-a"],
            "'jira' is in four of five skills and weighs little; rest/api/issue decide"
        );
        assert!(relevant_skills(&skills, &query_terms("bake bread")).is_empty());
        assert!(
            relevant_skills(&skills, &query_terms("atlassian jira")).is_empty(),
            "words most skills share must not suggest any of them"
        );
    }

    #[test]
    fn past_session_is_the_newest_message_that_covers_the_request() {
        use crate::session::chat_history_search::{ChatRecallMessage, ChatRecallResult};
        let at = |h: u32| chrono::Utc::now() - chrono::Duration::hours(i64::from(h));
        let message = |role: &str, content: &str, h: u32| ChatRecallMessage {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: at(h),
        };
        let results = vec![
            ChatRecallResult {
                session_id: "s-old".to_string(),
                session_description: "vendor port".to_string(),
                session_working_dir: "/tmp".to_string(),
                last_activity: at(50),
                total_messages_in_session: 2,
                messages: vec![message(
                    "user",
                    "which port does the bench vendor answer on?\nsecond line",
                    50,
                )],
            },
            ChatRecallResult {
                session_id: "s-new".to_string(),
                session_description: "unrelated".to_string(),
                session_working_dir: "/tmp".to_string(),
                last_activity: at(1),
                total_messages_in_session: 2,
                messages: vec![message("assistant", "the port is closed", 1)],
            },
        ];
        let terms = query_terms("Which port does the bench vendor answer on?");
        let past = select_past_session(&results, &terms).unwrap();
        assert_eq!(
            past.session_id, "s-old",
            "one shared word ('port') does not cover the request"
        );
        assert_eq!(past.role, "user");
        assert_eq!(past.headline, "which port does the bench vendor answer on?");
        let block = render(&[], &[], Some(&past)).unwrap();
        assert!(block.starts_with(
            "<recall-line>recalled: past session s-old</recall-line>\n<past-session>"
        ));
        assert!(block.contains("session s-old (\"vendor port\","));
        assert!(select_past_session(&results, &query_terms("bake bread")).is_none());
    }

    #[test]
    fn skill_keywords_reach_a_skill_the_description_would_miss() {
        let mut jql = skill("jira-api", "Atlassian Jira Cloud REST API v3 integration");
        jql.properties.insert(
            "keywords".to_string(),
            serde_json::json!(["jql", "issue search", "webhooks"]),
        );
        let skills = vec![
            jql,
            skill("note-d90573", "Tenant A Atlassian operations"),
            skill("note-6799ed", "Tenant C Atlassian operations"),
        ];
        let names: Vec<&str> = relevant_skills(&skills, &query_terms("write a jql query"))
            .into_iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["jira-api"]);
        assert_eq!(skill_keywords(&skills[0]), "jql issue search webhooks");
    }

    #[test]
    fn recall_line_names_what_rode_along_and_is_extractable() {
        let hits = [hit("postgres", 2.0, 1)];
        let skills = [skill("jira-api", "Jira REST")];
        let refs: Vec<&SourceEntry> = skills.iter().collect();
        let block = render(&hits, &refs, None).unwrap();
        assert!(block.starts_with(
            "<recall-line>recalled: memories postgres · skills jira-api</recall-line>\n"
        ));
        assert_eq!(
            recall_line_of(&block),
            Some("recalled: memories postgres · skills jira-api")
        );
        assert_eq!(
            recall_line_of("<turn-context>\n<current-time>x</current-time>"),
            None
        );
    }

    #[test]
    fn corrections_are_read_at_the_head_of_the_message_and_graded() {
        assert_eq!(
            correction_strength("No, don't touch the scheduler."),
            Some(Correction::Strong)
        );
        assert_eq!(
            correction_strength("That's not what I asked for."),
            Some(Correction::Strong)
        );
        assert_eq!(
            correction_strength("Wrong file — I said the CLI one."),
            Some(Correction::Strong)
        );
        assert_eq!(
            correction_strength("Why did you delete the tests?"),
            Some(Correction::Weak)
        );
        assert_eq!(
            correction_strength("No, the other one."),
            Some(Correction::Weak)
        );
        assert_eq!(
            correction_strength("Add a flag so users can say no to telemetry, and instead log it."),
            None
        );
        assert_eq!(
            correction_strength("List the files in this directory."),
            None
        );
        assert_eq!(
            correction_strength("Why is the sky blue?"),
            Some(Correction::Weak),
            "a weak marker only nudges, never writes"
        );
        let (content, tags) = correction_memory(
            "No — don't use the shell for reading files here.",
            "Ran cat on queries.txt",
        );
        assert!(content.starts_with("Correction: No — don't use the shell for reading files here."));
        assert!(content.contains("Said after goose did: Ran cat on queries.txt"));
        assert_eq!(tags, vec!["feedback", "correction"]);
    }

    #[test]
    fn a_skill_matched_only_by_its_description_must_cover_the_whole_request() {
        let skills = vec![
            skill(
                "tenant-a",
                "Tenant A Atlassian operations: Jira issue triage over the REST API",
            ),
            skill(
                "leanzero-tutorial",
                "Author a tutorial or blog post for the website",
            ),
        ];
        let names: Vec<&str> = relevant_skills(
            &skills,
            &query_terms("set up my scratchpad for a refactor of the rendering"),
        )
        .into_iter()
        .map(|s| s.name.as_str())
        .collect();
        assert!(names.is_empty(), "no skill is about this: {names:?}");
        let names: Vec<&str> = relevant_skills(&skills, &query_terms("write a blog post"))
            .into_iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["leanzero-tutorial"],
            "the description covers the whole request"
        );
    }

    #[test]
    fn a_nameless_whole_request_body_rides_only_when_it_says_two_of_the_words_together() {
        let mut plans = named_hit("plans-overview-before-after-first", 6.6, 2, 2);
        plans.named = true;
        let mut do_all = named_hit("do-all-of-it-never-defer", 5.8, 3, 0);
        do_all.matched_terms = 3;
        do_all.together = false;
        let mut qwen = named_hit("local-qwen-swarm-agent", 5.8, 3, 0);
        qwen.matched_terms = 3;
        qwen.together = false;
        let recalled: Vec<String> = select_hits(vec![plans, do_all, qwen], 3)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            recalled,
            vec!["plans-overview-before-after-first"],
            "open/plan/present each alone in a long body is vocabulary, not the request"
        );

        let mut forge = named_hit("forge-live-ui-testing-facts", 13.3, 3, 3);
        forge.named = true;
        forge.topic_in_name = true;
        let mut bank = named_hit("bank-agent-three-bucket-rule", 8.9, 4, 0);
        bank.matched_terms = 4;
        bank.together = true;
        let recalled: Vec<String> = select_hits(vec![forge, bank], 4)
            .into_iter()
            .map(|h| h.entry.category)
            .collect();
        assert_eq!(
            recalled,
            vec![
                "forge-live-ui-testing-facts",
                "bank-agent-three-bucket-rule"
            ],
            "'Forge deploy' in the bucket list says the request's words together"
        );
    }

    #[test]
    fn a_tenant_desk_is_not_suggested_on_a_request_that_names_no_tenant() {
        let skills = vec![
            skill(
                "alterdomus",
                "Alter Domus Atlassian operations across alterdomus.atlassian.net (PRODUCTION) and alterdomus-sandbox.atlassian.net. Use whenever the task involves an Alter Domus ticket (ET- on production; AHUB- on the sandbox), the Altomata Forge automations app, or any REST automation against them.",
            ),
            skill(
                "bankofireland",
                "Bank of Ireland Atlassian engagement — Forge app development, Forge deployment/approval. Every change to a production system goes through the bank's change control.",
            ),
            skill(
                "siemens",
                "Siemens Atlassian operations on se-dps.atlassian.net (PRODUCTION CLIENT). Four sandboxes exist; rehearse in staging.",
            ),
            skill(
                "goose-swarm-campaign",
                "Run a goose-local-edition swarm build end to end. Use when the user wants to start / watch / kill / measure a swarm run or benchmark unit.",
            ),
            skill(
                "leanzero-tutorial",
                "Author a tutorial for the website. Use when the user asks to write a tutorial or blog post.",
            ),
            skill(
                "goose-clean",
                "Reclaim disk by cleaning the goose checkout's build caches.",
            ),
        ];
        let names = |request: &str| -> Vec<String> {
            relevant_skills(&skills, &query_terms(request))
                .into_iter()
                .map(|s| s.name.clone())
                .collect()
        };
        assert!(
            names("Deploy the app to the sandbox first, then production.").is_empty(),
            "sandbox, production and app sit apart in every desk's description"
        );
        assert_eq!(
            names("How do I start a benchmark run properly?"),
            vec!["goose-swarm-campaign"],
            "'a swarm run or benchmark unit': one function word between is together"
        );
        assert_eq!(
            names("Write a blog post about local models."),
            vec!["leanzero-tutorial"],
            "'blog post'"
        );
        assert_eq!(
            names("Deploy the Forge app to production."),
            vec!["bankofireland"],
            "a desk whose description says 'Forge app' and 'Forge deployment' is still suggested"
        );
        let hits = skill_hits(
            &skills,
            &query_terms("Deploy the app to the sandbox first, then production."),
        );
        let alterdomus = hits.iter().find(|h| h.skill.name == "alterdomus").unwrap();
        assert_eq!(
            (
                alterdomus.matched_terms,
                alterdomus.together,
                alterdomus.about
            ),
            (3, false, false),
            "{alterdomus:?}"
        );
    }

    #[test]
    fn a_family_word_in_a_skill_name_does_not_name_the_skill() {
        let skills = vec![
            skill(
                "jira-api-skill",
                "Atlassian Jira Cloud REST API v3 integration — issues, JQL search, OAuth / API-token auth.",
            ),
            skill(
                "confluence-api-skill",
                "Atlassian Confluence Cloud REST API v2 integration — pages, blogposts, API-token auth.",
            ),
            skill(
                "atlassian-organizations-api-skill",
                "Atlassian Organizations REST API. Use when managing organizations, users and groups.",
            ),
            skill(
                "web-search",
                "Search the web using DuckDuckGo (no API key required), Tavily, or SearXNG.",
            ),
            skill(
                "goose-benchmark-iteration",
                "Iterate the LeanZero agentic benchmark (sb-N tiers) — build a new tier, fix a scorer.",
            ),
            skill(
                "goose-clean",
                "Reclaim disk by cleaning the goose checkout's build caches at ~/Projects/goose.",
            ),
            skill("tenant-a", "Tenant A Atlassian operations over the REST API."),
            skill("tenant-b", "Tenant B Atlassian operations over the REST API."),
        ];
        let names = |request: &str| -> Vec<String> {
            relevant_skills(&skills, &query_terms(request))
                .into_iter()
                .map(|s| s.name.clone())
                .collect()
        };
        assert!(
            names("Should I remind him to rotate the API key I was just given?").is_empty(),
            "'api' sits in three names and most descriptions; it names none of them"
        );
        assert_eq!(
            names("How do I start a benchmark run properly?"),
            vec!["goose-benchmark-iteration"],
            "'benchmark' is that skill's own name word: one rare term suffices"
        );
        assert_eq!(
            names("Which git identity do goose commits use?"),
            Vec::<String>::new(),
            "'goose' is a family word: goose-clean matches it and nothing else"
        );
        let hits = skill_hits(&skills, &query_terms("rotate the api key"));
        let jira = hits
            .iter()
            .find(|h| h.skill.name == "jira-api-skill")
            .unwrap();
        assert_eq!((jira.name_terms, jira.own_name_terms), (1, 0), "{jira:?}");
    }

    #[test]
    fn an_open_question_is_the_assistant_s_last_line_when_it_asks() {
        assert_eq!(
            open_question("I found two configs.\nWhich one should I edit, dev or prod?"),
            Some("Which one should I edit, dev or prod?".to_string())
        );
        assert_eq!(
            open_question("I'm not sure which port the vendor uses; I will assume 8850"),
            Some("I'm not sure which port the vendor uses; I will assume 8850".to_string())
        );
        assert_eq!(open_question("Done. The tests pass."), None);
    }

    #[test]
    fn previous_assistant_text_is_what_the_user_reacts_to() {
        let messages = vec![
            Message::user().with_text("first request"),
            Message::assistant().with_text("Which config, dev or prod?"),
            Message::user().with_text("prod"),
        ];
        assert_eq!(
            previous_assistant_text(&messages).as_deref(),
            Some("Which config, dev or prod?")
        );
        let two_requests = vec![
            Message::assistant().with_text("earlier answer"),
            Message::user().with_text("a request the assistant has not answered"),
            Message::user().with_text("another request"),
        ];
        assert_eq!(previous_assistant_text(&two_requests), None);
        assert_eq!(
            previous_assistant_text(&[Message::user().with_text("hello")]),
            None
        );

        let mut args = serde_json::Map::new();
        args.insert(
            "command".to_string(),
            serde_json::json!("head -2 queries.txt"),
        );
        let with_tool = vec![
            Message::user().with_text("show me the file"),
            Message::assistant().with_tool_request(
                "call-1",
                Ok(rmcp::model::CallToolRequestParams::new("shell").with_arguments(args)),
            ),
            Message::user().with_tool_response(
                "call-1",
                Ok(CallToolResult::success(vec![Content::text("line 1")])),
            ),
            Message::assistant().with_text("Here is the first line. What next?"),
            Message::user().with_text("No — never use the shell here."),
        ];
        let action = previous_assistant_text(&with_tool).unwrap();
        assert_eq!(
            action,
            "tool shell({\"command\":\"head -2 queries.txt\"})\nHere is the first line. What next?",
            "the whole previous turn: the call first, then the closing words"
        );
    }

    #[test]
    fn autoload_needs_two_name_terms_and_a_body_within_budget() {
        let mut small = skill("jira-api", "Jira REST");
        small.content = "x".repeat(1_000);
        let mut big = skill("jira-api-big", "Jira REST");
        big.content = "x".repeat(100_000);
        assert_eq!(
            autoload_pick(&[(2, &small)], 262_144).map(|s| s.name.as_str()),
            Some("jira-api")
        );
        assert!(
            autoload_pick(&[(1, &small)], 262_144).is_none(),
            "one name term is a hint, not a pick"
        );
        assert!(
            autoload_pick(&[(2, &big)], 262_144).is_none(),
            "100k chars exceeds a 32k budget"
        );
        assert!(
            autoload_pick(&[(2, &small)], 4_000).is_none(),
            "a 4k-token window has a 500-char budget"
        );
    }

    #[test]
    fn extras_render_their_sections_and_the_line() {
        let extras = Extras {
            autoloaded: Some(("jira-api".to_string(), "BODY".to_string())),
            correction_of: Some("Deleted the tests".to_string()),
            answered: Some(("Which config?".to_string(), "prod".to_string())),
        };
        let out = render_with(&[], &[], None, &extras).unwrap();
        assert!(out.starts_with("<recall-line>recalled:  · loaded jira-api · correction noticed · answer noticed</recall-line>"), "{out}");
        assert!(out.contains("<loaded-skill name=\"jira-api\">"));
        assert!(out.contains("correction of what you just did (\"Deleted the tests\")"));
        assert!(out.contains("You asked \"Which config?\" and the user answered \"prod\""));
    }

    #[test]
    fn render_says_nothing_when_nothing_matched() {
        assert_eq!(render(&[], &[], None), None);
        let hits = [hit("postgres", 2.0, 1)];
        let skills = [skill("jira-api", "Jira REST")];
        let refs: Vec<&SourceEntry> = skills.iter().collect();
        let out = render(&hits, &refs, None).unwrap();
        assert!(out.starts_with("<recall-line>"));
        assert!(out.contains("\n<recalled-memories>\n"));
        assert!(out.contains("## postgres (global [project])\npostgres headline\nbody"));
        assert!(out.contains("<relevant-skills>\n"));
        assert!(out.contains("- jira-api: Jira REST"));
        assert!(out.ends_with("</relevant-skills>"));
    }
}
