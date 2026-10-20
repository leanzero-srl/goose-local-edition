//! I-28 review: a reply the engine's loop guard stopped keeps ONE copy of the repeated text in the
//! history. The guard can let up to 4 x 1024 tokens of exact repeats through (plus the reasoning);
//! left in the history they bloat every later request and show the model the very loop the nudge
//! tells it not to repeat.

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
    messages
        .iter()
        .map(|message| {
            if message.role != Role::Assistant {
                return message.clone();
            }
            let mut message = message.clone();
            for content in message.content.iter_mut() {
                match content {
                    MessageContent::Text(text) => text.text = collapse_text(&text.text, stamp),
                    MessageContent::Thinking(thinking) => {
                        thinking.thinking = collapse_text(&thinking.thinking, stamp)
                    }
                    _ => {}
                }
            }
            message
        })
        .collect()
}

fn collapse_text(text: &str, stamp: &str) -> String {
    let stamp = stamp.trim();
    let (head, tail) = match text.find(stamp) {
        Some(at) => text.split_at(at),
        None => (text, ""),
    };
    let trimmed = head.trim_end();
    let spacing = head.get(trimmed.len()..).unwrap_or_default();
    match collapse_repeated_suffix(trimmed) {
        Some(collapsed) => format!("{collapsed}{spacing}{tail}"),
        None => text.to_string(),
    }
}

/// The text with a trailing run of exact copies cut to one copy plus a note, or `None` when it
/// does not end in one. Linear: a Z-function over the reversed text finds every period at once.
pub(crate) fn collapse_repeated_suffix(text: &str) -> Option<String> {
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
        let collapsed = collapse_text(&text, STAMP);
        assert_eq!(collapsed.matches("do it. First").count(), 1, "{collapsed}");
        assert!(collapsed.starts_with("Plan.\ndo it. First"), "{collapsed}");
        assert!(collapsed.contains("repeated 37 more times"), "{collapsed}");
        assert!(collapsed.ends_with(STAMP), "{collapsed}");
    }

    #[test]
    fn a_cut_last_copy_is_dropped_with_the_rest() {
        let unit = "x = compute(a, b); y = x + 1; ";
        let text = format!("{}{}", unit.repeat(5), unit.get(..9).unwrap());
        let collapsed = collapse_repeated_suffix(&text).unwrap();
        assert_eq!(collapsed.matches("compute").count(), 1, "{collapsed}");
    }

    #[test]
    fn text_without_a_long_loop_is_unchanged() {
        assert_eq!(
            collapse_repeated_suffix("A finished answer with no loop."),
            None
        );
        assert_eq!(collapse_repeated_suffix(&"ha ".repeat(50)), None);
        let twice = "a unit that is long enough. ".repeat(2);
        assert_eq!(collapse_repeated_suffix(&twice), None);
        let text = format!("{}{STAMP}", "nothing repeats here at all, honestly.");
        assert_eq!(collapse_text(&text, STAMP), text);
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
