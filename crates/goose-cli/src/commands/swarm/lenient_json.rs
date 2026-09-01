//! The tolerant JSON parse every structured reply goes through (the opener, the synthesis
//! planner, the reviewer, each research lane). Sibling module under the incremental-split law
//! (development_gates::swarm_rs_line_count_only_decreases): moved verbatim from swarm.rs, paying
//! for the research-lane clause of the judge contract (NEXT names where to look, never the
//! answer) in the same commit.

/// A tolerant JSON object parse: takes the first balanced `{...}` in the reply and deserialises it.
/// The fleet wraps structured output in prose and fences often enough that a strict parse is a coin
/// flip, and losing a whole phase to a stray "Sure — here you go:" is not a trade worth making.
pub(super) fn parse_json_lenient<T: serde::de::DeserializeOwned>(raw: &str) -> Option<T> {
    if let Ok(v) = serde_json::from_str::<T>(raw) {
        return Some(v);
    }
    let obj = extract_first_json_object(raw)?;
    serde_json::from_str::<T>(&obj).ok()
}

fn extract_first_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let b = s.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0usize, false, false);
    for (i, &c) in b.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return s.get(start..=i).map(str::to_string);
                }
            }
            _ => {}
        }
    }
    None
}
