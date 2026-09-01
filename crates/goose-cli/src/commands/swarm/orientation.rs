//! OPEN-1's ORIENTATION cluster: the spec cut by its OWN headings (`SpecSection`,
//! `spec_sections`), the arming floor that decides message FORMATION (never model work), the
//! compact index the opener and every research lane consume (`spec_orientation`), the
//! sentence-end head cut every prompt-facing excerpt shares, and the measured coverage gap
//! (`unclaimed_sections`).
//!
//! Third sibling module under the incremental-split law (development_gates::
//! swarm_rs_line_count_only_decreases): moved verbatim from swarm.rs — behavior unchanged, the
//! WHY of every part stays in each item's own doc — paying for the research fan's grounding
//! wiring (request file, snowball block, `research_context`, `phase: research`) in the same
//! commit.

use super::OpenOutput;

/// SYNTHESIS TAKES THE SLICES DIRECTLY (P1-5): each slice's brief IS the slice — its objective
/// plus its own questions, plus the decisions PARTITION (item 0: settled answers quoted verbatim;
/// only a still-open one keeps "choose the conventional option"). RESEARCH used to write these
/// briefs over 48 measured minutes (r2) with
/// 2 of 3 nodes idle, and its median-4,789-char paraphrases did not prevent the five wrong-key
/// defects — the real dependency source, which every worker now reads (dep_block + ledger block),
/// is the authority a paraphrase never was. Pure, so the straight line is testable without a model.
/// OPEN-1: one heading-delimited section of the operator's spec, cut by the document's OWN
/// structure (`#`..`######` headings; tables stay inside their section's body). The spec is
/// split by CODE so the opener never has to swallow the whole document to orient itself —
/// Mihai 08-30 07:30: "the benchmark prompt is ~50k tokens and the orientation is simple —
/// SPLIT THAT FILE and detail what needs detailing; OPEN must not swallow 50k in one prompt."
pub(super) struct SpecSection {
    pub(super) heading: String,
    pub(super) body: String,
    /// The heading's `#` count (1..=6; 0 for the heading-less preamble). Kept so the consumer
    /// routing (`research::consumed_spec_sections`) can read the document's OWN hierarchy — a
    /// claimed parent's children, the top-level sections that bind every slice — instead of
    /// guessing it from heading text. r6c's opener partitioned the 28 headings perfectly and
    /// the `####` children of §3 and §8 went to whichever slice named them, never to the
    /// slice that claimed the parent.
    pub(super) level: usize,
}

