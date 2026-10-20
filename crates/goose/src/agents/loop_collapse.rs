//! I-28 review: a reply the engine's loop guard stopped keeps ONE copy of the repeated text in the
//! history. The guard can let up to 4 x 1024 tokens of exact repeats through (plus the reasoning);
//! left in the history they bloat every later request and show the model the very loop the nudge
//! tells it not to repeat.
//!
//! A cloud model has no engine guard: Grok 4.7 on OpenRouter (Forge 2.0, 2026-10) wrote one or two
//! sentences again and again until its 65,536-token output cap, four times in one build (286-325k
//! characters each, every one an exact repeat after the first ~300 characters), and all four stayed
//! in the history. [`RunawayWatch`] stops such a stream on goose's side.

use goose_providers::conversation::message::{Message, MessageContent};
use rmcp::model::Role;

/// The shortest repeated unit worth collapsing, in characters: a stutter is the model's own text.
const MIN_PERIOD_CHARS: usize = 16;
/// Copies of a unit before it counts as a loop (the engine's long-period rule stops at 4).
const MIN_COPIES: usize = 3;

/// `messages` with every assistant text and thinking block that ENDS in a run of `MIN_COPIES` or
/// more exact copies of one unit cut back to a single copy and a note. The `stamp` the formatter
/// appended after the loop stays where it was.
pub(crate) fn collapse_looped_reply(messages: &[Message], stamp: &str) -> Vec<Message> {
    collapse_reply_runs(messages, stamp, 0)
}

/// As [`collapse_looped_reply`], for a reply the server cut at its output limit: only a run that
/// covers at least `min_covered` characters is a runaway (a cut reply may legitimately end in a
/// few equal lines), so the default is the runaway watch's own window.
pub(crate) fn collapse_runaway_reply(
    messages: &[Message],
    stamp: &str,
    min_covered: usize,
) -> Vec<Message> {
    collapse_reply_runs(messages, stamp, min_covered.max(1))
}

fn collapse_reply_runs(messages: &[Message], stamp: &str, min_covered: usize) -> Vec<Message> {
    messages
        .iter()
        .map(|message| {
            if message.role != Role::Assistant {
                return message.clone();
            }
            let mut message = message.clone();
            for content in message.content.iter_mut() {
                match content {
                    MessageContent::Text(text) => {
                        text.text = collapse_text(&text.text, stamp, min_covered)
                    }
                    MessageContent::Thinking(thinking) => {
                        thinking.thinking = collapse_text(&thinking.thinking, stamp, min_covered)
                    }
                    _ => {}
                }
            }
            message
        })
        .collect()
}

fn collapse_text(text: &str, stamp: &str, min_covered: usize) -> String {
    let stamp = stamp.trim();
    let (head, tail) = match text.find(stamp) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let trimmed = head.trim_end();
    let spacing = head.get(trimmed.len()..).unwrap_or_default();
    match collapse_repeated_suffix(trimmed, min_covered) {
        Some(collapsed) => format!("{collapsed}{spacing}{tail}"),
        None => text.to_string(),
    }
}

/// The text with a trailing run of exact copies cut to one copy plus a note, or `None` when it
/// does not end in one. Linear: a Z-function over the reversed text finds every period at once.
/// A run shorter than `min_covered` characters is left as it is.
fn collapse_repeated_suffix(text: &str, min_covered: usize) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let reversed: Vec<char> = chars.iter().rev().copied().collect();
    let z = z_function(&reversed);
    // The smallest period is the loop's own unit: a long multiple of a stutter is still a stutter.
    let period =
        (1..=n / MIN_COPIES).find(|&p| z.get(p).is_some_and(|&lcp| lcp >= (MIN_COPIES - 1) * p))?;
    if period < MIN_PERIOD_CHARS {
        return None;
    }
    let covered = z[period] + period;
    if covered < min_covered {
        return None;
    }
    let copies = covered.div_ceil(period);
    let keep = n - covered + period;
    let kept: String = chars.get(..keep)?.iter().collect();
    Some(format!(
        "{kept}\n[goose: the text above was repeated {} more time{} here and the repeats were \
         removed]",
        copies - 1,
        if copies == 2 { "" } else { "s" }
    ))
}

