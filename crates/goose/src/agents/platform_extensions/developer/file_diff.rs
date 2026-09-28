//! What a file write or edit changed, for the PERSON (Q-189/Q-190).
//!
//! A successful `write`/`edit` result carries its unified diff in the RESULT's `_meta`
//! (`goose/fileDiff`), never as a content item. Every path that reaches a model reads `content`:
//! the provider formatters, and — measured 2026-09-27 — seven that ignore content audience
//! annotations altogether (compaction summaries, the turn judge, the swarm judge and its repeat hash,
//! create_recipe, the plan reasoner, the orchestrator). A user-only content item would have leaked
//! into all seven; `_meta` is read by none of them. The ACP server forwards it to the client as
//! `_meta.goose.fileDiff` on the tool-call update (live and on replay), where the desktop draws it on
//! the tool card and in the session's Changes rail.

use std::time::{Duration, Instant};

use rmcp::model::{CallToolResult, Meta};
use serde_json::json;
use similar::{Algorithm, ChangeTag, TextDiff};

/// The result `_meta` key that carries a file diff.
pub const FILE_DIFF_META_KEY: &str = "goose/fileDiff";

/// Lines of unchanged context around each hunk — the unified-diff convention (`diff -u`).
const CONTEXT_LINES: usize = 3;

/// How long the diff may search for the MINIMAL script before settling for a correct coarser one.
/// A person-perception budget for a display artifact — it bounds no model work and no content:
/// past it `similar` still returns a valid script (the rest of the region as removed + added).
const DISPLAY_BUDGET: Duration = Duration::from_millis(250);

/// What the file held before the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Before {
    /// The file existed and was read as text: the diff is against it.
    File,
    /// The file did not exist: every line is an addition.
    None,
    /// The file existed but was not UTF-8 text: the diff shows the new text as additions and
    /// says so, rather than pretending the file was created.
    Unreadable,
}

impl Before {
    fn as_str(self) -> &'static str {
        match self {
            Before::File => "file",
            Before::None => "none",
            Before::Unreadable => "unreadable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub unified: String,
    pub added: usize,
    pub removed: usize,
}

/// The unified diff of `old` → `new`. `old` is `None` when there was no readable "before"; the old
/// header is then `/dev/null` and the `before` marker says whether the file was absent or
/// unreadable.
///
/// Patience keeps a rewritten block as one hunk instead of interleaving it with shared braces and
/// blank lines.
pub fn diff_texts(path: &str, old: Option<&str>, new: &str) -> FileDiff {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .deadline(Instant::now() + DISPLAY_BUDGET)
        .diff_lines(old.unwrap_or(""), new);

    let mut added = 0;
    let mut removed = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => {}
        }
    }

    let old_header = if old.is_some() { path } else { "/dev/null" };
    let unified = diff
        .unified_diff()
        .context_radius(CONTEXT_LINES)
        .header(old_header, path)
        .to_string();

    FileDiff {
        unified,
        added,
        removed,
    }
}

/// `result` with the diff in its `_meta`. The content — what the model reads — is untouched.
pub fn with_file_diff(
    mut result: CallToolResult,
    path: &str,
    before: Before,
    diff: &FileDiff,
) -> CallToolResult {
    let meta = result.meta.get_or_insert_with(Meta::new);
    meta.0.insert(
        FILE_DIFF_META_KEY.to_string(),
        json!({
            "path": path,
            "before": before.as_str(),
            "added": diff.added,
            "removed": diff.removed,
            "unified": diff.unified,
        }),
    );
    result
}

/// One file a conversation's `write`/`edit` calls changed, summed over its calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenFile {
    pub path: String,
    /// The first call that wrote it found no file there.
    pub created: bool,
    pub added: u64,
    pub removed: u64,
    pub edits: usize,
}

