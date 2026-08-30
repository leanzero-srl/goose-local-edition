//! SPEC-ENUMERATED FILE SETS: when the operator's spec freezes an exact deliverable list for an
//! area, the tree is measured against it.
//!
//! WHY (r5, reader 2): spec-build-sb7.md:374 says "Ship it as FOUR files, each owned and written
//! separately: `web/index.html` … `web/styles.css` … `web/app.js` … `web/viz.js`", and the 150 KB
//! budget (spec:844) enumerates the same four — yet web/ shipped FIVE files: brush.js (5,035 B)
//! rode outside the counted budget, and nothing measured the excess. The fifth file was a
//! documented plan decision ("the single shared brush-state module. CONTRACT OWNER."), which is
//! exactly why this is a FACT for REPAIR/the sink and never a refusal: an extra file may be
//! legitimate; the doc decides.
//!
//! HONEST PARSE, NO GUESSING: a set is extracted only when a paragraph carries "as <COUNT> files"
//! AND exactly COUNT distinct backticked paths sharing one top directory. A count that does not
//! match the paths it announces yields nothing — a wrong frozen set steered into a repair prompt
//! is worse than no set.
//!
//! FROZEN SPEC ONLY: the sets are derived from `spec_frozen` (the operator's words before any
//! model appended to the prompt — the #136 law) and persisted once to `.swarm/spec_sets.json`;
//! the roll-up re-derives the EXCESS from the tree NOW on every rebuild, the same
//! fixed-defect-vanishes rule `open_defects` lives by. An absent sidecar means the spec froze no
//! enumeration (persist writes whenever one exists) — honest-empty, nothing to measure.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct SpecFileSet {
    pub(super) area: String,
    pub(super) frozen: Vec<String>,
}

fn count_word(tok: &str) -> Option<usize> {
    if let Ok(n) = tok.parse::<usize>() {
        return Some(n);
    }
    Some(match tok.to_ascii_lowercase().as_str() {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        _ => return None,
    })
}

fn path_like(tok: &str) -> bool {
    tok.contains('/')
        && !tok.contains(char::is_whitespace)
        && !tok.contains('*')
        && !tok.contains('{')
        && tok
            .rsplit('/')
            .next()
            .is_some_and(|base| base.contains('.') && !base.starts_with('.'))
}

/// Every exact deliverable enumeration the spec freezes, count-verified. Paragraphs are
/// blank-line blocks; the announced count must equal the distinct backticked paths found, all
/// sharing one first path segment (the area).
pub(super) fn enumerated_file_sets(spec: &str) -> Vec<SpecFileSet> {
    let mut out: Vec<SpecFileSet> = Vec::new();
    for para in spec.split("\n\n") {
        let words: Vec<&str> = para
            .split(|c: char| !(c.is_ascii_alphanumeric()))
            .filter(|w| !w.is_empty())
            .collect();
        let mut announced: Option<usize> = None;
        for w in words.windows(3) {
            if w[0].eq_ignore_ascii_case("as") && w[2].eq_ignore_ascii_case("files") {
                if let Some(n) = count_word(w[1]) {
                    announced = Some(n);
                }
            }
        }
        let Some(n) = announced else { continue };
        if n < 2 {
            continue;
        }
        let mut paths: Vec<String> = Vec::new();
        for (i, seg) in para.split('`').enumerate() {
            if i % 2 == 1 && path_like(seg) && !paths.iter().any(|p| p == seg) {
                paths.push(seg.to_string());
            }
        }
        if paths.len() != n {
            continue;
        }
        let Some(area) = paths[0].split('/').next().map(String::from) else {
            continue;
        };
        if !paths.iter().all(|p| p.split('/').next() == Some(&area)) {
            continue;
        }
        if !out.iter().any(|s| s.area == area) {
            out.push(SpecFileSet {
                area,
                frozen: paths,
            });
        }
    }
    out
}

/// Files present in the set's area right now that the frozen enumeration does not name.
/// Relative paths, sorted; hidden files and tool litter excluded.
pub(super) fn set_exceeded(working_dir: &Path, set: &SpecFileSet) -> Vec<String> {
    let mut extra: Vec<String> = Vec::new();
    fn walk(dir: &Path, root: &Path, set: &SpecFileSet, extra: &mut Vec<String>, depth: usize) {
        if depth > 4 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "__pycache__" || name == "node_modules" {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, set, extra, depth + 1);
            } else if let Ok(rel) = p.strip_prefix(root) {
                let rel = rel.display().to_string();
                if !set.frozen.contains(&rel) {
                    extra.push(rel);
                }
            }
        }
    }
    walk(
        &working_dir.join(&set.area),
        working_dir,
        set,
        &mut extra,
        0,
    );
    extra.sort();
    extra
}