/// The runaway watch's defaults: the last 16,384 characters (~4k tokens) of a streamed reply's text
/// are ONE unit of at most 2,048 characters repeated exactly, at least 8 times. Ordinary output is
/// never exactly periodic over a span that long — a table, a log or a list varies in its numbers,
/// names or values from row to row, and code varies line to line — so the watch trips only on a
/// model writing the same text over and over. Grok 4.7's four runaways (units of 105-140
/// characters) trip it ~16k characters in, where the stream otherwise ran on to 286-325k.
pub const RUNAWAY_TEXT_CHARS_DEFAULT: usize = 16_384;
pub const RUNAWAY_TEXT_COPIES_DEFAULT: usize = 8;
/// config.yaml keys: the span in characters (0 turns the watch off) and the copies of one unit it
/// must hold.
pub const RUNAWAY_TEXT_CHARS_KEY: &str = "GOOSE_RUNAWAY_TEXT_CHARS";
pub const RUNAWAY_TEXT_COPIES_KEY: &str = "GOOSE_RUNAWAY_TEXT_COPIES";

/// Watches a streamed reply's text and says when its tail has become one unit repeated: the span
/// (`chars`) is a single period of at most `chars / copies` characters. Tool-call arguments are
/// never fed to it — a file's content can repeat legitimately — only the reply's own text.
pub(crate) struct RunawayWatch {
    chars: usize,
    copies: usize,
    seen: usize,
    next_check: usize,
}

impl RunawayWatch {
    pub(crate) fn new(chars: usize, copies: usize) -> Option<Self> {
        (chars > 0 && copies >= 2).then_some(Self {
            chars,
            copies,
            seen: 0,
            next_check: chars,
        })
    }

    /// From config.yaml (or the environment), the defaults when unset; `None` when turned off.
    pub(crate) fn from_config(config: &crate::config::Config) -> Option<Self> {
        let chars = config
            .get_param::<usize>(RUNAWAY_TEXT_CHARS_KEY)
            .unwrap_or(RUNAWAY_TEXT_CHARS_DEFAULT);
        let copies = config
            .get_param::<usize>(RUNAWAY_TEXT_COPIES_KEY)
            .unwrap_or(RUNAWAY_TEXT_COPIES_DEFAULT);
        Self::new(chars, copies)
    }

    pub(crate) fn span(&self) -> usize {
        self.chars
    }

    /// Restarts the watch for the next provider call's reply.
    pub(crate) fn reset(&mut self) {
        self.seen = 0;
        self.next_check = self.chars;
    }

    /// `delta` was just appended to `text`, the reply so far: the repeated unit's length when the
    /// reply now ends in a runaway. The tail is examined once per eighth of the span of new text,
    /// so the cost stays linear in the reply.
    pub(crate) fn runaway_period(&mut self, delta: &str, text: &str) -> Option<usize> {
        self.seen += delta.chars().count();
        if self.seen < self.next_check {
            return None;
        }
        self.next_check = self.seen + (self.chars / 8).max(1);
        let start = text.char_indices().rev().nth(self.chars - 1)?.0;
        let tail: Vec<char> = text.get(start..)?.chars().collect();
        let z = z_function(&tail);
        (1..=self.chars / self.copies).find(|&p| z.get(p).is_some_and(|&lcp| lcp + p >= self.chars))
    }
}