pub(super) fn spec_sections(spec: &str) -> Vec<SpecSection> {
    let mut out: Vec<SpecSection> = Vec::new();
    let mut cur = SpecSection {
        heading: String::new(),
        body: String::new(),
        level: 0,
    };
    for line in spec.lines() {
        let t = line.trim_start();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        let is_heading = (1..=6).contains(&hashes) && {
            let (_, rest) = t.split_at(hashes);
            rest.starts_with(' ') && !rest.trim().is_empty()
        };
        if is_heading {
            if !cur.heading.is_empty() || !cur.body.trim().is_empty() {
                out.push(cur);
            }
            let (_, rest) = t.split_at(hashes);
            cur = SpecSection {
                heading: rest.trim().to_string(),
                body: String::new(),
                level: hashes,
            };
        } else {
            cur.body.push_str(line);
            cur.body.push('\n');
        }
    }
    if !cur.heading.is_empty() || !cur.body.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// The document's TOP LEVEL: the shallowest heading level that occurs more than once — the
/// level the request's own sections sit at. A lone deeper-than-title heading is a title, not
/// a section level (sb-7: one `#`, five `##` → top level 2). None when no level repeats (a
/// document with no section structure to read).
pub(super) fn top_level(sections: &[SpecSection]) -> Option<usize> {
    (1..=6usize).find(|lvl| {
        sections
            .iter()
            .filter(|s| !s.heading.is_empty() && s.level == *lvl)
            .count()
            >= 2
    })
}

/// The indices of `parent`'s descendants: every section after it up to the next heading at
/// its level or shallower — the document's own nesting, so a `###` component's `####` details
/// are its children whatever their headings say.
pub(super) fn children_of(sections: &[SpecSection], parent: usize) -> Vec<usize> {
    let level = sections[parent].level;
    sections
        .iter()
        .enumerate()
        .skip(parent + 1)
        .take_while(|(_, s)| s.level > level)
        .map(|(i, _)| i)
        .collect()
}

/// OPEN-1's arming floor. NOT a cap on model work (nothing is bounded or terminated by it) —
/// it decides MESSAGE FORMATION only: below it, the whole spec is the better opener input and
/// the prompt stays byte-identical; above it, with real document structure to lean on, the
/// opener reads the orientation index and the engine splices each section's full text into the
/// briefs afterwards. 12k chars is comfortably above any toy spec and ~4x below sb-7's 54k.
const SPEC_ORIENTATION_MIN_CHARS: usize = 12_000;

pub(super) fn orientation_armed(spec: &str, sections: &[SpecSection]) -> bool {
    sections.len() >= 3 && spec.chars().count() >= SPEC_ORIENTATION_MIN_CHARS
}

/// The compact index the opener consumes when `orientation_armed`: every section's heading with
/// a measured size and a head excerpt ending at a SENTENCE boundary. The detail is not lost —
/// `briefs_from_slices` splices each claimed section's FULL text into the owning slice's brief,
/// verbatim.
pub(super) fn spec_orientation(sections: &[SpecSection]) -> String {
    let mut s = String::new();
    for sec in sections {
        let heading = if sec.heading.is_empty() {
            "(preamble)"
        } else {
            &sec.heading
        };
        let head = head_to_sentence_end(&sec.body, 400);
        s.push_str(&format!(
            "## {heading} [{} chars]\n{}\n\n",
            sec.body.chars().count(),
            head.trim_end()
        ));
    }
    s
}

/// A model-read head cut never ends mid-sentence — shared by the orientation index, the
/// slice-index summary, the judge's steer direction and the ledger row's final_text (the
/// head-cuts whose consumer is a prompt; cuts that feed only an event or a log line stay
/// hard cuts). The excerpt used to cut back to the last full LINE
/// inside the first 400 chars, and markdown wraps sentences across lines — measured on r5's
/// live opener (open.think.log ~4404): section 7's entry ended at "`web/index.html` (structure
/// only)," — the one sentence naming the four deliverable files, cut after the first — and the
/// opener re-litigated "owned and written separately" four times. The cut point now extends
/// FORWARD to the end of the sentence it lands in (terminator then whitespace) or to the
/// paragraph break, whichever comes first. NOT a cap on model work — message formation only.
/// Measured on the real sb-7 spec: the index grows 8,923 -> 14,074 chars, still under a third
/// of the 53,597-char document.
pub(super) fn head_to_sentence_end(body: &str, min_chars: usize) -> String {
    let chars: Vec<char> = body.chars().collect();
    if chars.len() <= min_chars {
        return body.to_string();
    }
    let mut end = min_chars;
    while end < chars.len() {
        let prev = chars[end - 1];
        let next = chars[end];
        if (matches!(prev, '.' | '!' | '?') && next.is_whitespace())
            || (prev == '\n' && next == '\n')
        {
            break;
        }
        end += 1;
    }
    chars[..end].iter().collect()
}

/// The key a claimed heading and a spec heading are compared on, at BOTH matching sites (the
/// splice and the coverage gap): markdown decoration goes — backticks, bold/italic stars, a
/// leading `#` run, trailing punctuation — the dash variants fold to one, whitespace collapses
/// and case folds. Letters stay: a typo is still a miss ("Bta" never becomes "Beta").
///
/// r6d's first tick: the opener claimed "vs7dbg — REQUIRED and graded" for web-page AND
/// viz-field against request.md:718 "#### `vs7dbg` — REQUIRED and graded", and the exact
/// string compare missed twice (`slice_claimed_section_unmatched` ×2): the 1,148-char section
/// of the graded debug API reached neither brief nor either slice's research prompts, while
/// every backtick-free heading the same slices claimed spliced fine.
pub(super) fn heading_key(heading: &str) -> String {
    let folded: String = heading
        .trim()
        .trim_start_matches('#')
        .chars()
        .filter(|c| !matches!(c, '`' | '*'))
        .map(|c| if matches!(c, '—' | '–') { '-' } else { c })
        .collect::<String>()
        .to_lowercase();
    folded
        .trim_end_matches(['.', ':', ';', ',', '!', '?'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The section headings NO slice claimed — the coverage gap, measured deterministically so it
/// can be an event instead of a hope that the opener's own read-back caught it.
pub(super) fn unclaimed_sections(opened: &OpenOutput, sections: &[SpecSection]) -> Vec<String> {
    let claimed: std::collections::HashSet<String> = opened
        .slices
        .iter()
        .flat_map(|sl| sl.sections.iter())
        .map(|h| heading_key(h))
        .collect();
    sections
        .iter()
        .filter(|s| !s.heading.is_empty())
        .filter(|s| !claimed.contains(&heading_key(&s.heading)))
        .map(|s| s.heading.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OPEN-1: on a real 54k-char spec the opener consumes an orientation index cut at the
    /// document's own headings — never the whole document — while a small spec stays
    /// byte-identical (not armed). The index carries every heading; tables stay in bodies.
    #[test]
    fn the_opener_orientation_replaces_the_whole_spec() {
        let spec = include_str!("../../../../../evals/swarm-bench/spec-build-sb7.md");
        let sections = spec_sections(spec);
        assert!(
            orientation_armed(spec, &sections),
            "sb-7 (54k chars, {} sections) arms the orientation",
            sections.len()
        );
        let orientation = spec_orientation(&sections);
        for sec in sections.iter().filter(|s| !s.heading.is_empty()) {
            assert!(
                orientation.contains(&sec.heading),
                "every heading is in the index: {}",
                sec.heading
            );
        }
        assert!(
            orientation.chars().count() * 3 < spec.chars().count(),
            "the index is a fraction of the document: {} vs {}",
            orientation.chars().count(),
            spec.chars().count()
        );
        assert!(
            sections.iter().any(|s| s.body.contains("/api/payments")),
            "the endpoint table lives inside a section body, not lost at the cut"
        );
        // r5 (open.think.log ~4404): the line-cut entry ended at "`web/index.html` (structure
        // only)," and the opener re-litigated the missing file list four times. The entry now
        // ends at a sentence boundary, so the four-file sentence survives whole.
        let s7 = sections
            .iter()
            .find(|s| s.heading.contains("frontend"))
            .expect("sb-7 section 7 names the frontend");
        let s7_entry = head_to_sentence_end(&s7.body, 400);
        for f in [
            "web/index.html",
            "web/styles.css",
            "web/app.js",
            "web/viz.js",
        ] {
            assert!(
                s7_entry.contains(f),
                "the four-file sentence rides whole in section 7's index entry: missing {f}"
            );
        }
        assert!(
            s7_entry.trim_end().ends_with(['.', '!', '?']),
            "the entry ends at a sentence boundary, never mid-list: ...{:?}",
            s7_entry
                .chars()
                .rev()
                .take(40)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        );
        let small = "# a\nbody\n# b\nbody\n# c\nbody\n";
        assert!(
            !orientation_armed(small, &spec_sections(small)),
            "a small spec keeps the whole-text opener prompt byte-identical"
        );
        // The document's own hierarchy, read for the consumer routing: sb-7's one `#` is the
        // title, its five `##` the top level; §3's children are its five `####` details.
        assert_eq!(top_level(&sections), Some(2));
        let s3 = sections
            .iter()
            .position(|s| s.heading.starts_with("3. `ledgerd`"))
            .unwrap();
        assert_eq!(sections[s3].level, 3);
        let kids: Vec<&str> = children_of(&sections, s3)
            .into_iter()
            .map(|i| sections[i].heading.as_str())
            .collect();
        assert_eq!(
            kids,
            vec![
                "Sync discipline",
                "Endpoints",
                "The event ledger",
                "The outbox",
                "Error envelope"
            ]
        );
        assert_eq!(top_level(&spec_sections(small)), Some(1));
        assert_eq!(
            top_level(&spec_sections(
                "# only
x
"
            )),
            None
        );
    }
}