/// Derive the sets from the frozen spec and persist them once, for the roll-up's rebuilds (which
/// have the tree but not the spec). Nothing written when the spec froze no enumeration.
pub(super) fn persist(root: &Path, frozen_spec: &str) {
    let sets = enumerated_file_sets(frozen_spec);
    if sets.is_empty() {
        return;
    }
    let path = root.join(".swarm").join("spec_sets.json");
    let _ = std::fs::create_dir_all(root.join(".swarm"));
    if let Ok(bytes) = serde_json::to_string_pretty(&sets) {
        let _ = super::write_forming_atomic(&path, &bytes);
    }
}

/// One measured fact per exceeded set, re-derived from the tree NOW: `{area, frozen, extra}`.
/// Empty when the sidecar is absent (no enumeration was frozen) or every area holds exactly its
/// frozen files — the fixed-excess-vanishes rule.
pub(super) fn exceeded_facts(root: &Path) -> Vec<serde_json::Value> {
    let Ok(text) = std::fs::read_to_string(root.join(".swarm").join("spec_sets.json")) else {
        return Vec::new();
    };
    let Ok(sets) = serde_json::from_str::<Vec<SpecFileSet>>(&text) else {
        // An unreadable sidecar must not impersonate "no enumeration": name the breakage as the
        // fact itself so it surfaces in the same channel it was meant to feed.
        return vec![serde_json::json!({
            "area": "?",
            "frozen": [],
            "extra": [],
            "error": ".swarm/spec_sets.json exists but does not parse",
        })];
    };
    sets.iter()
        .filter_map(|s| {
            let extra = set_exceeded(root, s);
            (!extra.is_empty()).then(|| {
                serde_json::json!({
                    "area": s.area,
                    "frozen": s.frozen,
                    "extra": extra,
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "goose-specsets-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The REAL sb-7 spec: line 374's "Ship it as FOUR files" must yield exactly the four frozen
    /// web paths — the enumeration brush.js measurably rode outside on r5.
    #[test]
    fn the_real_sb7_spec_freezes_exactly_the_four_web_files() {
        let spec = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/swarm-bench/spec-build-sb7.md"),
        )
        .expect("the sb-7 spec is in the repo");
        let sets = enumerated_file_sets(&spec);
        assert_eq!(sets.len(), 1, "{sets:?}");
        assert_eq!(sets[0].area, "web");
        assert_eq!(
            sets[0].frozen,
            vec![
                "web/index.html".to_string(),
                "web/styles.css".to_string(),
                "web/app.js".to_string(),
                "web/viz.js".to_string(),
            ]
        );
    }

    /// A count that does not match its paths yields NOTHING — a wrong frozen set steered into a
    /// repair prompt is worse than no set. And prose without an enumeration yields nothing.
    #[test]
    fn a_mismatched_count_and_plain_prose_yield_no_set() {
        assert!(enumerated_file_sets(
            "Ship it as FOUR files: `web/a.js` and `web/b.js`.\n\nMore prose."
        )
        .is_empty());
        assert!(enumerated_file_sets("Build a great app. It should be fast.").is_empty());
        assert!(
            enumerated_file_sets("Ship it as TWO files: `web/a.js` and `docs/b.md`.").is_empty(),
            "paths spanning areas freeze nothing"
        );
    }

    /// THE r5 SHAPE END TO END: five files on disk where the spec froze four — brush.js is the
    /// measured extra; with the tree matching the enumeration the fact vanishes.
    #[test]
    fn brush_js_is_the_measured_extra_and_a_matching_tree_is_silent() {
        let dir = tmp("r5");
        let spec = "Ship it as FOUR files, each owned and written separately: `web/index.html` \
                    (structure only), `web/styles.css` (all styling), `web/app.js` (page \
                    behavior), and `web/viz.js` (the 3D engine, nothing else).";
        persist(&dir, spec);
        std::fs::create_dir_all(dir.join("web")).unwrap();
        for f in [
            "web/index.html",
            "web/styles.css",
            "web/app.js",
            "web/viz.js",
            "web/brush.js",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let facts = exceeded_facts(&dir);
        assert_eq!(facts.len(), 1, "{facts:?}");
        assert_eq!(facts[0]["area"], "web");
        assert_eq!(facts[0]["extra"], serde_json::json!(["web/brush.js"]));
        assert_eq!(facts[0]["frozen"].as_array().unwrap().len(), 4);

        std::fs::remove_file(dir.join("web/brush.js")).unwrap();
        assert!(
            exceeded_facts(&dir).is_empty(),
            "a tree matching the enumeration measures clean"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
