//! The proposal store (frame 1.14, event B and the session half of event A): a memory or a
//! knowledge piece the agent ASKED about and nobody has answered yet. One JSON file per key under
//! `<config>/proposals/` — deliberately NOT under the memory directory, which the desktop's
//! memories view walks as if every file there were a category. A proposal becomes a memory only
//! through [`crate::MemoryStore::remember`] when a human clicks Save; declined ones stay on disk as
//! the record of what was asked, so the same lesson is never proposed twice in a row.
//!
//! Keys: an ACP session id — the end-of-turn assessment's, and the memory extension's
//! `propose_knowledge` reads it from the tool call's `agent-session-id` meta — or
//! [`working_dir_key`] for a proposal filed with no session known (and every knowledge proposal
//! filed before Q-82, when a card proposed in one chat showed in every chat of the project). The
//! desktop lists both for a session and says where a working-dir one came from.
//!
//! This crate owns the format so the MCP process and the ACP server write ONE shape.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Open proposals a key may hold; the fourth is REFUSED, never queued — more than three
/// unanswered cards means the user is not answering them (frame 1.14 §3.1 caps).
pub const MAX_OPEN_PROPOSALS_PER_KEY: usize = 3;
/// A proposed memory's text is generated, so it is clamped tighter than what a human types.
pub const PROPOSAL_TEXT_MAX_CHARS: usize = 350;
pub const PROPOSAL_WHY_MAX_CHARS: usize = 200;
/// Days an unanswered proposal stays listable; after that it renders as expired and is dropped.
pub const PROPOSAL_TTL_SECS: u64 = 14 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalKind {
    Memory,
    Knowledge,
}

/// The agent's judgement of the turn the memory came from. Judged by the model — never by a word
/// list over the user's prose (frame 1.14 LAW 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalState {
    Open,
    Saved,
    Declined,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub id: String,
    pub kind: ProposalKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarity: Option<Polarity>,
    /// The memory text as proposed; the user may edit it before saving.
    pub text: String,
    /// The one-sentence reason the agent thinks it is worth keeping.
    #[serde(default)]
    pub why: String,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_global: bool,
    /// Knowledge only: the lookups that grounded it.
    #[serde(default)]
    pub sources: Vec<String>,
    pub created_at: u64,
    pub state: ProposalState,
}

/// What `ProposalStore::add` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposeOutcome {
    Added,
    /// The same text is already proposed (open) or was already answered under this key.
    Duplicate,
    /// The key already holds [`MAX_OPEN_PROPOSALS_PER_KEY`] open proposals.
    Refused,
}

pub struct ProposalStore {
    dir: PathBuf,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The key for a proposal filed by a process that knows only its working directory.
pub fn working_dir_key(working_dir: &Path) -> String {
    let text = working_dir.to_string_lossy();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("wd-{hash:016x}")
}

fn clamp_chars(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

fn safe_key(key: &str) -> io::Result<String> {
    let ok = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(key.to_string())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("proposal key {key:?} is not a session id or a working-dir key"),
        ))
    }
}