fn z_function(s: &[char]) -> Vec<usize> {
    let n = s.len();
    let mut z = vec![0usize; n];
    if n == 0 {
        return z;
    }
    z[0] = n;
    let (mut left, mut right) = (0usize, 0usize);
    for i in 1..n {
        if i < right {
            z[i] = (right - i).min(z[i - left]);
        }
        while i + z[i] < n && s[z[i]] == s[i + z[i]] {
            z[i] += 1;
        }
        if i + z[i] > right {
            left = i;
            right = i + z[i];
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMP: &str = "\n\n[OUTPUT STOPPED: loop]";

    #[test]
    fn a_trailing_loop_keeps_one_copy() {
        let unit = "do it. First, `config.js`. I'll add the field. ";
        let text = format!("Plan.\n{}{STAMP}", unit.repeat(38));
        let collapsed = collapse_text(&text, STAMP, 0);
        assert_eq!(collapsed.matches("do it. First").count(), 1, "{collapsed}");
        assert!(collapsed.starts_with("Plan.\ndo it. First"), "{collapsed}");
        assert!(collapsed.contains("repeated 37 more times"), "{collapsed}");
        assert!(collapsed.ends_with(STAMP), "{collapsed}");
    }

    #[test]
    fn a_cut_last_copy_is_dropped_with_the_rest() {
        let unit = "x = compute(a, b); y = x + 1; ";
        let text = format!("{}{}", unit.repeat(5), unit.get(..9).unwrap());
        let collapsed = collapse_repeated_suffix(&text, 0).unwrap();
        assert_eq!(collapsed.matches("compute").count(), 1, "{collapsed}");
    }

    #[test]
    fn text_without_a_long_loop_is_unchanged() {
        assert_eq!(
            collapse_repeated_suffix("A finished answer with no loop.", 0),
            None
        );
        assert_eq!(collapse_repeated_suffix(&"ha ".repeat(50), 0), None);
        let twice = "a unit that is long enough. ".repeat(2);
        assert_eq!(collapse_repeated_suffix(&twice, 0), None);
        let text = format!("{}{STAMP}", "nothing repeats here at all, honestly.");
        assert_eq!(collapse_text(&text, STAMP, 0), text);
    }

    /// Streams `text` through a default watch in deltas of `step` characters; the character count
    /// at which it tripped and the unit length.
    fn watch(text: &str, step: usize) -> Option<(usize, usize)> {
        let mut watch =
            RunawayWatch::new(RUNAWAY_TEXT_CHARS_DEFAULT, RUNAWAY_TEXT_COPIES_DEFAULT).unwrap();
        let chars: Vec<char> = text.chars().collect();
        let mut so_far = String::new();
        for delta in chars.chunks(step) {
            let delta: String = delta.iter().collect();
            so_far.push_str(&delta);
            if let Some(period) = watch.runaway_period(&delta, &so_far) {
                return Some((so_far.chars().count(), period));
            }
        }
        None
    }

    /// Grok 4.7's message 485 (Forge 2.0 low build, 2026-10): the two opening variants, then one
    /// unit, verbatim, until the 65,536-token cap (2,728 copies, 286,727 characters).
    #[test]
    fn grok_runaway_trips_the_watch_early() {
        let unit = "I'll cap each request at 200 issues.The field write grouped by value, so one \
                    request touched 342 issues. ";
        let text = format!(
            "The field write batches by value, not by issue. I'll split each batch so no request \
             touches more than 200 issues.The field update batches by value, so one request \
             touched 342 issues. I'll split each batch at 200 issues.The field write grouped by \
             value, so one request touched 342 issues. {}",
            unit.repeat(2_728)
        );
        let (at, period) = watch(&text, 23).expect("the runaway trips the watch");
        assert_eq!(period, unit.chars().count());
        assert!(at < 20_000, "tripped at {at} of {}", text.chars().count());
    }

    /// The four Grok runaways had units of 105-140 characters; one with no space between its
    /// sentences (message 730) is the same unit to the watch.
    #[test]
    fn a_runaway_without_sentence_breaks_trips_it_too() {
        let unit = "The field values are present, so I'll run the full suite once more.";
        let (_, period) = watch(&unit.repeat(4_000), 7).unwrap();
        assert_eq!(period, unit.chars().count());
    }

    fn rust_file() -> String {
        (0..900)
            .map(|i| {
                format!(
                    "    fn handler_{i}(req: &Request) -> Result<Response> {{\n        \
                     let id = req.param(\"id_{i}\")?;\n        Ok(Response::json(&store.get(id)?))\n    }}\n\n"
                )
            })
            .collect()
    }

    fn markdown_table() -> String {
        let mut table = String::from("| issue | field | value | status |\n|---|---|---|---|\n");
        for i in 0..1_200 {
            table.push_str(&format!("| LZPT-{i} | Story Points | {} | ok |\n", i % 13));
        }
        table
    }

    fn repeated_but_legit_list() -> String {
        (1..=1_500)
            .map(|i| {
                format!("- [ ] Re-run the migration for batch {i} and confirm the field refresh.\n")
            })
            .collect()
    }

    fn log_lines() -> String {
        (0..1_000)
            .map(|i| {
                format!(
                    "2026-10-20T10:{:02}:{:02}Z INFO request completed status=200 rows=200\n",
                    i / 60 % 60,
                    i % 60
                )
            })
            .collect()
    }

    fn csv_rows() -> String {
        (0..3_000).map(|i| format!("{i},0,0,0,0,0\n")).collect()
    }

    /// The same 3,000-character block written out twice in one reply (a model restating a file):
    /// a repeat, but one a span of 16,384 characters cannot hold eight copies of.
    fn a_long_block_twice() -> String {
        let block: String = (0..60)
            .map(|i| format!("line {i} of the configuration that matters.\n"))
            .collect();
        format!("{}{}{}", block.repeat(2), rust_file(), block.repeat(2))
    }

    #[test]
    fn long_ordinary_output_never_trips_the_watch() {
        for (name, text) in [
            ("code", rust_file()),
            ("table", markdown_table()),
            ("list", repeated_but_legit_list()),
            ("log", log_lines()),
            ("csv", csv_rows()),
            ("a block twice", a_long_block_twice()),
            (
                "prose",
                (0..1_500)
                    .map(|i| format!("Step {i} moved the next batch forward. "))
                    .collect::<String>(),
            ),
        ] {
            assert!(
                text.chars().count() > 2 * RUNAWAY_TEXT_CHARS_DEFAULT,
                "{name} is too short to test the watch"
            );
            assert_eq!(watch(&text, 31), None, "{name} tripped the watch");
        }
    }

    #[test]
    fn the_watch_turns_off_and_restarts() {
        assert!(RunawayWatch::new(0, 8).is_none(), "0 turns it off");
        assert!(
            RunawayWatch::new(16_384, 1).is_none(),
            "one copy is not a repeat"
        );
        let mut watch = RunawayWatch::new(64, 4).unwrap();
        let loop_text = "abcdefgh".repeat(20);
        assert!(watch.runaway_period(&loop_text, &loop_text).is_some());
        watch.reset();
        assert_eq!(watch.runaway_period("abc", "abc"), None);
    }

    /// A reply the server cut at its output limit keeps one copy only when the run is runaway-long:
    /// a cut reply ending in a few equal lines is left as it was.
    #[test]
    fn a_length_cut_reply_collapses_only_a_runaway_long_run() {
        let unit = "I'll confirm the field values were actually written. ";
        let runaway =
            Message::assistant().with_text(format!("Start.\n{}{STAMP}", unit.repeat(2_000)));
        let collapsed = collapse_runaway_reply(&[runaway], STAMP, RUNAWAY_TEXT_CHARS_DEFAULT);
        let text = collapsed[0].as_concat_text();
        assert_eq!(text.matches("I'll confirm").count(), 1, "{text}");
        assert!(text.ends_with(STAMP), "{text}");

        let short = format!("Done.\n{}{STAMP}", "| a | b |\n".repeat(3));
        let kept = collapse_runaway_reply(
            &[Message::assistant().with_text(short.clone())],
            STAMP,
            RUNAWAY_TEXT_CHARS_DEFAULT,
        );
        assert_eq!(kept[0].as_concat_text(), short);
    }

    #[test]
    fn thinking_and_text_both_collapse() {
        let unit = "Wait, let me reconsider the plan once more. ";
        let message = Message::assistant()
            .with_thinking(unit.repeat(10), "")
            .with_text(format!("{}{STAMP}", unit.repeat(10)));
        let collapsed = collapse_looped_reply(&[message], STAMP);
        for content in &collapsed[0].content {
            let text = match content {
                MessageContent::Text(t) => t.text.clone(),
                MessageContent::Thinking(t) => t.thinking.clone(),
                _ => continue,
            };
            assert_eq!(text.matches("reconsider").count(), 1, "{text}");
        }
    }
}