/// Every file the conversation's `write`/`edit` calls changed, in first-touched order, from the diff
/// each result carries in `_meta` — the Rust half of the desktop's `sessionChanges`
/// (changes/fileDiff.ts). A shell command that changed files leaves no diff and is not listed.
pub fn written_files(messages: &[crate::conversation::message::Message]) -> Vec<WrittenFile> {
    use crate::conversation::message::MessageContent;

    let writers: Vec<&str> = messages
        .iter()
        .flat_map(|message| message.content.iter())
        .filter_map(|content| match content {
            MessageContent::ToolRequest(request)
                if request
                    .tool_call
                    .as_ref()
                    .is_ok_and(|call| matches!(call.name.as_ref(), "write" | "edit")) =>
            {
                Some(request.id.as_str())
            }
            _ => None,
        })
        .collect();
    let mut files: Vec<WrittenFile> = Vec::new();
    for content in messages.iter().flat_map(|message| message.content.iter()) {
        let MessageContent::ToolResponse(response) = content else {
            continue;
        };
        if !writers.contains(&response.id.as_str()) {
            continue;
        }
        let Some(diff) = response
            .tool_result
            .as_ref()
            .ok()
            .and_then(|result| result.meta.as_ref())
            .and_then(|meta| meta.0.get(FILE_DIFF_META_KEY))
        else {
            continue;
        };
        let Some(path) = diff.get("path").and_then(|path| path.as_str()) else {
            continue;
        };
        let count = |key: &str| diff.get(key).and_then(serde_json::Value::as_u64);
        let (Some(added), Some(removed)) = (count("added"), count("removed")) else {
            continue;
        };
        match files.iter_mut().find(|file| file.path == path) {
            Some(file) => {
                file.added += added;
                file.removed += removed;
                file.edits += 1;
            }
            None => files.push(WrittenFile {
                path: path.to_string(),
                created: diff.get("before").and_then(|b| b.as_str()) == Some(Before::None.as_str()),
                added,
                removed,
                edits: 1,
            }),
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;

    #[test]
    fn an_edit_is_a_hunk_with_real_line_numbers() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nd\nE1\nE2\nf\ng\nh\n";
        let diff = diff_texts("/w/notes.md", Some(old), new);
        assert_eq!(diff.added, 2);
        assert_eq!(diff.removed, 1);
        assert_eq!(
            diff.unified,
            "--- /w/notes.md\n+++ /w/notes.md\n@@ -2,7 +2,8 @@\n b\n c\n d\n-e\n+E1\n+E2\n f\n g\n h\n"
        );
    }

    #[test]
    fn a_create_is_all_additions_against_dev_null() {
        let diff = diff_texts("/w/new.md", None, "one\ntwo\n");
        assert_eq!((diff.added, diff.removed), (2, 0));
        assert!(diff
            .unified
            .starts_with("--- /dev/null\n+++ /w/new.md\n@@ -0,0 +1,2 @@\n"));
        assert!(diff.unified.ends_with("+one\n+two\n"));
    }

    #[test]
    fn an_unchanged_write_has_no_hunks() {
        let diff = diff_texts("/w/same.md", Some("x\n"), "x\n");
        assert_eq!((diff.added, diff.removed), (0, 0));
        assert!(!diff.unified.contains("@@"));
    }

    #[test]
    fn a_missing_trailing_newline_is_marked() {
        let diff = diff_texts("/w/f", Some("a\n"), "a\nb");
        assert_eq!((diff.added, diff.removed), (1, 0));
        assert!(diff.unified.contains("\\ No newline at end of file"));
    }

    #[test]
    fn the_diff_rides_meta_and_leaves_content_alone() {
        let content = vec![Content::text("Edited /w/f (1 lines -> 1 lines)").with_priority(0.0)];
        let diff = diff_texts("/w/f", Some("a\n"), "b\n");
        let result = with_file_diff(
            CallToolResult::success(content.clone()),
            "/w/f",
            Before::File,
            &diff,
        );
        assert_eq!(result.content, content);
        assert_eq!(
            result.meta.as_ref().and_then(|m| m.get(FILE_DIFF_META_KEY)),
            Some(&json!({
                "path": "/w/f",
                "before": "file",
                "added": 1,
                "removed": 1,
                "unified": "--- /w/f\n+++ /w/f\n@@ -1 +1 @@\n-a\n+b\n",
            }))
        );
    }
}