impl ProposalStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, key: &str) -> io::Result<PathBuf> {
        Ok(self.dir.join(format!("{}.json", safe_key(key)?)))
    }

    fn read(&self, key: &str) -> io::Result<Vec<MemoryProposal>> {
        let path = self.file(key)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write(&self, key: &str, rows: &[MemoryProposal]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(rows)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(self.file(key)?, text)
    }

    /// Every proposal under the key, with open ones past the TTL flipped to expired.
    pub fn list(&self, key: &str) -> io::Result<Vec<MemoryProposal>> {
        let now = now_secs();
        let mut rows = self.read(key)?;
        for row in &mut rows {
            if row.state == ProposalState::Open
                && now.saturating_sub(row.created_at) > PROPOSAL_TTL_SECS
            {
                row.state = ProposalState::Expired;
            }
        }
        Ok(rows)
    }

    pub fn open_count(&self, key: &str) -> io::Result<usize> {
        Ok(self
            .list(key)?
            .iter()
            .filter(|p| p.state == ProposalState::Open)
            .count())
    }

    /// File a proposal. Text and reason are clamped here, at the one writer. A duplicate text
    /// under the key is not raised again; a full key REFUSES rather than queues.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        key: &str,
        kind: ProposalKind,
        polarity: Option<Polarity>,
        text: &str,
        why: &str,
        category: &str,
        tags: &[String],
        is_global: bool,
        sources: &[String],
    ) -> io::Result<ProposeOutcome> {
        let text = clamp_chars(text, PROPOSAL_TEXT_MAX_CHARS);
        if text.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a proposal needs text",
            ));
        }
        crate::validate_category(category)?;
        let mut rows = self.list(key)?;
        if rows.iter().any(|p| p.text == text) {
            return Ok(ProposeOutcome::Duplicate);
        }
        if rows
            .iter()
            .filter(|p| p.state == ProposalState::Open)
            .count()
            >= MAX_OPEN_PROPOSALS_PER_KEY
        {
            return Ok(ProposeOutcome::Refused);
        }
        let created_at = now_secs();
        rows.push(MemoryProposal {
            id: format!("p-{created_at}-{}", rows.len() + 1),
            kind,
            polarity,
            text,
            why: clamp_chars(why, PROPOSAL_WHY_MAX_CHARS),
            category: category.to_string(),
            tags: tags.to_vec(),
            is_global,
            sources: sources.to_vec(),
            created_at,
            state: ProposalState::Open,
        });
        self.write(key, &rows)?;
        Ok(ProposeOutcome::Added)
    }

    /// Record the human's answer. The edited text (if any) is re-clamped here. Returns the row as
    /// answered, or None when the id is unknown or the proposal is no longer open.
    pub fn answer(
        &self,
        key: &str,
        id: &str,
        saved: bool,
        edited_text: Option<&str>,
    ) -> io::Result<Option<MemoryProposal>> {
        let mut rows = self.list(key)?;
        let Some(row) = rows
            .iter_mut()
            .find(|p| p.id == id && p.state == ProposalState::Open)
        else {
            return Ok(None);
        };
        if let Some(text) = edited_text {
            let text = clamp_chars(text, PROPOSAL_TEXT_MAX_CHARS);
            if !text.is_empty() {
                row.text = text;
            }
        }
        row.state = if saved {
            ProposalState::Saved
        } else {
            ProposalState::Declined
        };
        let answered = row.clone();
        self.write(key, &rows)?;
        Ok(Some(answered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ProposalStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProposalStore::new(dir.path().join("proposals"));
        (dir, store)
    }

    #[test]
    fn a_proposal_is_filed_listed_and_answered() {
        let (_d, store) = store();
        let out = store
            .add(
                "s1",
                ProposalKind::Memory,
                Some(Polarity::Positive),
                "  run tests with X  ",
                "why",
                "workflow",
                &[],
                false,
                &[],
            )
            .unwrap();
        assert_eq!(out, ProposeOutcome::Added);
        let rows = store.list("s1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "run tests with X");
        assert_eq!(rows[0].state, ProposalState::Open);
        let answered = store
            .answer(
                "s1",
                &rows[0].id,
                true,
                Some("run tests with X, exported first"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(answered.state, ProposalState::Saved);
        assert_eq!(answered.text, "run tests with X, exported first");
        assert!(store
            .answer("s1", &rows[0].id, false, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_fourth_open_proposal_is_refused_and_a_repeat_is_a_duplicate() {
        let (_d, store) = store();
        for i in 0..MAX_OPEN_PROPOSALS_PER_KEY {
            assert_eq!(
                store
                    .add(
                        "s1",
                        ProposalKind::Memory,
                        None,
                        &format!("m{i}"),
                        "",
                        "c",
                        &[],
                        false,
                        &[]
                    )
                    .unwrap(),
                ProposeOutcome::Added
            );
        }
        assert_eq!(
            store
                .add(
                    "s1",
                    ProposalKind::Memory,
                    None,
                    "m9",
                    "",
                    "c",
                    &[],
                    false,
                    &[]
                )
                .unwrap(),
            ProposeOutcome::Refused
        );
        assert_eq!(
            store
                .add(
                    "s1",
                    ProposalKind::Memory,
                    None,
                    "m0",
                    "",
                    "c",
                    &[],
                    false,
                    &[]
                )
                .unwrap(),
            ProposeOutcome::Duplicate
        );
    }

    #[test]
    fn text_and_why_are_clamped_at_the_writer() {
        let (_d, store) = store();
        let long = "x".repeat(1000);
        store
            .add(
                "s1",
                ProposalKind::Knowledge,
                None,
                &long,
                &long,
                "c",
                &[],
                false,
                &["web-search__search".into()],
            )
            .unwrap();
        let row = &store.list("s1").unwrap()[0];
        assert_eq!(row.text.chars().count(), PROPOSAL_TEXT_MAX_CHARS);
        assert_eq!(row.why.chars().count(), PROPOSAL_WHY_MAX_CHARS);
        assert_eq!(row.sources, vec!["web-search__search".to_string()]);
    }

    #[test]
    fn a_key_that_is_not_an_id_is_refused_and_the_wd_key_is_stable() {
        let (_d, store) = store();
        assert!(store.list("../etc").is_err());
        let a = working_dir_key(Path::new("/tmp/project"));
        assert_eq!(a, working_dir_key(Path::new("/tmp/project")));
        assert_ne!(a, working_dir_key(Path::new("/tmp/other")));
        assert!(store.list(&a).unwrap().is_empty());
    }
}
